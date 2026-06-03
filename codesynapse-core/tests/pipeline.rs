use std::collections::HashMap;
use std::path::{Path, PathBuf};

use codesynapse_core::build::GraphBuilder;
use codesynapse_core::cluster::CommunityDetector;
use codesynapse_core::detect::Detector;
use codesynapse_core::error::Result;
use codesynapse_core::export::Exporter;
use codesynapse_core::extract::Extractor;
use codesynapse_core::graph::{GraphStore, MemoryGraphStore, SledGraphStore};
use codesynapse_core::ts_extract::{
    JsonPackageExtractor, McpConfigExtractor, TsBashExtractor, TsCExtractor, TsCSharpExtractor,
    TsCppExtractor, TsGoExtractor, TsJavaExtractor, TsJavaScriptExtractor, TsKotlinExtractor,
    TsPhpExtractor, TsPythonExtractor, TsRubyExtractor, TsRustExtractor, TsSqlExtractor,
    TsSvelteExtractor, TsSwiftExtractor, TsTypeScriptExtractor, TsVueExtractor,
};
use codesynapse_core::types::{Edge, Node};

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../tests/fixtures")
        .canonicalize()
        .unwrap()
}

fn single_file_dir() -> PathBuf {
    fixtures_dir().join("single-file")
}

fn corpus_full_dir() -> PathBuf {
    fixtures_dir().join("corpus-full")
}

fn structural_similarity(nodes_a: &[Node], nodes_b: &[Node]) -> f64 {
    if nodes_a.is_empty() && nodes_b.is_empty() {
        return 1.0;
    }
    if nodes_a.is_empty() || nodes_b.is_empty() {
        return 0.0;
    }
    let set_b: std::collections::HashSet<(&str, &str)> = nodes_b
        .iter()
        .map(|n| (n.label.as_str(), n.source_file.as_str()))
        .collect();
    let matches = nodes_a
        .iter()
        .filter(|n| set_b.contains(&(n.label.as_str(), n.source_file.as_str())))
        .count();
    matches as f64 / nodes_a.len() as f64
}

fn register_extractors(extractor: &mut Extractor) {
    extractor.register("py", Box::new(TsPythonExtractor));
    extractor.register("js", Box::new(TsJavaScriptExtractor));
    extractor.register("jsx", Box::new(TsJavaScriptExtractor));
    extractor.register("mjs", Box::new(TsJavaScriptExtractor));
    extractor.register("cjs", Box::new(TsJavaScriptExtractor));
    extractor.register("ts", Box::new(TsTypeScriptExtractor));
    extractor.register("tsx", Box::new(TsTypeScriptExtractor));
    extractor.register("mts", Box::new(TsTypeScriptExtractor));
    extractor.register("cts", Box::new(TsTypeScriptExtractor));
    extractor.register("go", Box::new(TsGoExtractor));
    extractor.register("rs", Box::new(TsRustExtractor));
    extractor.register("java", Box::new(TsJavaExtractor));
    extractor.register("c", Box::new(TsCExtractor));
    extractor.register("h", Box::new(TsCExtractor));
    extractor.register("cpp", Box::new(TsCppExtractor));
    extractor.register("cxx", Box::new(TsCppExtractor));
    extractor.register("hpp", Box::new(TsCppExtractor));
    extractor.register("cs", Box::new(TsCSharpExtractor));
    extractor.register("kt", Box::new(TsKotlinExtractor));
    extractor.register("kts", Box::new(TsKotlinExtractor));
    extractor.register("swift", Box::new(TsSwiftExtractor));
    extractor.register("php", Box::new(TsPhpExtractor));
    extractor.register("rb", Box::new(TsRubyExtractor));
    extractor.register("sql", Box::new(TsSqlExtractor));
    extractor.register("sh", Box::new(TsBashExtractor));
    extractor.register("bash", Box::new(TsBashExtractor));
    extractor.register("vue", Box::new(TsVueExtractor));
    extractor.register("svelte", Box::new(TsSvelteExtractor));
    extractor.register("json", Box::new(JsonPackageExtractor));
    extractor.register("mcp.json", Box::new(McpConfigExtractor));
}

fn run_pipeline(root: &Path) -> Result<(Vec<Node>, Vec<Edge>)> {
    let detector = Detector::new(root);
    let files = detector.discover(root)?;

    let mut extractor = Extractor::new();
    register_extractors(&mut extractor);

    let mut all_fragments = Vec::new();
    for file in &files {
        if file.file_type.as_str() == "code" {
            let source = std::fs::read(&file.path)?;
            if let Ok(fragment) = extractor.extract_file(&file.path, &source) {
                all_fragments.push((file.relative_path.clone(), fragment.nodes, fragment.edges));
            }
        }
    }

    let store = MemoryGraphStore::new();
    let builder = GraphBuilder::new(Box::new(store));
    builder.build_from_fragments(all_fragments)?;

    let final_nodes = builder.store().get_all_nodes()?;
    let final_edges = builder.store().get_all_edges()?;
    Ok((final_nodes, final_edges))
}

