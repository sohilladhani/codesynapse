//! Stress & performance tests.
//!
//! These tests verify the system handles moderate-scale graphs without
//! excessive memory or time. They are NOT exhaustive benchmarks but
//! serve as regression guards against performance regressions.

use std::collections::HashMap;
use std::time::Instant;

use codesynapse_core::build::GraphBuilder;
use codesynapse_core::cluster::CommunityDetector;
use codesynapse_core::dedup::Deduplicator;
use codesynapse_core::graph::MemoryGraphStore;
use codesynapse_core::types::{Edge, Node};

fn make_node(id: &str, label: &str) -> Node {
    Node {
        id: id.to_string(),
        label: label.to_string(),
        file_type: "code".to_string(),
        source_file: "stress.py".to_string(),
        source_location: None,
        community: None,
        rationale: None,
        docstring: None,
        metadata: HashMap::new(),
    }
}

fn make_edge(src: &str, tgt: &str, weight: f64) -> Edge {
    Edge {
        source: src.to_string(),
        target: tgt.to_string(),
        relation: "connects".to_string(),
        confidence: "EXTRACTED".to_string(),
        source_file: Some("stress.py".to_string()),
        weight,
        context: None,
    }
}

#[test]
fn test_stress_1k_nodes() {
    let store = MemoryGraphStore::new();
    let builder = GraphBuilder::new(Box::new(store));

    let nodes: Vec<Node> = (0..1_000)
        .map(|i| make_node(&format!("n{i}"), &format!("Label{i}")))
        .collect();
    builder.add_nodes(nodes).unwrap();

    assert_eq!(builder.store().node_count().unwrap(), 1_000);
}

#[test]
fn test_stress_5k_edges() {
    let store = MemoryGraphStore::new();
    let builder = GraphBuilder::new(Box::new(store));

    let nodes: Vec<Node> = (0..1_000)
        .map(|i| make_node(&format!("n{i}"), &format!("Label{i}")))
        .collect();
    builder.add_nodes(nodes).unwrap();

    // Chain: n0→n1→n2→...→n999 (999 edges)
    // Plus random cross edges
    let edges: Vec<Edge> = (0..999)
        .map(|i| make_edge(&format!("n{i}"), &format!("n{}", i + 1), 1.0))
        .chain((0..4_001).map(|i| {
            let src = i % 1_000;
            let tgt = (i * 7) % 1_000;
            make_edge(&format!("n{src}"), &format!("n{tgt}"), 0.5)
        }))
        .collect();
    builder.add_edges(edges).unwrap();

    assert_eq!(builder.store().edge_count().unwrap(), 5_000);
}

#[test]
fn test_stress_build_time() {
    let store = MemoryGraphStore::new();
    let builder = GraphBuilder::new(Box::new(store));

    let nodes: Vec<Node> = (0..2_000)
        .map(|i| make_node(&format!("n{i}"), &format!("Label{i}")))
        .collect();

    let start = Instant::now();
    builder.add_nodes(nodes).unwrap();
    let elapsed = start.elapsed();

    assert!(
        elapsed.as_secs_f64() < 5.0,
        "building 2k nodes should be fast"
    );
}

#[test]
fn test_stress_dedup_large_set() {
    // All same label from same source file → merges to 1
    let nodes: Vec<Node> = (0..500)
        .map(|i| make_node(&format!("dup_{i}"), "CommonLabel"))
        .collect();

    let dedup = Deduplicator::default();
    let start = Instant::now();
    let (result, _) = dedup.deduplicate(nodes, vec![]).unwrap();
    let elapsed = start.elapsed();

    assert_eq!(result.len(), 1, "all 500 same-label nodes merge to 1");
    assert!(
        elapsed.as_secs_f64() < 5.0,
        "dedup of 500 nodes should be fast"
    );
}

