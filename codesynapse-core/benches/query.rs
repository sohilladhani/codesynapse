use criterion::{black_box, criterion_group, criterion_main, Criterion};

use codesynapse_core::benchmark::{query_subgraph_tokens, SAMPLE_QUESTIONS};
use codesynapse_core::graph::{GraphStore, MemoryGraphStore};
use codesynapse_core::query::QueryEngine;
use codesynapse_core::types::{Edge, Node};

fn make_store(node_count: usize, edge_count: usize) -> MemoryGraphStore {
    let store = MemoryGraphStore::new();
    for i in 0..node_count {
        store
            .add_node(Node {
                id: format!("node_{i}"),
                label: format!("symbol_{i}"),
                file_type: "Code".to_string(),
                source_file: format!("src/mod_{}.rs", i % 200),
                source_location: Some(format!("L{}", i * 3)),
                community: Some(i % 20),
                rationale: None,
                docstring: None,
                metadata: Default::default(),
            })
            .unwrap();
    }
    for i in 0..edge_count {
        store
            .add_edge(Edge {
                source: format!("node_{}", i % node_count),
                target: format!("node_{}", (i + 7) % node_count),
                relation: "calls".to_string(),
                confidence: "EXTRACTED".to_string(),
                source_file: None,
                weight: 1.0,
                context: None,
            })
            .unwrap();
    }
    store
}

fn bench_bfs_10k(c: &mut Criterion) {
    let store = make_store(10_000, 15_000);
    let engine = QueryEngine::new(Box::new(store));
    c.bench_function("bfs_10k_nodes", |b| {
        b.iter(|| {
            black_box(
                engine
                    .query_text("symbol_0", "bfs", 3, Some(50_000), None)
                    .unwrap(),
            )
        })
    });
}

fn bench_dijkstra_10k(c: &mut Criterion) {
    let store = make_store(10_000, 15_000);
    let engine = QueryEngine::new(Box::new(store));
    c.bench_function("dijkstra_10k_nodes", |b| {
        b.iter(|| {
            black_box(
                engine
                    .shortest_path_with_max_hops("node_0", "node_9999", 20)
                    .unwrap(),
            )
        })
    });
}

fn bench_token_reduction_1k(c: &mut Criterion) {
    let nodes: Vec<serde_json::Value> = (0..1_000)
        .map(|i| {
            serde_json::json!({
                "id": format!("n{i}"),
                "label": format!("symbol_{i}"),
                "source_file": format!("src/mod_{}.rs", i % 50),
                "source_location": format!("L{}", i * 3),
                "community": i % 10
            })
        })
        .collect();
    let links: Vec<serde_json::Value> = (0..1_500)
        .map(|i| {
            serde_json::json!({
                "source": format!("n{}", i % 1_000),
                "target": format!("n{}", (i + 7) % 1_000),
                "relation": "calls"
            })
        })
        .collect();
    let graph_data = serde_json::json!({"nodes": nodes, "links": links});

    c.bench_function("token_reduction_1k_nodes", |b| {
        b.iter(|| {
            for q in SAMPLE_QUESTIONS {
                black_box(query_subgraph_tokens(&graph_data, q, 3));
            }
        })
    });
}

criterion_group!(
    query_benches,
    bench_bfs_10k,
    bench_dijkstra_10k,
    bench_token_reduction_1k
);
criterion_main!(query_benches);
