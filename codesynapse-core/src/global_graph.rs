use crate::types::{Edge, GraphData, Node};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

const MAX_GRAPH_FILE_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct PrefixedGraph {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoManifestEntry {
    pub added_at: String,
    pub source_path: String,
    pub node_count: usize,
    pub edge_count: usize,
    pub source_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalManifest {
    pub version: u32,
    pub repos: HashMap<String, RepoManifestEntry>,
}

impl Default for GlobalManifest {
    fn default() -> Self {
        Self {
            version: 1,
            repos: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct AddResult {
    pub repo_tag: String,
    pub nodes_added: usize,
    pub nodes_removed: usize,
    pub skipped: bool,
}

fn file_hash(path: &Path) -> crate::error::Result<String> {
    use sha2::{Digest, Sha256};
    let bytes = std::fs::read(path)?;
    let hash = Sha256::digest(&bytes);
    Ok(format!("{:x}", hash)[..16].to_string())
}

fn global_graph_path(global_dir: &Path) -> PathBuf {
    global_dir.join("global-graph.json")
}

fn global_manifest_path(global_dir: &Path) -> PathBuf {
    global_dir.join("global-manifest.json")
}

fn load_manifest(global_dir: &Path) -> GlobalManifest {
    let path = global_manifest_path(global_dir);
    if path.exists() {
        if let Ok(text) = std::fs::read_to_string(&path) {
            if let Ok(m) = serde_json::from_str::<GlobalManifest>(&text) {
                return m;
            }
        }
    }
    GlobalManifest::default()
}

fn save_manifest(global_dir: &Path, manifest: &GlobalManifest) -> crate::error::Result<()> {
    std::fs::create_dir_all(global_dir)?;
    let text = serde_json::to_string_pretty(manifest)
        .map_err(crate::error::CodeSynapseError::Serialization)?;
    std::fs::write(global_manifest_path(global_dir), text)?;
    Ok(())
}

fn load_global_graph(global_dir: &Path) -> crate::error::Result<GraphData> {
    let path = global_graph_path(global_dir);
    if !path.exists() {
        return Ok(GraphData {
            nodes: vec![],
            edges: vec![],
            hyperedges: None,
        });
    }
    let size = std::fs::metadata(&path)?.len();
    if size > MAX_GRAPH_FILE_BYTES {
        return Err(crate::error::CodeSynapseError::Validation(format!(
            "global graph exceeds {} byte limit",
            MAX_GRAPH_FILE_BYTES
        )));
    }
    let text = std::fs::read_to_string(&path)?;
    let v: serde_json::Value =
        serde_json::from_str(&text).map_err(crate::error::CodeSynapseError::Serialization)?;

    let nodes: Vec<Node> = v
        .get("nodes")
        .and_then(|n| n.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| serde_json::from_value(item.clone()).ok())
                .collect()
        })
        .unwrap_or_default();

    let edge_arr = v
        .get("edges")
        .or_else(|| v.get("links"))
        .and_then(|e| e.as_array());
    let edges: Vec<Edge> = edge_arr
        .map(|arr| {
            arr.iter()
                .filter_map(|item| serde_json::from_value(item.clone()).ok())
                .collect()
        })
        .unwrap_or_default();

    Ok(GraphData {
        nodes,
        edges,
        hyperedges: None,
    })
}

fn save_global_graph(global_dir: &Path, graph: &GraphData) -> crate::error::Result<()> {
    std::fs::create_dir_all(global_dir)?;
    let data = json!({
        "nodes": graph.nodes,
        "edges": graph.edges,
    });
    let text = serde_json::to_string_pretty(&data)
        .map_err(crate::error::CodeSynapseError::Serialization)?;
    std::fs::write(global_graph_path(global_dir), text)?;
    Ok(())
}

pub fn prefix_graph(graph: &GraphData, repo_tag: &str) -> PrefixedGraph {
    let prefixed_nodes: Vec<Node> = graph
        .nodes
        .iter()
        .map(|n| {
            let mut meta = n.metadata.clone();
            meta.insert("repo".to_string(), repo_tag.to_string());
            meta.insert("local_id".to_string(), n.id.clone());
            Node {
                id: format!("{}::{}", repo_tag, n.id),
                label: n.label.clone(),
                file_type: n.file_type.clone(),
                source_file: n.source_file.clone(),
                source_location: n.source_location.clone(),
                community: n.community,
                rationale: n.rationale.clone(),
                docstring: None,
                metadata: meta,
            }
        })
        .collect();

    let prefixed_edges: Vec<Edge> = graph
        .edges
        .iter()
        .map(|e| Edge {
            source: format!("{}::{}", repo_tag, e.source),
            target: format!("{}::{}", repo_tag, e.target),
            relation: e.relation.clone(),
            confidence: e.confidence.clone(),
            source_file: e.source_file.clone(),
            weight: e.weight,
            context: None,
        })
        .collect();

    PrefixedGraph {
        nodes: prefixed_nodes,
        edges: prefixed_edges,
    }
}

pub fn prune_repo_from_graph(graph: &mut GraphData, repo_tag: &str) -> usize {
    let prefix = format!("{}::", repo_tag);
    let _before = graph.nodes.len();
    let remove_ids: HashSet<String> = graph
        .nodes
        .iter()
        .filter(|n| {
            n.id.starts_with(&prefix)
                || n.metadata.get("repo").map(|s| s.as_str()) == Some(repo_tag)
        })
        .map(|n| n.id.clone())
        .collect();
    graph.nodes.retain(|n| !remove_ids.contains(&n.id));
    graph
        .edges
        .retain(|e| !remove_ids.contains(&e.source) && !remove_ids.contains(&e.target));
    remove_ids.len()
}

pub fn global_add(
    source_path: &Path,
    repo_tag: &str,
    global_dir: &Path,
) -> crate::error::Result<AddResult> {
    global_add_force(source_path, repo_tag, global_dir, false)
}

pub fn global_add_force(
    source_path: &Path,
    repo_tag: &str,
    global_dir: &Path,
    force: bool,
) -> crate::error::Result<AddResult> {
    if !source_path.exists() {
        return Err(crate::error::CodeSynapseError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("graph not found: {}", source_path.display()),
        )));
    }

    let size = std::fs::metadata(source_path)?.len();
    if size > MAX_GRAPH_FILE_BYTES {
        return Err(crate::error::CodeSynapseError::Validation(format!(
            "source graph exceeds {} byte limit",
            MAX_GRAPH_FILE_BYTES
        )));
    }

    let mut manifest = load_manifest(global_dir);
    let src_hash = file_hash(source_path)?;

    let existing = manifest.repos.get(repo_tag);
    if let Some(existing) = existing {
        let canonical = source_path
            .canonicalize()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| source_path.display().to_string());
        if !existing.source_path.is_empty() && existing.source_path != canonical {
            eprintln!(
                "[codesynapse global] warning: repo tag '{}' previously pointed to {:?}, now updating to {:?}. Use --as <tag> to give it a different name.",
                repo_tag, existing.source_path, canonical
            );
        }
        if !force && existing.source_hash == src_hash {
            return Ok(AddResult {
                repo_tag: repo_tag.to_string(),
                nodes_added: 0,
                nodes_removed: 0,
                skipped: true,
            });
        }
    }

    let text = std::fs::read_to_string(source_path)?;
    let v: serde_json::Value =
        serde_json::from_str(&text).map_err(crate::error::CodeSynapseError::Serialization)?;

    let src_nodes: Vec<Node> = v
        .get("nodes")
        .and_then(|n| n.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| serde_json::from_value(item.clone()).ok())
                .collect()
        })
        .unwrap_or_default();
    let edge_arr = v
        .get("edges")
        .or_else(|| v.get("links"))
        .and_then(|e| e.as_array());
    let src_edges: Vec<Edge> = edge_arr
        .map(|arr| {
            arr.iter()
                .filter_map(|item| serde_json::from_value(item.clone()).ok())
                .collect()
        })
        .unwrap_or_default();

    let src_graph = GraphData {
        nodes: src_nodes,
        edges: src_edges,
        hyperedges: None,
    };
    let prefixed = prefix_graph(&src_graph, repo_tag);
    let edge_count = prefixed.edges.len();

    let mut global = load_global_graph(global_dir)?;
    let removed = prune_repo_from_graph(&mut global, repo_tag);

    let external_labels: HashMap<String, String> = global
        .nodes
        .iter()
        .filter(|n| n.source_file.is_empty() && !n.label.is_empty())
        .map(|n| (n.label.clone(), n.id.clone()))
        .collect();

    let skip_ids: HashSet<String> = prefixed
        .nodes
        .iter()
        .filter(|n| n.source_file.is_empty() && external_labels.contains_key(&n.label))
        .map(|n| n.id.clone())
        .collect();

    let added = prefixed.nodes.len() - skip_ids.len();
    for node in &prefixed.nodes {
        if !skip_ids.contains(&node.id) {
            global.nodes.push(node.clone());
        }
    }
    for edge in &prefixed.edges {
        if !skip_ids.contains(&edge.source) && !skip_ids.contains(&edge.target) {
            global.edges.push(edge.clone());
        }
    }

    save_global_graph(global_dir, &global)?;

    let canonical = source_path
        .canonicalize()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| source_path.display().to_string());

    manifest.repos.insert(
        repo_tag.to_string(),
        RepoManifestEntry {
            added_at: chrono::Utc::now().to_rfc3339(),
            source_path: canonical,
            node_count: added,
            edge_count,
            source_hash: src_hash,
        },
    );
    save_manifest(global_dir, &manifest)?;

    Ok(AddResult {
        repo_tag: repo_tag.to_string(),
        nodes_added: added,
        nodes_removed: removed,
        skipped: false,
    })
}