#[test]
fn test_stress_cluster_1k_nodes() {
    let nodes: Vec<Node> = (0..1_000)
        .map(|i| make_node(&format!("n{i}"), &format!("N{i}")))
        .collect();
    let edges: Vec<Edge> = (0..999)
        .map(|i| make_edge(&format!("n{i}"), &format!("n{}", i + 1), 1.0))
        .collect();

    let detector = CommunityDetector;
    let start = Instant::now();
    let communities = detector.detect(&nodes, &edges, 1.0).unwrap();
    let elapsed = start.elapsed();

    assert!(!communities.is_empty());
    assert!(
        elapsed.as_secs_f64() < 10.0,
        "clustering 1k nodes should finish in reasonable time"
    );
}

#[test]
fn test_stress_chain_shortest_path() {
    let store = MemoryGraphStore::new();
    let builder = GraphBuilder::new(Box::new(store));

    let nodes: Vec<Node> = (0..500)
        .map(|i| make_node(&format!("n{i}"), &format!("N{i}")))
        .collect();
    builder.add_nodes(nodes).unwrap();

    let edges: Vec<Edge> = (0..499)
        .map(|i| make_edge(&format!("n{i}"), &format!("n{}", i + 1), 1.0))
        .collect();
    builder.add_edges(edges).unwrap();

    let start = Instant::now();
    let path = builder
        .store()
        .shortest_path("n0", "n499")
        .unwrap()
        .unwrap();
    let elapsed = start.elapsed();

    assert_eq!(path.len(), 500);
    assert!(
        elapsed.as_secs_f64() < 5.0,
        "shortest path in 500-node chain should be fast"
    );
}

#[test]
fn test_stress_export_1k_nodes() {
    use codesynapse_core::export::Exporter;

    let nodes: Vec<Node> = (0..1_000)
        .map(|i| make_node(&format!("n{i}"), &format!("Label{i}")))
        .collect();
    let edges: Vec<Edge> = (0..999)
        .map(|i| make_edge(&format!("n{i}"), &format!("n{}", i + 1), 1.0))
        .collect();

    let exporter = Exporter;
    let start = Instant::now();
    let json = exporter.to_json(&nodes, &edges, None).unwrap();
    let elapsed = start.elapsed();

    assert!(json.len() > 1_000);
    assert!(
        elapsed.as_secs_f64() < 5.0,
        "JSON export of 1k nodes should be fast"
    );
}

#[test]
fn test_stress_svg_1k_nodes() {
    use codesynapse_core::export::Exporter;

    let nodes: Vec<Node> = (0..500)
        .map(|i| make_node(&format!("n{i}"), &format!("N{i}")))
        .collect();
    let edges: Vec<Edge> = (0..499)
        .map(|i| make_edge(&format!("n{i}"), &format!("n{}", i + 1), 1.0))
        .collect();

    let exporter = Exporter;
    let start = Instant::now();
    let svg = exporter.to_svg(&nodes, &edges).unwrap();
    let elapsed = start.elapsed();

    assert!(svg.contains("<svg"));
    assert!(svg.contains("</svg>"));
    assert!(
        elapsed.as_secs_f64() < 10.0,
        "SVG export of 500 nodes should be fast"
    );
}

#[test]
fn test_stress_graph_clear_rebuild() {
    let store = MemoryGraphStore::new();
    let builder = GraphBuilder::new(Box::new(store));

    let nodes: Vec<Node> = (0..500)
        .map(|i| make_node(&format!("n{i}"), &format!("N{i}")))
        .collect();
    builder.add_nodes(nodes).unwrap();
    assert_eq!(builder.store().node_count().unwrap(), 500);

    builder.store().clear().unwrap();
    assert_eq!(builder.store().node_count().unwrap(), 0);

    // Rebuild with different nodes
    let nodes2: Vec<Node> = (0..300)
        .map(|i| make_node(&format!("m{i}"), &format!("M{i}")))
        .collect();
    builder.add_nodes(nodes2).unwrap();
    assert_eq!(builder.store().node_count().unwrap(), 300);
}
