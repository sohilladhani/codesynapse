use crate::error::Result;
use crate::graph::GraphStore;
use crate::types::{Edge, Node};
use std::collections::{HashMap, HashSet};
use std::path::Path;

pub fn normalize_id(id: &str) -> String {
    id.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("_")
}

pub struct GraphBuilder {
    store: Box<dyn GraphStore>,
    directed: bool,
}

impl GraphBuilder {
    pub fn new(store: Box<dyn GraphStore>) -> Self {
        GraphBuilder {
            store,
            directed: false,
        }
    }

    pub fn new_directed(store: Box<dyn GraphStore>) -> Self {
        GraphBuilder {
            store,
            directed: true,
        }
    }

    pub fn store(&self) -> &dyn GraphStore {
        self.store.as_ref()
    }

    pub fn is_directed(&self) -> bool {
        self.directed
    }

    pub fn set_directed(&mut self, directed: bool) {
        self.directed = directed;
    }

    pub fn add_nodes(&self, nodes: Vec<Node>) -> Result<()> {
        for node in nodes {
            self.store.add_node(node)?;
        }
        Ok(())
    }

    pub fn add_edges(&self, edges: Vec<Edge>) -> Result<()> {
        for edge in edges {
            self.store.add_edge(edge)?;
        }
        Ok(())
    }

    pub fn build_from_fragments(
        &self,
        fragments: Vec<(String, Vec<Node>, Vec<Edge>)>,
    ) -> Result<()> {
        for (_source, nodes, edges) in fragments {
            self.add_nodes(nodes)?;
            self.add_edges(edges)?;
        }
        Ok(())
    }

    pub fn merge(&self, new_nodes: Vec<Node>, new_edges: Vec<Edge>) -> Result<()> {
        self.add_nodes(new_nodes)?;
        self.add_edges(new_edges)?;
        Ok(())
    }

    pub fn merge_with_prune(
        &self,
        new_nodes: Vec<Node>,
        new_edges: Vec<Edge>,
        prune_sources: &[String],
    ) -> Result<(usize, usize)> {
        let all_nodes = self.store.get_all_nodes()?;
        let all_edges = self.store.get_all_edges()?;

        let prune_set: HashSet<&str> = prune_sources.iter().map(|s| s.as_str()).collect();
        let mut removed_nodes = 0usize;
        let mut removed_edges = 0usize;

        for node in &all_nodes {
            if prune_set.contains(node.source_file.as_str()) {
                self.store.remove_node(&node.id)?;
                removed_nodes += 1;
            }
        }

        for edge in &all_edges {
            if prune_set.contains(edge.source_file.as_deref().unwrap_or("")) {
                self.store
                    .remove_edge(&edge.source, &edge.target, &edge.relation)?;
                removed_edges += 1;
            }
        }

        self.add_nodes(new_nodes)?;
        self.add_edges(new_edges)?;

        Ok((removed_nodes, removed_edges))
    }

    pub fn label_dedup(&self) -> Result<usize> {
        let nodes = self.store.get_all_nodes()?;
        let edges = self.store.get_all_edges()?;

        let mut label_to_ids: HashMap<&str, Vec<&str>> = HashMap::new();
        for node in &nodes {
            label_to_ids
                .entry(node.label.as_str())
                .or_default()
                .push(node.id.as_str());
        }

        let mut removed = 0usize;
        for ids in label_to_ids.values() {
            if ids.len() < 2 {
                continue;
            }
            let keep_id = ids[0];
            for dup_id in &ids[1..] {
                for edge in &edges {
                    if edge.source == **dup_id {
                        let new_edge = Edge {
                            source: keep_id.to_string(),
                            ..edge.clone()
                        };
                        self.store.add_edge(new_edge)?;
                    }
                    if edge.target == **dup_id {
                        let new_edge = Edge {
                            target: keep_id.to_string(),
                            ..edge.clone()
                        };
                        self.store.add_edge(new_edge)?;
                    }
                }
                self.store.remove_node(dup_id)?;
                removed += 1;
            }
        }

        // Remove duplicate edges that may have been created by rewiring
        if removed > 0 {
            self.remove_duplicate_edges()?;
        }

        Ok(removed)
    }

