/// A map of edges.
///
/// This is a realtime-safe collection of edges which only ever grows if the initial capacity is exhausted.  The
/// underlying implementation, however, is currently `O(edges)` on all operations.  This will be fixed; it's just to get
/// us off the ground.
pub(crate) struct EdgeMap<E: Edge> {
    edges: Vec<E>,
}

/// AN edge in an edge map, with an outgoing and incoming endpoint.
pub(crate) trait Edge {
    type Outgoing: Eq + Ord + std::hash::Hash;
    type Incoming: Eq + Ord + std::hash::Hash;

    fn get_outgoing(&self) -> &Self::Outgoing;
    fn get_incoming(&self) -> &Self::Incoming;
}

/// A generalized endpoint is like the node in a (node, input) tuple.
///
/// All outgoing and incoming types should generalize to themselves, plus any auxiliary data, e.g. nodes.
///
/// Generalizations should be such that a sorted sequence of the more specific value produces a sorted sequence of the
/// generalized value.  The current implementation does not rely on this because it is `O(N)` on everything, but a more
/// efficient implementation is likely to be based on binary searches, where the generalized sequences need to be sorted
/// if the more specific ones are.  In practice, it is okay to generalize to a node, here node meaning ordered
/// identifier, but also generalizing to an input wouldn't work since many nodes can have a 2nd input.  For our use case
/// this is fine: we only ever need to generalize to nodes/node-like things (it would not make sense to do anything with
/// "everything connected to a 2nd input").
pub(crate) trait GeneralizedEndpoint<T> {
    fn generalize(&self) -> &T;
}

impl<E: Edge> EdgeMap<E> {
    pub(crate) fn new(capacity: usize) -> Self {
        EdgeMap {
            edges: Vec::with_capacity(capacity),
        }
    }

    /// Find an edge with a given incoming and outgoing value.
    fn find(&self, outgoing: &E::Outgoing, incoming: &E::Incoming) -> Option<usize> {
        self.edges
            .iter()
            .enumerate()
            .find(|(_, e)| e.get_incoming() == incoming && e.get_outgoing() == outgoing)
            .map(|x| x.0)
    }

    /// Insert or replace an edge from a given incoming source to a given outgoing destination, returning the old edge.
    pub fn upsert(&mut self, edge: E) -> Option<E> {
        match self.find(edge.get_outgoing(), edge.get_incoming()) {
            Some(x) => {
                let mut new = edge;
                std::mem::swap(&mut new, &mut self.edges[x]);
                Some(new)
            }
            None => {
                self.edges.push(edge);
                None
            }
        }
    }

    /// Remove an edge from the edge map, if present. Returns the old edge if any.
    fn remove(&mut self, outgoing: &E::Outgoing, incoming: &E::Incoming) -> Option<E> {
        let ind = self.find(outgoing, incoming)?;
        Some(self.edges.remove(ind))
    }

    /// Iterate over all outgoing edges for a given outgoing value, potentially generalized.
    ///
    /// For example, "give me all things output 2 of node a is connected to", or "all things any output of node a is connected to"
    fn iter_outgoing<'a, Pred>(&'a self, outgoing: &'a Pred) -> impl Iterator<Item = &E> + 'a
    where
        E::Outgoing: GeneralizedEndpoint<Pred>,
        Pred: PartialEq,
    {
        self.edges
            .iter()
            .filter(move |e| e.get_outgoing().generalize() == outgoing)
    }

    /// Iterate over all incoming edges for a given incoming value, potentially generalized.
    ///
    /// For example, "give me all things connected to input 2 of node a", or "give me all things connected to an input
    /// of node a".
    fn iter_incoming<'a, Pred>(&'a self, incoming: &'a Pred) -> impl Iterator<Item = &E> + 'a
    where
        E::Incoming: GeneralizedEndpoint<Pred>,
        Pred: PartialEq,
    {
        self.edges
            .iter()
            .filter(move |e| e.get_incoming().generalize() == incoming)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
    struct NodeInput {
        node: u8,
        input: u8,
    }

    #[derive(Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
    struct NodeOutput {
        node: u8,
        output: u8,
    }

    #[derive(Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
    struct Connection {
        output: NodeOutput,
        input: NodeInput,
    }

    impl GeneralizedEndpoint<NodeInput> for NodeInput {
        fn generalize(&self) -> &NodeInput {
            self
        }
    }

    impl GeneralizedEndpoint<NodeOutput> for NodeOutput {
        fn generalize(&self) -> &NodeOutput {
            self
        }
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

        let mut edges = map
            .iter_outgoing(&NodeOutput { node: 1, output: 0 })
            .collect::<Vec<_>>();
        edges.sort_unstable();
        assert_eq!(edges, vec![&conn(1, 0, 2, 1), &conn(1, 0, 3, 0)]);
    }
}