// ---------------------------------------------------------------------------
// Gap test #127: pipeline_corpus_full
// ---------------------------------------------------------------------------

#[test]
fn test_pipeline_corpus_full() {
    let root = corpus_full_dir();

    let (nodes1, edges1) = run_pipeline(&root).unwrap();

    assert!(
        nodes1.len() >= 30,
        "full corpus pipeline should produce at least 30 nodes, got {}",
        nodes1.len()
    );
    assert!(
        edges1.len() >= 5,
        "full corpus pipeline should produce at least 5 edges, got {}",
        edges1.len()
    );

    // Structural self-consistency: two runs on the same corpus must agree ≥ 0.85
    let (nodes2, _) = run_pipeline(&root).unwrap();
    let sim = structural_similarity(&nodes1, &nodes2);
    assert!(
        sim >= 0.85,
        "structural similarity between two pipeline runs should be ≥ 0.85, got {sim:.3}"
    );
}

// ---------------------------------------------------------------------------
// Gap test #126: pipeline_corpus_minimal
// ---------------------------------------------------------------------------

#[test]
fn test_pipeline_corpus_minimal() {
    let root = single_file_dir();
    let (nodes, edges) = run_pipeline(&root).unwrap();

    assert!(
        !nodes.is_empty(),
        "pipeline should produce at least one node"
    );
    assert!(
        !edges.is_empty(),
        "pipeline should produce at least one edge"
    );

    let exporter = Exporter;
    let json = exporter.to_json(&nodes, &edges, None).unwrap();
    assert!(
        json.contains("\"nodes\""),
        "JSON output should contain nodes"
    );
    assert!(
        json.contains("\"edges\""),
        "JSON output should contain edges"
    );
}

// ---------------------------------------------------------------------------
// Gap test #128: pipeline_incremental
// ---------------------------------------------------------------------------

#[test]
fn test_pipeline_incremental() {
    let store = MemoryGraphStore::new();
    let builder = GraphBuilder::new(Box::new(store));

    let node_a = Node {
        id: "a".into(),
        label: "A".into(),
        file_type: "code".into(),
        source_file: "a.py".into(),
        source_location: None,
        community: None,
        rationale: None,
        docstring: None,
        metadata: HashMap::new(),
    };
    let node_b = Node {
        id: "b".into(),
        label: "B".into(),
        file_type: "code".into(),
        source_file: "b.py".into(),
        source_location: None,
        community: None,
        rationale: None,
        docstring: None,
        metadata: HashMap::new(),
    };

    builder.add_nodes(vec![node_a, node_b]).unwrap();
    assert_eq!(builder.store().node_count().unwrap(), 2);

    // Incremental: add a new node without removing existing ones
    let node_c = Node {
        id: "c".into(),
        label: "C".into(),
        file_type: "code".into(),
        source_file: "c.py".into(),
        source_location: None,
        community: None,
        rationale: None,
        docstring: None,
        metadata: HashMap::new(),
    };
    builder.add_nodes(vec![node_c]).unwrap();
    assert_eq!(builder.store().node_count().unwrap(), 3);
}

// ---------------------------------------------------------------------------
// Gap test #129: pipeline_directed
// ---------------------------------------------------------------------------

#[test]
fn test_pipeline_directed() {
    let store = MemoryGraphStore::new();
    let builder = GraphBuilder::new_directed(Box::new(store));
    assert!(builder.is_directed());
}

// ---------------------------------------------------------------------------
// Gap test #131: pipeline_cluster_only
// ---------------------------------------------------------------------------

#[test]
fn test_pipeline_cluster_only() {
    let (nodes, edges) = run_pipeline(&single_file_dir()).unwrap();
    assert!(!nodes.is_empty());

    let detector = CommunityDetector;
    let communities = detector.detect(&nodes, &edges, 1.0).unwrap();
    assert!(
        !communities.is_empty(),
        "cluster should produce communities"
    );
    assert_eq!(
        nodes.len(),
        communities.iter().flat_map(|c| c.nodes.iter()).count(),
        "every node should belong to exactly one community"
    );
}

// ---------------------------------------------------------------------------
// Gap test #132: pipeline_force
// ---------------------------------------------------------------------------

