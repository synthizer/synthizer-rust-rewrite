use std::hash::Hash;

/// Computes the execution order of nodes in a graph
///
/// We divide nodes into stages,.  This type can return the execution order of some set of nodes such that each stage
/// contains nodes which can execute in arbitrary order with each other, but are blocked by and block the previous/next
/// stages.
///
/// Internally, this type is a reusable buffer and a mostly stateless function which takes a graph traverser, which
/// executes a bredth-first search.  We choose bredth-first searching here because we can use the tail of the buffer to
/// expand the nodes, which means that most of the algorithm occurs in a cache-friendly fashion.  At the end, we sort
/// the vec and deduplicate.
///
/// The constraint on the type here is `Eq + Ord + Hash`: this allows future-proofing.
///
/// Internally, stages are `u16`, where the first stage is `u16::MAX` and we count down.
pub(crate) struct BfsStager<N: Copy + Eq + Ord + std::hash::Hash> {
    buffer: Vec<(u16, N)>,

    /// Once we find something below this number of stages, panicn and assume there was a cycle.
    ///
    /// Since this is for audio applications, we can bound the depth of the graph.
    min_stage: u16,
}

impl<N: Copy + Eq + Ord + Hash> BfsStager<N> {
    pub fn new(capacity: usize, max_depth: u16) -> Self {
        BfsStager {
            buffer: Vec::with_capacity(capacity),
            min_stage: u16::MAX - max_depth,
        }
    }

    /// Clear this stager for the next time it will be reused.  Does not deallocate.
    pub(crate) fn clear(&mut self) {
        self.buffer.clear();
    }

    /// Execute a bredth-first search, preparing to be able to yield nodes in execution order.
    pub(crate) fn execute(&mut self, policy: &impl BfsPolicy<Node = N>) {
        self.clear();

        // Put the roots into the vec.
        policy.determine_roots(|n| {
            self.buffer.push((u16::MAX, n));
        });
        self.bfs_recurse(policy, 0, u16::MAX - 1);

        // First, sort by the node then by the stage.  This allows deduplicating to find the lowest stage of each nodee
        // with a simple slice op.
        self.buffer.sort_unstable_by_key(|x| (x.1, x.0));

        // This dedup keeps the leftmost, which is the earliest position at which any node must execute.
        self.buffer.dedup_by(|a, b| a.1 == b.1);

        // Now a normal sort gets nodes in stage order.
        self.buffer.sort_unstable();
    }

    /// The recursive step of the bfs search.
    fn bfs_recurse(&mut self, policy: &impl BfsPolicy<Node = N>, fringe_start: usize, stage: u16) {
        assert!(
            stage >= self.min_stage,
            "Found a cycle or a graph which is too deep to handle. Only allowing a max depth of {}",
            self.min_stage
        );

        let fringe_stop = self.buffer.len();

        for i in fringe_start..fringe_stop {
            policy.determine_dependencies(self.buffer[i].1, |n| self.buffer.push((stage, n)));
        }

        if fringe_stop < self.buffer.len() {
            self.bfs_recurse(policy, fringe_stop, stage - 1);
        }
    }

    /// Iterate over all nodes in execution order.
    fn iter(&self) -> impl Iterator<Item = N> + '_ {
        self.buffer.iter().map(|x| x.1)
    }
}

pub(crate) trait BfsPolicy {
    type Node: Copy + Eq + Ord + Hash;

    /// call the provided closure with all roots of the search, those nodes which can execute last.
    ///
    /// Put another way, call the provided closure with any node that has no outgoing edges.
    fn determine_roots(&self, callback: impl FnMut(Self::Node));

    /// Call the provided closure with all dependencies of the specified node.
    fn determine_dependencies(&self, node: Self::Node, callback: impl FnMut(Self::Node));
}

#[cfg(test)]
mod tests {
    use super::*;

    use petgraph::{
        stable_graph::{NodeIndex, StableDiGraph},
        visit::EdgeRef,
    };
    use proptest::prelude::*;
    use proptest::proptest;

    use std::collections::{HashMap, HashSet};

    fn edges_to_graph(edges: &[(u8, u8)]) -> (StableDiGraph<u8, ()>, HashMap<u8, NodeIndex>) {
        let mut out = StableDiGraph::default();
        let mut nc = HashMap::new();
        let mut ec = HashSet::new();

        for (a, b) in edges.iter() {
            if ec.insert((*a, *b)) {
                continue;
            }

            let n1 = *nc.entry(*a).or_insert_with(|| out.add_node(*a));
            let n2 = *nc.entry(*b).or_insert_with(|| out.add_node(*b));
            out.add_edge(n1, n2, ());
        }

        let cycles = petgraph::algo::feedback_arc_set::greedy_feedback_arc_set(&out)
            .map(|x| x.id())
            .collect::<Vec<_>>();
        for r in cycles {
            out.remove_edge(r);
        }

        (out, nc)
    }

    struct TestPolicy<'a> {
        graph: &'a StableDiGraph<u8, ()>,
        map: &'a HashMap<u8, NodeIndex>,
    }

    impl<'a> BfsPolicy for TestPolicy<'a> {
        type Node = u8;

        fn determine_roots(&self, mut callback: impl FnMut(Self::Node)) {
            for n in self.graph.node_indices() {
                if self.graph.edges(n).next().is_none() {
                    let data = self.graph.node_weight(n).unwrap();
                    callback(*data);
                }
            }
        }

        fn determine_dependencies(&self, node: Self::Node, mut callback: impl FnMut(Self::Node)) {
            let nref = self.map.get(&node).unwrap();
            for e in self
                .graph
                .edges_directed(*nref, petgraph::Direction::Incoming)
            {
                let data = self.graph.node_weight(e.source()).unwrap();
                callback(*data);
            }
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig{cases:1000, ..Default::default()})]
        #[test]
        fn test_bfs_stager(
            edges in prop::collection::vec((0..u8::MAX, 0..u8::MAX), 0..10000),
        ) {
            let (graph, map) = edges_to_graph(&edges[..]);
            let policy = TestPolicy{
                graph: &graph,
                map: &map,
            };
            let mut stager = BfsStager::<u8>::new(0, u16::MAX);
            stager.execute(&policy);
            let proposal = stager.iter().collect::<Vec<_>>();
            prop_assert_eq!(proposal.len(), graph.node_count());

            // First: did we execute duplicates?
            let possible_dups = proposal.iter().collect::<HashSet<_>>();
            prop_assert_eq!(proposal.len(), possible_dups.len());

            // The easiest way to test the ordering is to check that we can "execute" nodes.
            let mut executed = HashSet::<u8>::new();
            for n in proposal {
                let nref = map.get(&n).unwrap();
                graph
                    .edges_directed(*nref, petgraph::Direction::Incoming)
                    .map(|x| graph.node_weight(x.source()).unwrap())
                    .for_each(|dep| {
                        assert!(executed.contains(dep));
                    });
                executed.insert(n);
            }

            prop_assert_eq!(executed.len(), graph.node_count());
        }
    }
}