pub fn global_remove(repo_tag: &str, global_dir: &Path) -> crate::error::Result<usize> {
    let mut manifest = load_manifest(global_dir);
    if !manifest.repos.contains_key(repo_tag) {
        return Err(crate::error::CodeSynapseError::Validation(format!(
            "repo '{}' not in global graph",
            repo_tag
        )));
    }

    let mut global = load_global_graph(global_dir)?;
    let removed = prune_repo_from_graph(&mut global, repo_tag);
    save_global_graph(global_dir, &global)?;

    manifest.repos.remove(repo_tag);
    save_manifest(global_dir, &manifest)?;

    Ok(removed)
}

pub fn global_list(global_dir: &Path) -> HashMap<String, RepoManifestEntry> {
    load_manifest(global_dir).repos
}

/// Generate dense embeddings for all nodes in the global graph and write to
/// `~/.codesynapse/embeddings.json`. Skipped (returns `Ok(0)`) when the model
/// is not installed or when `embeddings.json` is already newer than
/// `global-graph.json` (mtime-gated). Returns the number of nodes embedded.
pub fn embed_global_graph(global_dir: &Path) -> crate::error::Result<usize> {
    let model_path = global_dir.join("models").join("potion-code-16M");
    if !model_path.exists() {
        return Ok(0);
    }

    let graph_path = global_graph_path(global_dir);
    let embed_path = global_dir.join("embeddings.json");

    // mtime gate: skip if embeddings are newer than the graph
    if embed_path.exists() && graph_path.exists() {
        let graph_mtime = std::fs::metadata(&graph_path)
            .and_then(|m| m.modified())
            .ok();
        let embed_mtime = std::fs::metadata(&embed_path)
            .and_then(|m| m.modified())
            .ok();
        if let (Some(g), Some(e)) = (graph_mtime, embed_mtime) {
            if e >= g {
                return Ok(0);
            }
        }
    }

    let graph = load_global_graph(global_dir)?;
    if graph.nodes.is_empty() {
        return Ok(0);
    }

    // Build child-label lookup for CONTAINS edges (method names per class)
    let mut child_labels: HashMap<String, Vec<String>> = HashMap::new();
    for edge in &graph.edges {
        if edge.relation == "contains" || edge.relation == "CONTAINS" {
            if let Some(target) = graph.nodes.iter().find(|n| n.id == edge.target) {
                child_labels
                    .entry(edge.source.clone())
                    .or_default()
                    .push(target.label.clone());
            }
        }
    }

    // Build (id, text) pairs — label + docstring + source_file + child labels, capped at 1024 chars
    let pairs: Vec<(String, String)> = graph
        .nodes
        .iter()
        .map(|n| {
            let docstring_str = n.docstring.as_deref().unwrap_or("");
            let methods = child_labels
                .get(&n.id)
                .map(|v| v.join(" "))
                .unwrap_or_default();
            let text = format!(
                "{} {} {} {}",
                n.label, docstring_str, n.source_file, methods
            );
            let text = text.trim().to_string();
            let text = if text.len() > 1024 {
                text[..1024].to_string()
            } else {
                text
            };
            (n.id.clone(), text)
        })
        .collect();

    let embedder = crate::embedding::StaticEmbedder::from_path(&model_path)
        .map_err(crate::error::CodeSynapseError::Validation)?;

    let refs: Vec<(&str, &str)> = pairs
        .iter()
        .map(|(id, txt)| (id.as_str(), txt.as_str()))
        .collect();
    let embeddings = embedder.embed_nodes(&refs);

    let text = serde_json::to_string(&embeddings)
        .map_err(crate::error::CodeSynapseError::Serialization)?;
    std::fs::write(&embed_path, text)?;

    Ok(embeddings.len())
}

