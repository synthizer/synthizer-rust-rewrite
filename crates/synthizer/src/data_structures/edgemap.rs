use ahash::{HashMap, HashMapExt};

/// A map of edges.
///
/// This is a realtime-safe collection of edges which only ever grows if the initial capacity is exhausted.  It is
/// backed by lazily-sorted vecs and a hashmap.  Usage proceeds in two phases: it is mutated by whatever, [EdgeMap::sort] is called,
/// and then immutable access is permitted.  Put another way, all immutable access must first be preceeded by
/// [EdgeMap::sort].
pub struct EdgeMap<E: Edge> {
    edges: HashMap<(E::Outgoing, E::Incoming), E>,

    by_outgoing: Vec<(E::Outgoing, E::Incoming)>,

    /// Edges in the map sorted by (incoming, outgoing).
    by_incoming: Vec<(E::Incoming, E::Outgoing)>,

    /// True if the vecs are sorted.
    sorted: bool,

    /// True if we need to run retain on the index vectors because of a removal.
    needs_retain: bool,
}

/// AN edge in an edge map, with an outgoing and incoming endpoint.
pub trait Edge {
    type Outgoing: Copy + Eq + Ord + std::hash::Hash;
    type Incoming: Copy + Eq + Ord + std::hash::Hash;

    fn get_outgoing(&self) -> &Self::Outgoing;
    fn get_incoming(&self) -> &Self::Incoming;
}

/// A generalized endpoint is like the node in a (node, input) tuple.
///
/// All outgoing and incoming types should generalize to themselves, plus any auxiliary data, e.g. nodes.
/// Self-generalization is handled automatically via a blanket impl.
///
/// Generalizations should be such that a sorted sequence of the more specific value produces a sorted sequence of the
/// generalized value.  The current implementation does not rely on this because it is `O(N)` on everything, but a more
/// efficient implementation is likely to be based on binary searches, where the generalized sequences need to be sorted
/// if the more specific ones are.  In practice, it is okay to generalize to a node, here node meaning ordered
/// identifier, but also generalizing to an input wouldn't work since many nodes can have a 2nd input.  For our use case
/// this is fine: we only ever need to generalize to nodes/node-like things (it would not make sense to do anything with
/// "everything connected to a 2nd input").
pub trait GeneralizedEndpoint<T: Copy> {
    fn generalize(&self) -> &T;
}

impl<T: Copy> GeneralizedEndpoint<T> for T {
    fn generalize(&self) -> &T {
        self
    }
}

impl<E: Edge> EdgeMap<E> {
    pub fn new(capacity: usize) -> Self {
        EdgeMap {
            edges: HashMap::with_capacity(capacity),
            by_outgoing: Vec::with_capacity(capacity),
            by_incoming: Vec::with_capacity(capacity),
            sorted: true,
            needs_retain: false,
        }
    }

    /// Sort the vecs if required, and drop tombstones.
    ///
    /// Should be called after mutation, for immutable access.
    pub fn maintenance(&mut self) {
        if self.needs_retain {
            self.by_outgoing.retain(|i| self.edges.contains_key(i));
            self.by_incoming
                .retain(|i| self.edges.contains_key(&(i.1, i.0)));
            self.needs_retain = false;
        }

        if !self.sorted {
            self.by_outgoing.sort_unstable();
            self.by_incoming.sort_unstable();

            self.sorted = true;
        }
    }

    fn assert_maintenance_done(&self) {
        assert!(
            self.sorted && !self.needs_retain,
            ".maintenance() must be called after mutating and before reading"
        );
    }

    /// Insert or replace an edge from a given incoming source to a given outgoing destination, returning the old edge.
    pub fn upsert(&mut self, edge: E) -> Option<E> {
        let k = (*edge.get_outgoing(), *edge.get_incoming());
        match self.edges.insert(k, edge) {
            Some(e) => Some(e),
            None => {
                self.by_outgoing.push(k);
                self.by_incoming.push((k.1, k.0));
                self.sorted = false;
                None
            }
        }
    }

    /// Remove an edge from the edge map, if present.  Returns the old edge value.
    fn remove(&mut self, outgoing: &E::Outgoing, incoming: &E::Incoming) -> Option<E> {
        let k = (*outgoing, *incoming);
        match self.edges.remove(&k) {
            None => None,
            Some(x) => {
                self.needs_retain = true;
                Some(x)
            }
        }
    }

    /// Drop all outgoing edges with the specified (possbily generalized) endpoint.
    pub fn remove_outgoing<Pred>(&mut self, outgoing: &Pred)
    where
        E::Outgoing: GeneralizedEndpoint<Pred>,
        Pred: Ord + Copy,
    {
        self.maintenance();
        let outgoing_ind = self
            .by_outgoing
            .partition_point(|x| x.0.generalize() < outgoing);
        for i in &self.by_outgoing[outgoing_ind..self.by_outgoing.len()] {
            if i.0.generalize() != outgoing {
                break;
            }

            self.edges.remove(i);
        }

        self.needs_retain = true;
    }

    /// Drop all incoming edges with the specified (possbily generalized) endpoint.
    pub fn remove_incoming<Pred>(&mut self, incoming: &Pred)
    where
        E::Incoming: GeneralizedEndpoint<Pred>,
        Pred: Ord + Copy,
    {
        self.maintenance();
        let incoming_ind = self
            .by_incoming
            .partition_point(|x| x.0.generalize() < incoming);
        for i in &self.by_incoming[incoming_ind..self.by_outgoing.len()] {
            if i.0.generalize() != incoming {
                break;
            }

            self.edges.remove(&(i.1, i.0));
        }

        self.needs_retain = true;
    }

