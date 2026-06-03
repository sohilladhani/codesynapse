use criterion::{black_box, criterion_group, criterion_main, Criterion};

use codesynapse_core::build::GraphBuilder;
use codesynapse_core::dedup::Deduplicator;
use codesynapse_core::detect::Detector;
use codesynapse_core::export::Exporter;
use codesynapse_core::extract::Extractor;
use codesynapse_core::graph::MemoryGraphStore;
use codesynapse_core::types::{Edge, Node};

fn make_node(i: usize) -> Node {
    Node {
        id: format!("node_{i}"),
        label: format!("function_{i}"),
        file_type: "Code".to_string(),
        source_file: format!("src/file_{}.rs", i % 100),
        source_location: Some(format!("L{i}")),
        community: Some(i % 10),
        rationale: None,
        docstring: None,
        metadata: Default::default(),
    }
}

fn make_edge(i: usize, node_count: usize) -> Edge {
    Edge {
        source: format!("node_{}", i % node_count),
        target: format!("node_{}", (i + 1) % node_count),
        relation: "calls".to_string(),
        confidence: "INFERRED".to_string(),
        source_file: None,
        weight: 1.0,
        context: None,
    }
}

fn bench_detect_1k(c: &mut Criterion) {
    use std::fs;
    let dir = tempfile::tempdir().unwrap();
    for i in 0..1_000 {
        let path = dir.path().join(format!("file_{i}.py"));
        fs::write(&path, format!("def fn_{i}(): pass\n")).unwrap();
    }
    let det = Detector::new(dir.path());
    c.bench_function("detect_1k_files", |b| {
        b.iter(|| black_box(det.discover(dir.path()).unwrap()))
    });
}

fn bench_extract_100(c: &mut Criterion) {
    use std::fs;
    let dir = tempfile::tempdir().unwrap();
    let mut paths = Vec::new();
    for i in 0..100 {
        let path = dir.path().join(format!("module_{i}.py"));
        let src =
            format!("import os\ndef func_{i}(x, y):\n    return x + y\nclass Cls_{i}:\n    pass\n");
        fs::write(&path, &src).unwrap();
        paths.push((path, src.into_bytes()));
    }
    let ext = Extractor::new();
    c.bench_function("extract_100_files", |b| {
        b.iter(|| {
            let mut frags = Vec::new();
            for (p, src) in &paths {
                if let Ok(f) = ext.extract_file(p, src) {
                    frags.push(f);
                }
            }
            black_box(frags)
        })
    });
}

fn bench_build_10k(c: &mut Criterion) {
    let nodes: Vec<Node> = (0..10_000).map(make_node).collect();
    let edges: Vec<Edge> = (0..15_000).map(|i| make_edge(i, 10_000)).collect();
    c.bench_function("build_graph_10k_nodes", |b| {
        b.iter(|| {
            let store = Box::new(MemoryGraphStore::new());
            let builder = GraphBuilder::new(store);
            builder.add_nodes(nodes.clone()).unwrap();
            builder.add_edges(edges.clone()).unwrap();
            black_box(builder)
        })
    });
}

fn bench_dedup_50k(c: &mut Criterion) {
    let nodes: Vec<Node> = (0..1_000).map(make_node).collect();
    let edges: Vec<Edge> = (0..50_000).map(|i| make_edge(i, 1_000)).collect();
    let dedup = Deduplicator::new(Deduplicator::default_threshold());
    c.bench_function("dedup_50k_edges", |b| {
        b.iter(|| black_box(dedup.deduplicate(nodes.clone(), edges.clone()).unwrap()))
    });
}

fn bench_export_json_10k(c: &mut Criterion) {
    let nodes: Vec<Node> = (0..10_000).map(make_node).collect();
    let edges: Vec<Edge> = (0..12_000).map(|i| make_edge(i, 10_000)).collect();
    let exporter = Exporter;
    c.bench_function("export_json_10k_nodes", |b| {
        b.iter(|| black_box(exporter.to_json(&nodes, &edges, None).unwrap()))
    });
}

criterion_group!(
    extraction_benches,
    bench_detect_1k,
    bench_extract_100,
    bench_build_10k,
    bench_dedup_50k,
    bench_export_json_10k,
);
criterion_main!(extraction_benches);