    fn remove_duplicate_edges(&self) -> Result<()> {
        let edges = self.store.get_all_edges()?;
        let mut seen: HashSet<(String, String, String)> = HashSet::new();
        for edge in &edges {
            let key = (
                edge.source.clone(),
                edge.target.clone(),
                edge.relation.clone(),
            );
            if !seen.insert(key) {
                self.store
                    .remove_edge(&edge.source, &edge.target, &edge.relation)?;
            }
        }
        Ok(())
    }

    pub fn lang_filter_inferred_calls(&self) -> Result<usize> {
        let all_nodes = self.store.get_all_nodes()?;
        let edges = self.store.get_all_edges()?;
        let node_map: HashMap<&str, &Node> = all_nodes.iter().map(|n| (n.id.as_str(), n)).collect();

        let mut removed = 0usize;
        for edge in &edges {
            if edge.confidence == "INFERRED" {
                if let (Some(src_node), Some(tgt_node)) = (
                    node_map.get(edge.source.as_str()),
                    node_map.get(edge.target.as_str()),
                ) {
                    if src_node.file_type != tgt_node.file_type
                        || Self::lang_from_source(&src_node.source_file)
                            != Self::lang_from_source(&tgt_node.source_file)
                    {
                        self.store
                            .remove_edge(&edge.source, &edge.target, &edge.relation)?;
                        removed += 1;
                    }
                }
            }
        }
        Ok(removed)
    }