    /// Iterate over all outgoing edges for a given outgoing value, potentially generalized.
    ///
    /// For example, "give me all things output 2 of node a is connected to", or "all things any output of node a is
    /// connected to"
    pub fn iter_outgoing<'a, Pred>(&'a self, outgoing: &'a Pred) -> impl Iterator<Item = &E> + 'a
    where
        E::Outgoing: GeneralizedEndpoint<Pred>,
        Pred: Ord + Copy,
    {
        self.assert_maintenance_done();
        let outgoing_ind = self
            .by_outgoing
            .partition_point(|x| x.0.generalize() < outgoing);

        (outgoing_ind..self.by_outgoing.len())
            .map(|i| self.edges.get(&self.by_outgoing[i]).unwrap())
            .take_while(move |e| e.get_outgoing().generalize() == outgoing)
    }

    /// Iterate over all incoming edges for a given incoming value, potentially generalized.
    ///
    /// For example, "give me all things connected to input 2 of node a", or "give me all things connected to an input
    /// of node a".
    pub fn iter_incoming<'a, Pred>(&'a self, incoming: &'a Pred) -> impl Iterator<Item = &E> + 'a
    where
        E::Incoming: GeneralizedEndpoint<Pred>,
        Pred: Ord + Copy,
    {
        self.assert_maintenance_done();
        let incoming_ind = self
            .by_incoming
            .partition_point(|x| x.0.generalize() < incoming);
        (incoming_ind..self.by_incoming.len())
            .map(|i| {
                let k = (self.by_incoming[i].1, self.by_incoming[i].0);
                self.edges.get(&k).unwrap()
            })
            .take_while(move |e| e.get_incoming().generalize() == incoming)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Copy, Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
    struct NodeInput {
        node: u8,
        input: u8,
    }

    #[derive(Copy, Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
    struct NodeOutput {
        node: u8,
        output: u8,
    }

    #[derive(Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
    struct Connection {
        output: NodeOutput,
        input: NodeInput,
    }

    impl GeneralizedEndpoint<u8> for NodeInput {
        fn generalize(&self) -> &u8 {
            &self.node
        }
    }

    impl GeneralizedEndpoint<u8> for NodeOutput {
        fn generalize(&self) -> &u8 {
            &self.node
        }
    }

    impl Edge for Connection {
        type Outgoing = NodeOutput;
        type Incoming = NodeInput;

        fn get_incoming(&self) -> &Self::Incoming {
            &self.input
        }

        fn get_outgoing(&self) -> &Self::Outgoing {
            &self.output
        }
    }

    fn conn(node: u8, output: u8, node2: u8, input: u8) -> Connection {
        Connection {
            output: NodeOutput { node, output },
            input: NodeInput { node: node2, input },
        }
    }

    #[test]
    fn test_edgemap() {
        let mut map = EdgeMap::<Connection>::new(100);

        map.upsert(conn(1, 0, 2, 0));
        map.upsert(conn(1, 0, 3, 0));
        map.upsert(conn(1, 0, 2, 1));
        map.upsert(conn(1, 1, 3, 0));
        map.upsert(conn(2, 0, 3, 0));
        map.upsert(conn(2, 1, 3, 0));
        map.upsert(conn(2, 1, 3, 1));
        map.maintenance();

        let mut edges = map
            .iter_outgoing(&NodeOutput { node: 1, output: 0 })
            .collect::<Vec<_>>();
        edges.sort_unstable();
        assert_eq!(
            edges,
            vec![&conn(1, 0, 2, 0), &conn(1, 0, 2, 1), &conn(1, 0, 3, 0)]
        );

        // These edges generalize so that a u8 is the node id.
        let mut edges = map.iter_outgoing(&1).collect::<Vec<_>>();
        edges.sort_unstable();
        assert_eq!(
            edges,
            vec![
                &conn(1, 0, 2, 0),
                &conn(1, 0, 2, 1),
                &conn(1, 0, 3, 0),
                &conn(1, 1, 3, 0)
            ]
        );

        let mut edges = map
            .iter_incoming(&NodeInput { node: 3, input: 0 })
            .collect::<Vec<_>>();
        edges.sort_unstable();
        assert_eq!(
            edges,
            vec![
                &conn(1, 0, 3, 0),
                &conn(1, 1, 3, 0),
                &conn(2, 0, 3, 0),
                &conn(2, 1, 3, 0)
            ]
        );

        let mut edges = map.iter_incoming(&3).collect::<Vec<_>>();
        edges.sort_unstable();
        assert_eq!(
            edges,
            vec![
                &conn(1, 0, 3, 0),
                &conn(1, 1, 3, 0),
                &conn(2, 0, 3, 0),
                &conn(2, 1, 3, 0),
                &conn(2, 1, 3, 1),
            ]
        );

        map.remove(
            &NodeOutput { node: 1, output: 0 },
            &NodeInput { node: 2, input: 0 },
        )
        .expect("Should have removed");
        map.maintenance();

        let mut edges = map
            .iter_outgoing(&NodeOutput { node: 1, output: 0 })
            .collect::<Vec<_>>();
        edges.sort_unstable();
        assert_eq!(edges, vec![&conn(1, 0, 2, 1), &conn(1, 0, 3, 0)]);
    }
}