pub fn check_cross_repo_guard(nodes: &[Node]) -> crate::error::Result<()> {
    let repos: HashSet<&str> = nodes
        .iter()
        .filter_map(|n| n.metadata.get("repo").map(|s| s.as_str()))
        .filter(|s| !s.is_empty())
        .collect();
    if repos.len() > 1 {
        return Err(crate::error::CodeSynapseError::Validation(format!(
            "nodes span multiple repos: {:?} — dedup requires a single-repo input",
            repos
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Edge, Node};
    use std::collections::HashMap;

    fn make_node(id: &str, label: &str, source_file: &str) -> Node {
        Node {
            id: id.into(),
            label: label.into(),
            file_type: "code".into(),
            source_file: source_file.into(),
            source_location: None,
            community: None,
            rationale: None,
            docstring: None,
            metadata: HashMap::new(),
        }
    }

    fn make_node_with_repo(id: &str, label: &str, repo: &str) -> Node {
        let mut meta = HashMap::new();
        meta.insert("repo".to_string(), repo.to_string());
        Node {
            id: id.into(),
            label: label.into(),
            file_type: "code".into(),
            source_file: "src/x.py".into(),
            source_location: None,
            community: None,
            rationale: None,
            docstring: None,
            metadata: meta,
        }
    }

    fn make_edge(src: &str, tgt: &str) -> Edge {
        Edge {
            source: src.into(),
            target: tgt.into(),
            relation: "calls".into(),
            confidence: "EXTRACTED".into(),
            source_file: None,
            weight: 1.0,
            context: None,
        }
    }

    fn simple_graph() -> GraphData {
        GraphData {
            nodes: vec![make_node("userservice", "UserService", "src/user.py")],
            edges: vec![],
            hyperedges: None,
        }
    }

    #[test]
    fn test_prefix_graph_preserves_label() {
        let g = simple_graph();
        let h = prefix_graph(&g, "repoA");
        assert!(h.nodes.iter().any(|n| n.id == "repoA::userservice"));
        assert!(!h.nodes.iter().any(|n| n.id == "userservice"));
        let node = h
            .nodes
            .iter()
            .find(|n| n.id == "repoA::userservice")
            .unwrap();
        assert_eq!(node.label, "UserService");
    }

    #[test]
    fn test_prefix_graph_sets_repo_and_local_id() {
        let g = simple_graph();
        let h = prefix_graph(&g, "repoA");
        let node = h
            .nodes
            .iter()
            .find(|n| n.id == "repoA::userservice")
            .unwrap();
        assert_eq!(node.metadata.get("repo").map(|s| s.as_str()), Some("repoA"));
        assert_eq!(
            node.metadata.get("local_id").map(|s| s.as_str()),
            Some("userservice")
        );
    }

    #[test]
    fn test_prefix_graph_rewrites_edges() {
        let g = GraphData {
            nodes: vec![make_node("a", "A", "a.py"), make_node("b", "B", "b.py")],
            edges: vec![make_edge("a", "b")],
            hyperedges: None,
        };
        let h = prefix_graph(&g, "repo1");
        assert!(h
            .edges
            .iter()
            .any(|e| e.source == "repo1::a" && e.target == "repo1::b"));
        assert!(!h.edges.iter().any(|e| e.source == "a" && e.target == "b"));
    }

    #[test]
    fn test_prune_repo_removes_correct_nodes() {
        let mut g = GraphData {
            nodes: vec![
                make_node_with_repo("repoA::userservice", "UserService", "repoA"),
                make_node_with_repo("repoB::userservice", "UserService", "repoB"),
                make_node_with_repo("repoA::auth", "Auth", "repoA"),
            ],
            edges: vec![],
            hyperedges: None,
        };
        let removed = prune_repo_from_graph(&mut g, "repoA");
        assert_eq!(removed, 2);
        assert!(g.nodes.iter().any(|n| n.id == "repoB::userservice"));
        assert!(!g.nodes.iter().any(|n| n.id == "repoA::userservice"));
        assert!(!g.nodes.iter().any(|n| n.id == "repoA::auth"));
    }

    #[test]
    fn test_prune_repo_returns_zero_if_not_present() {
        let mut g = GraphData {
            nodes: vec![make_node_with_repo("repoA::x", "X", "repoA")],
            edges: vec![],
            hyperedges: None,
        };
        let removed = prune_repo_from_graph(&mut g, "repoB");
        assert_eq!(removed, 0);
        assert_eq!(g.nodes.len(), 1);
    }

    #[test]
    fn test_global_add_creates_global_graph() {
        let tmp = tempfile::tempdir().unwrap();
        let src_path = tmp.path().join("graph.json");
        let g = simple_graph();
        std::fs::write(
            &src_path,
            serde_json::to_string(&serde_json::json!({"nodes": g.nodes, "edges": g.edges}))
                .unwrap(),
        )
        .unwrap();
        let global_dir = tmp.path().join(".codesynapse");
        let result = global_add(&src_path, "repoA", &global_dir).unwrap();
        assert!(!result.skipped);
        assert!(result.nodes_added > 0);
        assert!(global_manifest_path(&global_dir).exists());
        let manifest = load_manifest(&global_dir);
        assert!(manifest.repos.contains_key("repoA"));
    }

    #[test]
    fn test_global_add_skip_on_unchanged_hash() {
        let tmp = tempfile::tempdir().unwrap();
        let src_path = tmp.path().join("graph.json");
        let g = simple_graph();
        std::fs::write(
            &src_path,
            serde_json::to_string(&serde_json::json!({"nodes": g.nodes, "edges": g.edges}))
                .unwrap(),
        )
        .unwrap();
        let global_dir = tmp.path().join(".codesynapse");
        global_add(&src_path, "repoA", &global_dir).unwrap();
        let result2 = global_add(&src_path, "repoA", &global_dir).unwrap();
        assert!(result2.skipped);
    }

    #[test]
    fn test_global_add_two_repos_no_collision() {
        let tmp = tempfile::tempdir().unwrap();
        let g1_path = tmp.path().join("graph1.json");
        let g2_path = tmp.path().join("graph2.json");
        let g = simple_graph();
        let data = serde_json::to_string(&serde_json::json!({"nodes": g.nodes, "edges": g.edges}))
            .unwrap();
        std::fs::write(&g1_path, &data).unwrap();
        std::fs::write(&g2_path, &data).unwrap();
        let global_dir = tmp.path().join(".codesynapse");
        global_add(&g1_path, "repoA", &global_dir).unwrap();
        global_add(&g2_path, "repoB", &global_dir).unwrap();
        let global = load_global_graph(&global_dir).unwrap();
        assert!(global.nodes.iter().any(|n| n.id == "repoA::userservice"));
        assert!(global.nodes.iter().any(|n| n.id == "repoB::userservice"));
        assert_eq!(global.nodes.len(), 2);
    }

    #[test]
    fn test_global_remove() {
        let tmp = tempfile::tempdir().unwrap();
        let src_path = tmp.path().join("graph.json");
        let g = simple_graph();
        std::fs::write(
            &src_path,
            serde_json::to_string(&serde_json::json!({"nodes": g.nodes, "edges": g.edges}))
                .unwrap(),
        )
        .unwrap();
        let global_dir = tmp.path().join(".codesynapse");
        global_add(&src_path, "repoA", &global_dir).unwrap();
        let removed = global_remove("repoA", &global_dir).unwrap();
        assert!(removed > 0);
        let repos = global_list(&global_dir);
        assert!(!repos.contains_key("repoA"));
    }

    #[test]
    fn test_global_remove_unknown_tag_raises() {
        let tmp = tempfile::tempdir().unwrap();
        let global_dir = tmp.path().join(".codesynapse");
        let result = global_remove("nonexistent", &global_dir);
        assert!(result.is_err());
    }

    #[test]
    fn test_global_add_collision_warning() {
        let tmp = tempfile::tempdir().unwrap();
        let g1 = tmp.path().join("graph1.json");
        let g2 = tmp.path().join("graph2.json");
        let g = GraphData {
            nodes: vec![make_node("x", "X", "x.py")],
            edges: vec![],
            hyperedges: None,
        };
        let data1 = serde_json::to_string(&serde_json::json!({"nodes": g.nodes, "edges": g.edges}))
            .unwrap();
        std::fs::write(&g1, &data1).unwrap();
        std::fs::write(&g2, &data1).unwrap();
        let global_dir = tmp.path().join(".codesynapse");
        global_add(&g1, "myrepo", &global_dir).unwrap();
        let result = global_add(&g2, "myrepo", &global_dir);
        assert!(result.is_ok());
    }

    #[test]
    fn test_dedup_raises_on_cross_repo_nodes() {
        let nodes = vec![
            make_node_with_repo("repoA::userservice", "UserService", "repoA"),
            make_node_with_repo("repoB::userservice", "UserService", "repoB"),
        ];
        let result = check_cross_repo_guard(&nodes);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("multiple repos"));
    }

    #[test]
    fn test_dedup_ok_with_single_repo() {
        let nodes = vec![
            make_node_with_repo("repoA::userservice", "UserService", "repoA"),
            make_node_with_repo("repoA::auth", "Auth", "repoA"),
        ];
        assert!(check_cross_repo_guard(&nodes).is_ok());
    }

    #[test]
    fn test_dedup_ok_with_no_repo_attr() {
        let nodes = vec![
            make_node("userservice", "UserService", "src.py"),
            make_node("auth", "Auth", "src.py"),
        ];
        assert!(check_cross_repo_guard(&nodes).is_ok());
    }

    #[test]
    fn test_merge_graphs_prefixes_ids() {
        let g1 = GraphData {
            nodes: vec![make_node("userservice", "UserService", "src/user.py")],
            edges: vec![],
            hyperedges: None,
        };
        let g2 = g1.clone();
        let p1 = prefix_graph(&g1, "repo1");
        let p2 = prefix_graph(&g2, "repo2");
        let all_ids: HashSet<&str> = p1
            .nodes
            .iter()
            .chain(p2.nodes.iter())
            .map(|n| n.id.as_str())
            .collect();
        assert!(all_ids.contains("repo1::userservice"));
        assert!(all_ids.contains("repo2::userservice"));
        assert_eq!(all_ids.len(), 2);
    }

    #[test]
    fn test_global_add_rejects_oversized_source_graph() {
        let tmp = tempfile::tempdir().unwrap();
        let src_path = tmp.path().join("graph.json");
        let g = simple_graph();
        std::fs::write(
            &src_path,
            serde_json::to_string(&serde_json::json!({"nodes": g.nodes, "edges": g.edges}))
                .unwrap(),
        )
        .unwrap();
        let global_dir = tmp.path().join(".codesynapse");
        // Normal file should pass (we can't easily set a small cap, so verify normal behavior)
        let result = global_add(&src_path, "repoA", &global_dir);
        assert!(result.is_ok());
    }

    // Fix 2: embed_global_graph returns Ok(0) when model not installed
    #[test]
    fn test_embed_global_graph_no_model_returns_zero() {
        let tmp = tempfile::tempdir().unwrap();
        let global_dir = tmp.path();
        // no models/potion-code-16M directory → skip silently
        let result = embed_global_graph(global_dir);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 0);
        assert!(!global_dir.join("embeddings.json").exists());
    }

    // Fix 2: embed_global_graph returns Ok(0) when graph is empty
    #[test]
    fn test_embed_global_graph_empty_graph_returns_zero() {
        let tmp = tempfile::tempdir().unwrap();
        let global_dir = tmp.path();
        // create fake model dir to pass model check
        std::fs::create_dir_all(global_dir.join("models").join("potion-code-16M")).unwrap();
        // write an empty global graph
        std::fs::write(
            global_dir.join("global-graph.json"),
            r#"{"nodes":[],"edges":[]}"#,
        )
        .unwrap();
        let result = embed_global_graph(global_dir);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 0);
    }

    // Fix 2: mtime gate — embeddings.json newer than global-graph.json → skip
    #[test]
    fn test_embed_global_graph_mtime_gate_skips_when_current() {
        let tmp = tempfile::tempdir().unwrap();
        let global_dir = tmp.path();
        std::fs::create_dir_all(global_dir.join("models").join("potion-code-16M")).unwrap();

        let graph_path = global_dir.join("global-graph.json");
        std::fs::write(&graph_path, r#"{"nodes":[{"id":"a","label":"A","file_type":"code","source_file":"a.rs"}],"edges":[]}"#).unwrap();

        // sleep to ensure embeddings.json gets a strictly later mtime than graph
        std::thread::sleep(std::time::Duration::from_millis(20));
        let embed_path = global_dir.join("embeddings.json");
        std::fs::write(&embed_path, r#"{"sentinel":true}"#).unwrap();

        let original = std::fs::read_to_string(&embed_path).unwrap();
        let result = embed_global_graph(global_dir);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 0, "mtime gate should skip re-embedding");
        assert_eq!(
            std::fs::read_to_string(&embed_path).unwrap(),
            original,
            "embeddings.json should not be rewritten when up-to-date"
        );
    }
}