#[test]
fn test_pipeline_force() {
    let store = MemoryGraphStore::new();
    let builder = GraphBuilder::new(Box::new(store));

    let node_a = Node {
        id: "a".into(),
        label: "A".into(),
        file_type: "code".into(),
        source_file: "a.py".into(),
        source_location: None,
        community: None,
        rationale: None,
        docstring: None,
        metadata: HashMap::new(),
    };
    builder.add_nodes(vec![node_a]).unwrap();
    assert_eq!(builder.store().node_count().unwrap(), 1);

    // "Force" rebuild: clear and re-add
    builder.store().clear().unwrap();
    assert_eq!(builder.store().node_count().unwrap(), 0);

    let node_b = Node {
        id: "b".into(),
        label: "B".into(),
        file_type: "code".into(),
        source_file: "b.py".into(),
        source_location: None,
        community: None,
        rationale: None,
        docstring: None,
        metadata: HashMap::new(),
    };
    builder.add_nodes(vec![node_b]).unwrap();
    assert_eq!(builder.store().node_count().unwrap(), 1);

    let nodes = builder.store().get_all_nodes().unwrap();
    assert_eq!(nodes[0].id, "b", "force rebuild should replace old data");
}

// ---------------------------------------------------------------------------
// Gap test #133: pipeline_merge_graphs
// ---------------------------------------------------------------------------

#[test]
fn test_pipeline_merge_graphs() {
    let store_a = MemoryGraphStore::new();
    let builder_a = GraphBuilder::new(Box::new(store_a));
    let node_a1 = Node {
        id: "a1".into(),
        label: "A1".into(),
        file_type: "code".into(),
        source_file: "a.py".into(),
        source_location: None,
        community: None,
        rationale: None,
        docstring: None,
        metadata: HashMap::new(),
    };
    builder_a.add_nodes(vec![node_a1]).unwrap();

    let store_b = MemoryGraphStore::new();
    let builder_b = GraphBuilder::new(Box::new(store_b));
    let node_b1 = Node {
        id: "b1".into(),
        label: "B1".into(),
        file_type: "code".into(),
        source_file: "b.py".into(),
        source_location: None,
        community: None,
        rationale: None,
        docstring: None,
        metadata: HashMap::new(),
    };
    builder_b.add_nodes(vec![node_b1]).unwrap();

    let nodes_a = builder_a.store().get_all_nodes().unwrap();
    let nodes_b = builder_b.store().get_all_nodes().unwrap();

    let mut merged_nodes = nodes_a;
    merged_nodes.extend(nodes_b);
    assert_eq!(
        merged_nodes.len(),
        2,
        "merged graph should have union of nodes"
    );
}

// ---------------------------------------------------------------------------
// Cross-module invariant tests
// ---------------------------------------------------------------------------

#[test]
fn test_invariant_extract_build_export_roundtrip() {
    // Extract → Build → Export: verify a real fixture survives the pipeline
    let root = single_file_dir();
    let (nodes, edges) = run_pipeline(&root).unwrap();
    assert!(!nodes.is_empty());

    let exporter = Exporter;
    let json = exporter.to_json(&nodes, &edges, None).unwrap();
    let data: codesynapse_core::types::GraphData = serde_json::from_str(&json).unwrap();

    assert_eq!(data.nodes.len(), nodes.len());
    assert_eq!(data.edges.len(), edges.len());
}

#[test]
fn test_invariant_build_cluster_export() {
    let root = single_file_dir();
    let (nodes, edges) = run_pipeline(&root).unwrap();

    let detector = CommunityDetector;
    let communities = detector.detect(&nodes, &edges, 1.0).unwrap();
    assert!(!communities.is_empty());

    let exporter = Exporter;
    let json = exporter.to_json(&nodes, &edges, None).unwrap();
    assert!(json.contains("\"community\""));
}

#[test]
fn test_invariant_extract_build_store_consistency() {
    let root = single_file_dir();
    let (nodes, edges) = run_pipeline(&root).unwrap();

    // Every node referenced in an edge must exist in the node set
    let node_ids: std::collections::HashSet<&str> = nodes.iter().map(|n| n.id.as_str()).collect();
    for edge in &edges {
        assert!(
            node_ids.contains(edge.source.as_str()),
            "edge source {} missing from nodes",
            edge.source
        );
        assert!(
            node_ids.contains(edge.target.as_str()),
            "edge target {} missing from nodes",
            edge.target
        );
    }
}

#[test]
fn test_invariant_build_search_finds_known() {
    let root = single_file_dir();
    let (nodes, _edges) = run_pipeline(&root).unwrap();
    assert!(!nodes.is_empty());

    // All nodes should have non-empty id and label
    for node in &nodes {
        assert!(!node.id.is_empty(), "node id should not be empty");
        assert!(!node.label.is_empty(), "node label should not be empty");
    }
}

