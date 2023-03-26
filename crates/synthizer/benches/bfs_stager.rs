use std::collections::HashSet;

use criterion::{criterion_group, criterion_main, Criterion, Throughput};

use synthizer::bench_reexport::{
    data_structures::bfs_stager::*, data_structures::edgemap, unique_id::UniqueId,
};

struct Case {
    sources: usize,
    effects: usize,
}

const CASES: &[Case] = &[
    Case {
        sources: 16,
        effects: 0,
    },
    Case {
        sources: 16,
        effects: 4,
    },
    Case {
        sources: 256,
        effects: 0,
    },
    Case {
        sources: 256,
        effects: 8,
    },
];

#[derive(Copy, Clone, Eq, Ord, PartialEq, PartialOrd, Hash)]
struct BenchEdge {
    outgoing: UniqueId,
    incoming: UniqueId,
}

impl edgemap::Edge for BenchEdge {
    type Outgoing = UniqueId;
    type Incoming = UniqueId;

    fn get_outgoing(&self) -> &Self::Outgoing {
        &self.outgoing
    }

    fn get_incoming(&self) -> &Self::Incoming {
        &self.incoming
    }
}

struct BenchPolicy<'a> {
    nodes: &'a HashSet<UniqueId>,
    edges: &'a edgemap::EdgeMap<BenchEdge>,
}

impl<'a> BfsPolicy for BenchPolicy<'a> {
    type Node = UniqueId;

    fn determine_roots(&self, mut callback: impl FnMut(Self::Node)) {
        for i in self.nodes.iter() {
            if self.edges.iter_outgoing(i).next().is_none() {
                callback(*i);
            }
        }
    }

    fn determine_dependencies(&self, node: Self::Node, mut callback: impl FnMut(Self::Node)) {
        for i in self.edges.iter_incoming(&node) {
            callback(i.outgoing);
        }
    }
}

pub fn bfs_searcher(c: &mut Criterion) {
    let mut group = c.benchmark_group("bfs_stager");

    for case in CASES.iter() {
        // This is number of plans per sec.
        group.throughput(Throughput::Elements(1));
        group.bench_with_input(
            criterion::BenchmarkId::from_parameter(format!(
                "sources={} effects={}",
                case.sources, case.effects
            )),
            &(case,),
            |b, (case,)| {
                let mut graph = edgemap::EdgeMap::<BenchEdge>::new(case.sources * 2 + case.effects);

                // Our fake buffer nodes.
                let buffer_nodes = (0..case.sources)
                    .map(|_| UniqueId::new())
                    .collect::<Vec<_>>();
                let source_nodes = (0..case.sources)
                    .map(|_| UniqueId::new())
                    .collect::<Vec<_>>();
                let effect_nodes = (0..case.effects)
                    .map(|_| UniqueId::new())
                    .collect::<Vec<_>>();
                let audio_output_node = UniqueId::new();

                for (b, s) in buffer_nodes.iter().zip(source_nodes.iter()) {
                    graph.upsert(BenchEdge {
                        outgoing: *b,
                        incoming: *s,
                    });
                }

                for s in source_nodes.iter() {
                    graph.upsert(BenchEdge {
                        outgoing: *s,
                        incoming: audio_output_node,
                    });

                    for e in effect_nodes.iter() {
                        graph.upsert(BenchEdge {
                            outgoing: *s,
                            incoming: *e,
                        });
                    }
                }

                for e in effect_nodes.iter() {
                    graph.upsert(BenchEdge {
                        outgoing: *e,
                        incoming: audio_output_node,
                    });
                }

                graph.sort();

                let nodes = source_nodes
                    .into_iter()
                    .chain(buffer_nodes.into_iter())
                    .chain(effect_nodes.into_iter())
                    .chain(std::iter::once(audio_output_node))
                    .collect::<HashSet<_>>();

                let mut bfs = BfsStager::<UniqueId>::new(nodes.len(), u16::MAX);
                let mut workspace = Vec::with_capacity(nodes.len());

                b.iter(|| {
                    let policy = BenchPolicy {
                        nodes: &nodes,
                        edges: &graph,
                    };
                    bfs.execute(&policy);
                    workspace.extend(bfs.iter());
                    assert_eq!(workspace.len(), nodes.len());
                    workspace.clear();
                });
            },
        );
    }
}

criterion_group!(benches, bfs_searcher);
criterion_main!(benches);