    fn lang_from_source(source_file: &str) -> &'static str {
        let ext = Path::new(source_file)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        match ext {
            "py" => "python",
            "js" | "jsx" => "javascript",
            "ts" | "tsx" => "typescript",
            "rs" => "rust",
            "go" => "go",
            "java" => "java",
            "c" | "h" => "c",
            "cpp" | "hpp" | "cc" | "cxx" => "cpp",
            "cs" => "csharp",
            "rb" => "ruby",
            "php" => "php",
            "swift" => "swift",
            "kt" | "kts" => "kotlin",
            _ => "unknown",
        }
    }

    pub fn normalize_source_file(&self, root: &Path) -> Result<()> {
        let nodes = self.store.get_all_nodes()?;
        for node in nodes {
            if let Ok(relative) = Path::new(&node.source_file).strip_prefix(root) {
                let relative_str = relative.to_string_lossy().to_string();
                let mut updated = node.clone();
                updated.source_file = relative_str;
                self.store.add_node(updated)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::GraphStore;
    use crate::NodeId;
    use std::collections::HashMap;
    use std::sync::Mutex;

    struct MockStore {
        nodes: Mutex<HashMap<NodeId, Node>>,
        edges: Mutex<Vec<Edge>>,
    }

    impl MockStore {
        fn new() -> Self {
            MockStore {
                nodes: Mutex::new(HashMap::new()),
                edges: Mutex::new(Vec::new()),
            }
        }
    }

    impl GraphStore for MockStore {
        fn dijkstra_shortest_path(&self, _src: &str, _tgt: &str) -> Result<Option<Vec<Node>>> {
            Ok(None)
        }

        fn add_node(&self, node: Node) -> Result<()> {
            self.nodes.lock().unwrap().insert(node.id.clone(), node);
            Ok(())
        }

        fn add_edge(&self, edge: Edge) -> Result<()> {
            self.edges.lock().unwrap().push(edge);
            Ok(())
        }

        fn get_node(&self, id: &str) -> Result<Option<Node>> {
            Ok(self.nodes.lock().unwrap().get(id).cloned())
        }

        fn get_all_nodes(&self) -> Result<Vec<Node>> {
            Ok(self.nodes.lock().unwrap().values().cloned().collect())
        }

        fn get_all_edges(&self) -> Result<Vec<Edge>> {
            Ok(self.edges.lock().unwrap().clone())
        }

        fn neighbors(&self, id: &str, _filter: Option<&str>) -> Result<Vec<(Node, Edge)>> {
            let edges = self.edges.lock().unwrap();
            let nodes = self.nodes.lock().unwrap();
            let mut result = Vec::new();
            for edge in edges.iter() {
                if edge.source == id {
                    if let Some(node) = nodes.get(&edge.target) {
                        result.push((node.clone(), edge.clone()));
                    }
                } else if edge.target == id {
                    if let Some(node) = nodes.get(&edge.source) {
                        result.push((node.clone(), edge.clone()));
                    }
                }
            }
            Ok(result)
        }

        fn search(&self, query: &str, _top_k: usize) -> Result<Vec<(f64, Node)>> {
            let nodes = self.nodes.lock().unwrap();
            let q = query.to_lowercase();
            let mut results = Vec::new();
            for node in nodes.values() {
                if node.label.to_lowercase().contains(&q) || node.id.to_lowercase().contains(&q) {
                    results.push((1.0, node.clone()));
                }
            }
            Ok(results)
        }

        fn shortest_path(&self, _src: &str, _tgt: &str) -> Result<Option<Vec<Node>>> {
            Ok(None)
        }

        fn subgraph(&self, node_ids: &[&str]) -> Result<(Vec<Node>, Vec<Edge>)> {
            let nodes = self.nodes.lock().unwrap();
            let edges = self.edges.lock().unwrap();
            let id_set: HashSet<&str> = node_ids.iter().copied().collect();

            let sg_nodes: Vec<Node> = nodes
                .values()
                .filter(|n| id_set.contains(n.id.as_str()))
                .cloned()
                .collect();
            let sg_edges: Vec<Edge> = edges
                .iter()
                .filter(|e| {
                    id_set.contains(e.source.as_str()) && id_set.contains(e.target.as_str())
                })
                .cloned()
                .collect();

            Ok((sg_nodes, sg_edges))
        }

        fn node_count(&self) -> Result<usize> {
            Ok(self.nodes.lock().unwrap().len())
        }

        fn edge_count(&self) -> Result<usize> {
            Ok(self.edges.lock().unwrap().len())
        }

        fn remove_node(&self, id: &str) -> Result<()> {
            self.nodes.lock().unwrap().remove(id);
            Ok(())
        }

        fn remove_edge(&self, source: &str, target: &str, relation: &str) -> Result<()> {
            let mut edges = self.edges.lock().unwrap();
            edges.retain(|e| !(e.source == source && e.target == target && e.relation == relation));
            Ok(())
        }

        fn clear(&self) -> Result<()> {
            self.nodes.lock().unwrap().clear();
            self.edges.lock().unwrap().clear();
            Ok(())
        }
    }

    #[test]
    fn test_build_empty() {
        let store = MockStore::new();
        let builder = GraphBuilder::new(Box::new(store));
        let nodes = vec![];
        let edges = vec![];
        builder
            .build_from_fragments(vec![("test".to_string(), nodes, edges)])
            .unwrap();
        assert_eq!(builder.store().node_count().unwrap(), 0);
        assert_eq!(builder.store().edge_count().unwrap(), 0);
    }

    #[test]
    fn test_build_single_node() {
        let store = MockStore::new();
        let builder = GraphBuilder::new(Box::new(store));
        let node = Node {
            id: "a".to_string(),
            label: "A".to_string(),
            file_type: "code".to_string(),
            source_file: "test.py".to_string(),
            source_location: None,
            community: None,
            rationale: None,
            docstring: None,
            metadata: HashMap::new(),
        };
        builder.add_nodes(vec![node]).unwrap();
        assert_eq!(builder.store().node_count().unwrap(), 1);
        assert_eq!(builder.store().edge_count().unwrap(), 0);
    }

    #[test]
    fn test_build_node_edge() {
        let store = MockStore::new();
        let builder = GraphBuilder::new(Box::new(store));
        let node = Node {
            id: "a".to_string(),
            label: "A".to_string(),
            file_type: "code".to_string(),
            source_file: "test.py".to_string(),
            source_location: None,
            community: None,
            rationale: None,
            docstring: None,
            metadata: HashMap::new(),
        };
        let edge = Edge {
            source: "a".to_string(),
            target: "b".to_string(),
            relation: "imports".to_string(),
            confidence: "EXTRACTED".to_string(),
            source_file: Some("test.py".to_string()),
            weight: 1.0,
            context: None,
        };
        builder.add_nodes(vec![node]).unwrap();
        builder.add_edges(vec![edge]).unwrap();
        assert_eq!(builder.store().node_count().unwrap(), 1);
        assert_eq!(builder.store().edge_count().unwrap(), 1);
    }

    #[test]
    fn test_build_duplicate_node() {
        let store = MockStore::new();
        let builder = GraphBuilder::new(Box::new(store));
        let node1 = Node {
            id: "a".to_string(),
            label: "A".to_string(),
            file_type: "code".to_string(),
            source_file: "test.py".to_string(),
            source_location: None,
            community: None,
            rationale: None,
            docstring: None,
            metadata: HashMap::new(),
        };
        let node2 = Node {
            id: "a".to_string(),
            label: "B".to_string(),
            file_type: "code".to_string(),
            source_file: "test.py".to_string(),
            source_location: None,
            community: None,
            rationale: None,
            docstring: None,
            metadata: HashMap::new(),
        };
        builder.add_nodes(vec![node1]).unwrap();
        builder.add_nodes(vec![node2]).unwrap();
        // Last write wins
        let node = builder.store().get_node("a").unwrap().unwrap();
        assert_eq!(node.label, "B");
    }

    #[test]
    fn test_build_duplicate_edge() {
        let store = MockStore::new();
        let builder = GraphBuilder::new(Box::new(store));
        let edge1 = Edge {
            source: "a".to_string(),
            target: "b".to_string(),
            relation: "imports".to_string(),
            confidence: "EXTRACTED".to_string(),
            source_file: Some("test.py".to_string()),
            weight: 1.0,
            context: None,
        };
        let edge2 = Edge {
            source: "a".to_string(),
            target: "b".to_string(),
            relation: "calls".to_string(),
            confidence: "EXTRACTED".to_string(),
            source_file: Some("test.py".to_string()),
            weight: 1.0,
            context: None,
        };
        builder.add_edges(vec![edge1, edge2]).unwrap();
        assert_eq!(builder.store().edge_count().unwrap(), 2);
    }

    #[test]
    fn test_build_directed() {
        let store = MockStore::new();
        let builder = GraphBuilder::new_directed(Box::new(store));
        assert!(builder.is_directed());

        let edge = Edge {
            source: "a".to_string(),
            target: "b".to_string(),
            relation: "imports".to_string(),
            confidence: "EXTRACTED".to_string(),
            source_file: Some("test.py".to_string()),
            weight: 1.0,
            context: None,
        };
        builder.add_edges(vec![edge.clone()]).unwrap();
        let stored_edges = builder.store().get_all_edges().unwrap();
        assert_eq!(stored_edges.len(), 1);
        assert_eq!(stored_edges[0].source, "a");
        assert_eq!(stored_edges[0].target, "b");
    }

    #[test]
    fn test_build_id_mismatch() {
        let id1 = normalize_id("session_validatetoken");
        let id2 = normalize_id("Session_ValidateToken");
        assert_eq!(id1, id2, "normalized IDs should match");

        let id3 = normalize_id("MY_Class__Name");
        let id4 = normalize_id("my-class-name");
        assert_eq!(id3, id4);

        let store = MockStore::new();
        let builder = GraphBuilder::new(Box::new(store));
        let node1 = Node {
            id: normalize_id("session_validatetoken"),
            label: "validateToken".to_string(),
            file_type: "code".to_string(),
            source_file: "test.py".to_string(),
            source_location: None,
            community: None,
            rationale: None,
            docstring: None,
            metadata: HashMap::new(),
        };
        let node2 = Node {
            id: normalize_id("Session-ValidateToken"),
            label: "validateToken".to_string(),
            file_type: "code".to_string(),
            source_file: "test.py".to_string(),
            source_location: None,
            community: None,
            rationale: None,
            docstring: None,
            metadata: HashMap::new(),
        };
        builder.add_nodes(vec![node1, node2]).unwrap();
        assert_eq!(builder.store().node_count().unwrap(), 1);
    }

    #[test]
    fn test_build_merge_basic() {
        let store = MockStore::new();
        let builder = GraphBuilder::new(Box::new(store));
        let node_a = Node {
            id: "a".to_string(),
            label: "A".to_string(),
            file_type: "code".to_string(),
            source_file: "test.py".to_string(),
            source_location: None,
            community: None,
            rationale: None,
            docstring: None,
            metadata: HashMap::new(),
        };
        builder.merge(vec![node_a], vec![]).unwrap();
        assert_eq!(builder.store().node_count().unwrap(), 1);

        let node_b = Node {
            id: "b".to_string(),
            label: "B".to_string(),
            file_type: "code".to_string(),
            source_file: "test.py".to_string(),
            source_location: None,
            community: None,
            rationale: None,
            docstring: None,
            metadata: HashMap::new(),
        };
        builder.merge(vec![node_b], vec![]).unwrap();
        assert_eq!(builder.store().node_count().unwrap(), 2);
    }

    #[test]
    fn test_build_merge_incremental() {
        let store = MockStore::new();
        let builder = GraphBuilder::new(Box::new(store));
        let node_a = Node {
            id: "a".to_string(),
            label: "A".to_string(),
            file_type: "code".to_string(),
            source_file: "test.py".to_string(),
            source_location: None,
            community: None,
            rationale: None,
            docstring: None,
            metadata: HashMap::new(),
        };
        builder.merge(vec![node_a], vec![]).unwrap();

        let node_b = Node {
            id: "b".to_string(),
            label: "B".to_string(),
            file_type: "code".to_string(),
            source_file: "test.py".to_string(),
            source_location: None,
            community: None,
            rationale: None,
            docstring: None,
            metadata: HashMap::new(),
        };
        let edge_ab = Edge {
            source: "a".to_string(),
            target: "b".to_string(),
            relation: "calls".to_string(),
            confidence: "EXTRACTED".to_string(),
            source_file: Some("test.py".to_string()),
            weight: 1.0,
            context: None,
        };
        builder.merge(vec![node_b], vec![edge_ab]).unwrap();
        assert_eq!(builder.store().node_count().unwrap(), 2);
        assert_eq!(builder.store().edge_count().unwrap(), 1);
    }

    #[test]
    fn test_build_merge_prune() {
        let store = MockStore::new();
        let builder = GraphBuilder::new(Box::new(store));
        let node_a = Node {
            id: "a".to_string(),
            label: "A".to_string(),
            file_type: "code".to_string(),
            source_file: "keep.py".to_string(),
            source_location: None,
            community: None,
            rationale: None,
            docstring: None,
            metadata: HashMap::new(),
        };
        let node_b = Node {
            id: "b".to_string(),
            label: "B".to_string(),
            file_type: "code".to_string(),
            source_file: "delete.py".to_string(),
            source_location: None,
            community: None,
            rationale: None,
            docstring: None,
            metadata: HashMap::new(),
        };
        builder.add_nodes(vec![node_a, node_b]).unwrap();
        assert_eq!(builder.store().node_count().unwrap(), 2);

        let (removed_nodes, removed_edges) = builder
            .merge_with_prune(vec![], vec![], &["delete.py".to_string()])
            .unwrap();
        assert_eq!(removed_nodes, 1);
        assert_eq!(removed_edges, 0);
        assert_eq!(builder.store().node_count().unwrap(), 1);
        assert!(builder.store().get_node("a").unwrap().is_some());
        assert!(builder.store().get_node("b").unwrap().is_none());
    }

    #[test]
    fn test_build_label_dedup() {
        let store = MockStore::new();
        let builder = GraphBuilder::new(Box::new(store));
        let node_a = Node {
            id: "dup_a".to_string(),
            label: "CommonLabel".to_string(),
            file_type: "code".to_string(),
            source_file: "test.py".to_string(),
            source_location: None,
            community: None,
            rationale: None,
            docstring: None,
            metadata: HashMap::new(),
        };
        let node_b = Node {
            id: "dup_b".to_string(),
            label: "CommonLabel".to_string(),
            file_type: "code".to_string(),
            source_file: "test.py".to_string(),
            source_location: None,
            community: None,
            rationale: None,
            docstring: None,
            metadata: HashMap::new(),
        };
        let edge = Edge {
            source: "dup_a".to_string(),
            target: "dup_b".to_string(),
            relation: "calls".to_string(),
            confidence: "EXTRACTED".to_string(),
            source_file: Some("test.py".to_string()),
            weight: 1.0,
            context: None,
        };
        builder.add_nodes(vec![node_a, node_b]).unwrap();
        builder.add_edges(vec![edge]).unwrap();
        assert_eq!(builder.store().node_count().unwrap(), 2);

        let removed = builder.label_dedup().unwrap();
        assert_eq!(removed, 1);
        assert_eq!(builder.store().node_count().unwrap(), 1);
    }

    #[test]
    fn test_build_lang_filter_inferred_calls() {
        let store = MockStore::new();
        let builder = GraphBuilder::new(Box::new(store));
        let py_node = Node {
            id: "py_func".to_string(),
            label: "py_func".to_string(),
            file_type: "code".to_string(),
            source_file: "app.py".to_string(),
            source_location: None,
            community: None,
            rationale: None,
            docstring: None,
            metadata: HashMap::new(),
        };
        let ts_node = Node {
            id: "ts_func".to_string(),
            label: "ts_func".to_string(),
            file_type: "code".to_string(),
            source_file: "app.ts".to_string(),
            source_location: None,
            community: None,
            rationale: None,
            docstring: None,
            metadata: HashMap::new(),
        };
        let cross_edge = Edge {
            source: "py_func".to_string(),
            target: "ts_func".to_string(),
            relation: "calls".to_string(),
            confidence: "INFERRED".to_string(),
            source_file: Some("app.py".to_string()),
            weight: 1.0,
            context: None,
        };
        builder.add_nodes(vec![py_node, ts_node]).unwrap();
        builder.add_edges(vec![cross_edge]).unwrap();
        assert_eq!(builder.store().edge_count().unwrap(), 1);

        let removed = builder.lang_filter_inferred_calls().unwrap();
        assert_eq!(removed, 1);
        assert_eq!(builder.store().edge_count().unwrap(), 0);
    }

    #[test]
    fn test_build_normalize_source_file() {
        let store = MockStore::new();
        let builder = GraphBuilder::new(Box::new(store));
        let root = std::env::temp_dir().join("codesynapse_test_normalize");
        let abs_path = root.join("src/main.py").to_string_lossy().to_string();

        let node = Node {
            id: "main".to_string(),
            label: "Main".to_string(),
            file_type: "code".to_string(),
            source_file: abs_path.clone(),
            source_location: None,
            community: None,
            rationale: None,
            docstring: None,
            metadata: HashMap::new(),
        };
        builder.add_nodes(vec![node]).unwrap();
        builder.normalize_source_file(&root).unwrap();

        let stored = builder.store().get_node("main").unwrap().unwrap();
        assert_eq!(stored.source_file, "src/main.py");
    }

    // --- Edge case tests ---

    #[test]
    fn test_build_empty_fragments_vec() {
        let store = MockStore::new();
        let builder = GraphBuilder::new(Box::new(store));
        builder.build_from_fragments(vec![]).unwrap();
        assert_eq!(builder.store().node_count().unwrap(), 0);
    }

    #[test]
    fn test_build_self_loop_edge() {
        let store = MockStore::new();
        let builder = GraphBuilder::new(Box::new(store));
        let node = Node {
            id: "a".to_string(),
            label: "A".to_string(),
            file_type: "code".to_string(),
            source_file: "test.py".to_string(),
            source_location: None,
            community: None,
            rationale: None,
            docstring: None,
            metadata: HashMap::new(),
        };
        let edge = Edge {
            source: "a".to_string(),
            target: "a".to_string(),
            relation: "self_ref".to_string(),
            confidence: "EXTRACTED".to_string(),
            source_file: Some("test.py".to_string()),
            weight: 1.0,
            context: None,
        };
        builder.add_nodes(vec![node]).unwrap();
        builder.add_edges(vec![edge]).unwrap();
        assert_eq!(builder.store().edge_count().unwrap(), 1);
    }

    #[test]
    fn test_build_edge_missing_node() {
        let store = MockStore::new();
        let builder = GraphBuilder::new(Box::new(store));
        let edge = Edge {
            source: "missing_src".to_string(),
            target: "missing_tgt".to_string(),
            relation: "calls".to_string(),
            confidence: "EXTRACTED".to_string(),
            source_file: Some("test.py".to_string()),
            weight: 1.0,
            context: None,
        };
        builder.add_edges(vec![edge]).unwrap();
        assert_eq!(builder.store().edge_count().unwrap(), 1);
        assert_eq!(builder.store().node_count().unwrap(), 0);
    }

    #[test]
    fn test_build_label_dedup_no_match() {
        let store = MockStore::new();
        let builder = GraphBuilder::new(Box::new(store));
        let node_a = Node {
            id: "a".to_string(),
            label: "Alpha".to_string(),
            file_type: "code".to_string(),
            source_file: "f1.py".to_string(),
            source_location: None,
            community: None,
            rationale: None,
            docstring: None,
            metadata: HashMap::new(),
        };
        let node_b = Node {
            id: "b".to_string(),
            label: "Beta".to_string(),
            file_type: "code".to_string(),
            source_file: "f2.py".to_string(),
            source_location: None,
            community: None,
            rationale: None,
            docstring: None,
            metadata: HashMap::new(),
        };
        builder.add_nodes(vec![node_a, node_b]).unwrap();
        let removed = builder.label_dedup().unwrap();
        assert_eq!(removed, 0, "no dedup when all labels are unique");
        assert_eq!(builder.store().node_count().unwrap(), 2);
    }

    #[test]
    fn test_build_merge_with_prune_nonexistent_source() {
        let store = MockStore::new();
        let builder = GraphBuilder::new(Box::new(store));
        builder
            .add_nodes(vec![Node {
                id: "a".to_string(),
                label: "A".to_string(),
                file_type: "code".to_string(),
                source_file: "real.py".to_string(),
                source_location: None,
                community: None,
                rationale: None,
                docstring: None,
                metadata: HashMap::new(),
            }])
            .unwrap();
        let (removed_nodes, removed_edges) = builder
            .merge_with_prune(vec![], vec![], &["nonexistent.py".to_string()])
            .unwrap();
        assert_eq!(removed_nodes, 0, "no-op on nonexistent source");
        assert_eq!(removed_edges, 0);
        assert_eq!(builder.store().node_count().unwrap(), 1);
    }

    #[test]
    fn test_build_lang_filter_inferred_empty() {
        let store = MockStore::new();
        let builder = GraphBuilder::new(Box::new(store));
        let removed = builder.lang_filter_inferred_calls().unwrap();
        assert_eq!(removed, 0, "no edges to remove on empty graph");
    }

    #[test]
    fn test_build_multiple_fragments_same_nodes() {
        let store = MockStore::new();
        let builder = GraphBuilder::new(Box::new(store));
        let node = Node {
            id: "shared".to_string(),
            label: "Shared".to_string(),
            file_type: "code".to_string(),
            source_file: "a.py".to_string(),
            source_location: None,
            community: None,
            rationale: None,
            docstring: None,
            metadata: HashMap::new(),
        };
        builder
            .build_from_fragments(vec![
                ("a.py".to_string(), vec![node.clone()], vec![]),
                ("b.py".to_string(), vec![node], vec![]),
            ])
            .unwrap();
        assert_eq!(builder.store().node_count().unwrap(), 1);
    }
}