#[test]
fn test_invariant_build_dedup_cluster_covers_all() {
    use codesynapse_core::dedup::Deduplicator;

    let store = MemoryGraphStore::new();
    let builder = GraphBuilder::new(Box::new(store));

    let node = |id: &str, label: &str, file: &str| Node {
        id: id.to_string(),
        label: label.to_string(),
        file_type: "code".to_string(),
        source_file: file.to_string(),
        source_location: None,
        community: None,
        rationale: None,
        docstring: None,
        metadata: HashMap::new(),
    };

    builder
        .add_nodes(vec![
            node("a", "Service", "a.py"),
            node("b", "Service", "a.py"),
        ])
        .unwrap();
    let all_nodes = builder.store().get_all_nodes().unwrap();

    let dedup = Deduplicator::default();
    let (deduped, _) = dedup.deduplicate(all_nodes, vec![]).unwrap();
    assert_eq!(deduped.len(), 1, "duplicate labels should be deduped");

    let detector = CommunityDetector;
    let communities = detector.detect(&deduped, &[], 1.0).unwrap();
    let covered: usize = communities.iter().map(|c| c.nodes.len()).sum();
    assert_eq!(covered, deduped.len(), "every deduped node in a community");
}

#[test]
fn test_invariant_export_json_reimport_roundtrip() {
    let store = MemoryGraphStore::new();
    let builder = GraphBuilder::new(Box::new(store));
    let node_a = Node {
        id: "a".into(),
        label: "Alpha".into(),
        file_type: "code".into(),
        source_file: "a.py".into(),
        source_location: None,
        community: Some(1),
        rationale: None,
        docstring: None,
        metadata: HashMap::from([("lang".into(), "python".into())]),
    };
    let node_b = Node {
        id: "b".into(),
        label: "Beta".into(),
        file_type: "code".into(),
        source_file: "b.py".into(),
        source_location: None,
        community: Some(1),
        rationale: None,
        docstring: None,
        metadata: HashMap::new(),
    };
    builder.add_nodes(vec![node_a, node_b]).unwrap();

    let nodes = builder.store().get_all_nodes().unwrap();
    let edges = builder.store().get_all_edges().unwrap();

    let exporter = Exporter;
    let json = exporter.to_json(&nodes, &edges, None).unwrap();
    let data: codesynapse_core::types::GraphData = serde_json::from_str(&json).unwrap();

    assert_eq!(data.nodes.len(), 2);
    let alpha = data.nodes.iter().find(|n| n.id == "a").unwrap();
    assert_eq!(alpha.community, Some(1));
    assert_eq!(alpha.metadata.get("lang").unwrap(), "python");
}

#[test]
fn test_full_pipeline_with_sled() {
    let dir = std::env::temp_dir().join("codesynapse-test-sled-pipeline");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let db_path = dir.join("sled.db");

    let store = SledGraphStore::open(&db_path).unwrap();

    let mut extractor = Extractor::new();
    register_extractors(&mut extractor);

    let fixtures = single_file_dir();
    let py_file = fixtures.join("class_def.py");
    let source = std::fs::read(&py_file).unwrap();

    let fragment = extractor.extract_file(&py_file, &source).unwrap();
    assert!(
        !fragment.nodes.is_empty(),
        "expected at least one node from class_def.py"
    );

    let builder = GraphBuilder::new(Box::new(store));
    builder
        .build_from_fragments(vec![(
            "class_def.py".to_string(),
            fragment.nodes,
            fragment.edges,
        )])
        .unwrap();

    let nodes = builder.store().get_all_nodes().unwrap();
    let edges = builder.store().get_all_edges().unwrap();
    assert!(
        nodes.iter().any(|n| n.source_file.contains("class_def.py")),
        "expected extracted nodes from class_def.py"
    );

    let exporter = Exporter;
    let json = exporter.to_json(&nodes, &edges, None).unwrap();
    assert!(
        json.contains("Foo") || json.contains("class_def.py"),
        "expected extracted content in JSON output"
    );

    drop(builder);
    drop(extractor);

    let store2 = SledGraphStore::open(&db_path).unwrap();
    assert_eq!(
        store2.node_count().unwrap(),
        nodes.len(),
        "sled reopen should have same node count"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// Gap test #130: pipeline_no_viz
// ---------------------------------------------------------------------------

#[test]
fn test_pipeline_no_viz() {
    let dir = std::env::temp_dir().join("codesynapse_no_viz_test");
    let _ = std::fs::create_dir_all(&dir);

    let (nodes, edges) = run_pipeline(&single_file_dir()).unwrap();

    // Export JSON only — no HTML or SVG
    let exporter = Exporter;
    exporter
        .to_json_file(&nodes, &edges, None, &dir.join("graph.json"))
        .unwrap();

    // Verify no .html or .svg files were produced
    let produced: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
            ext == "html" || ext == "svg"
        })
        .collect();

    assert!(
        produced.is_empty(),
        "no-viz pipeline should produce no html/svg files, found: {:?}",
        produced
    );

    let _ = std::fs::remove_dir_all(&dir);
}
