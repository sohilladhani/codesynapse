use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use wasm_bindgen::prelude::*;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WasmNode {
    pub id: String,
    pub label: String,
    pub file_type: String,
    pub source_file: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WasmEdge {
    pub source: String,
    pub target: String,
    pub relation: String,
    pub weight: f64,
}

#[derive(Default)]
pub struct WasmGraphInner {
    nodes: HashMap<String, WasmNode>,
    edges: Vec<WasmEdge>,
}

impl WasmGraphInner {
    pub fn new() -> Self {
        WasmGraphInner {
            nodes: HashMap::new(),
            edges: Vec::new(),
        }
    }

    pub fn add_node(&mut self, json: &str) -> Result<(), String> {
        let node: WasmNode = serde_json::from_str(json).map_err(|e| e.to_string())?;
        self.nodes.insert(node.id.clone(), node);
        Ok(())
    }

    pub fn add_edge(&mut self, json: &str) -> Result<(), String> {
        let edge: WasmEdge = serde_json::from_str(json).map_err(|e| e.to_string())?;
        self.edges.push(edge);
        Ok(())
    }

    pub fn get_node(&self, id: &str) -> Option<String> {
        self.nodes
            .get(id)
            .and_then(|n| serde_json::to_string(n).ok())
    }

    pub fn get_all_nodes(&self) -> String {
        let nodes: Vec<&WasmNode> = self.nodes.values().collect();
        serde_json::to_string(&nodes).unwrap_or_else(|_| "[]".to_string())
    }

    pub fn get_all_edges(&self) -> String {
        serde_json::to_string(&self.edges).unwrap_or_else(|_| "[]".to_string())
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    pub fn neighbors(&self, id: &str) -> String {
        let neighbors: Vec<&WasmNode> = self
            .edges
            .iter()
            .filter(|e| e.source == id)
            .filter_map(|e| self.nodes.get(&e.target))
            .collect();
        serde_json::to_string(&neighbors).unwrap_or_else(|_| "[]".to_string())
    }

    pub fn search(&self, query: &str, top_k: usize) -> String {
        let q = query.to_lowercase();
        let mut results: Vec<&WasmNode> = self
            .nodes
            .values()
            .filter(|n| {
                n.label.to_lowercase().contains(&q)
                    || n.file_type.to_lowercase().contains(&q)
                    || n.source_file.to_lowercase().contains(&q)
            })
            .collect();
        results.truncate(top_k);
        serde_json::to_string(&results).unwrap_or_else(|_| "[]".to_string())
    }

    pub fn shortest_path(&self, src: &str, tgt: &str) -> Option<String> {
        if !self.nodes.contains_key(src) || !self.nodes.contains_key(tgt) {
            return None;
        }
        if src == tgt {
            let path = vec![src];
            return serde_json::to_string(&path).ok();
        }
        let mut visited: HashMap<String, Option<String>> = HashMap::new();
        let mut queue: VecDeque<String> = VecDeque::new();
        visited.insert(src.to_string(), None);
        queue.push_back(src.to_string());
        while let Some(current) = queue.pop_front() {
            for edge in self.edges.iter().filter(|e| e.source == current) {
                if !visited.contains_key(&edge.target) {
                    visited.insert(edge.target.clone(), Some(current.clone()));
                    if edge.target == tgt {
                        let mut path = vec![tgt.to_string()];
                        let mut cur = tgt.to_string();
                        while let Some(Some(prev)) = visited.get(&cur) {
                            path.push(prev.clone());
                            cur = prev.clone();
                        }
                        path.reverse();
                        return serde_json::to_string(&path).ok();
                    }
                    queue.push_back(edge.target.clone());
                }
            }
        }
        None
    }

    pub fn clear(&mut self) {
        self.nodes.clear();
        self.edges.clear();
    }
}

#[derive(Default)]
#[wasm_bindgen]
pub struct WasmGraph {
    inner: WasmGraphInner,
}

#[wasm_bindgen]
impl WasmGraph {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        WasmGraph {
            inner: WasmGraphInner::new(),
        }
    }

    pub fn add_node(&mut self, json: &str) -> Result<(), JsValue> {
        self.inner.add_node(json).map_err(|e| JsValue::from_str(&e))
    }

    pub fn add_edge(&mut self, json: &str) -> Result<(), JsValue> {
        self.inner.add_edge(json).map_err(|e| JsValue::from_str(&e))
    }

    pub fn get_node(&self, id: &str) -> Option<String> {
        self.inner.get_node(id)
    }

    pub fn get_all_nodes(&self) -> String {
        self.inner.get_all_nodes()
    }

    pub fn get_all_edges(&self) -> String {
        self.inner.get_all_edges()
    }

    pub fn node_count(&self) -> usize {
        self.inner.node_count()
    }

    pub fn edge_count(&self) -> usize {
        self.inner.edge_count()
    }

    pub fn neighbors(&self, id: &str) -> String {
        self.inner.neighbors(id)
    }

    pub fn search(&self, query: &str, top_k: usize) -> String {
        self.inner.search(query, top_k)
    }

    pub fn shortest_path(&self, src: &str, tgt: &str) -> Option<String> {
        self.inner.shortest_path(src, tgt)
    }

    pub fn clear(&mut self) {
        self.inner.clear()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node_json(id: &str, label: &str) -> String {
        format!(
            r#"{{"id":"{id}","label":"{label}","file_type":"function","source_file":"main.rs"}}"#
        )
    }

    fn edge_json(src: &str, tgt: &str, rel: &str) -> String {
        format!(r#"{{"source":"{src}","target":"{tgt}","relation":"{rel}","weight":1.0}}"#)
    }

    #[test]
    fn test_new_empty() {
        let g = WasmGraphInner::new();
        assert_eq!(g.node_count(), 0);
        assert_eq!(g.edge_count(), 0);
    }

    #[test]
    fn test_add_node_valid() {
        let mut g = WasmGraphInner::new();
        assert!(g.add_node(&node_json("a", "A")).is_ok());
        assert_eq!(g.node_count(), 1);
    }

    #[test]
    fn test_add_node_invalid_json() {
        let mut g = WasmGraphInner::new();
        assert!(g.add_node("not json").is_err());
    }

    #[test]
    fn test_add_edge_valid() {
        let mut g = WasmGraphInner::new();
        g.add_node(&node_json("a", "A")).unwrap();
        g.add_node(&node_json("b", "B")).unwrap();
        assert!(g.add_edge(&edge_json("a", "b", "calls")).is_ok());
        assert_eq!(g.edge_count(), 1);
    }

    #[test]
    fn test_add_edge_invalid_json() {
        let mut g = WasmGraphInner::new();
        assert!(g.add_edge("{bad}").is_err());
    }

    #[test]
    fn test_get_node_existing() {
        let mut g = WasmGraphInner::new();
        g.add_node(&node_json("x", "X")).unwrap();
        let result = g.get_node("x");
        assert!(result.is_some());
        let parsed: WasmNode = serde_json::from_str(&result.unwrap()).unwrap();
        assert_eq!(parsed.id, "x");
        assert_eq!(parsed.label, "X");
    }

    #[test]
    fn test_get_node_missing() {
        let g = WasmGraphInner::new();
        assert!(g.get_node("missing").is_none());
    }

    #[test]
    fn test_get_all_nodes() {
        let mut g = WasmGraphInner::new();
        g.add_node(&node_json("a", "A")).unwrap();
        g.add_node(&node_json("b", "B")).unwrap();
        let nodes: Vec<WasmNode> = serde_json::from_str(&g.get_all_nodes()).unwrap();
        assert_eq!(nodes.len(), 2);
    }

    #[test]
    fn test_get_all_edges() {
        let mut g = WasmGraphInner::new();
        g.add_node(&node_json("a", "A")).unwrap();
        g.add_node(&node_json("b", "B")).unwrap();
        g.add_edge(&edge_json("a", "b", "calls")).unwrap();
        let edges: Vec<WasmEdge> = serde_json::from_str(&g.get_all_edges()).unwrap();
        assert_eq!(edges.len(), 1);
    }

    #[test]
    fn test_node_count() {
        let mut g = WasmGraphInner::new();
        g.add_node(&node_json("a", "A")).unwrap();
        g.add_node(&node_json("b", "B")).unwrap();
        assert_eq!(g.node_count(), 2);
    }

    #[test]
    fn test_edge_count() {
        let mut g = WasmGraphInner::new();
        g.add_node(&node_json("a", "A")).unwrap();
        g.add_node(&node_json("b", "B")).unwrap();
        g.add_edge(&edge_json("a", "b", "calls")).unwrap();
        g.add_edge(&edge_json("a", "b", "imports")).unwrap();
        assert_eq!(g.edge_count(), 2);
    }

    #[test]
    fn test_neighbors_with_edges() {
        let mut g = WasmGraphInner::new();
        g.add_node(&node_json("a", "A")).unwrap();
        g.add_node(&node_json("b", "B")).unwrap();
        g.add_node(&node_json("c", "C")).unwrap();
        g.add_edge(&edge_json("a", "b", "calls")).unwrap();
        g.add_edge(&edge_json("a", "c", "calls")).unwrap();
        let neighbors: Vec<WasmNode> = serde_json::from_str(&g.neighbors("a")).unwrap();
        assert_eq!(neighbors.len(), 2);
    }

    #[test]
    fn test_neighbors_no_edges() {
        let mut g = WasmGraphInner::new();
        g.add_node(&node_json("a", "A")).unwrap();
        let neighbors: Vec<WasmNode> = serde_json::from_str(&g.neighbors("a")).unwrap();
        assert_eq!(neighbors.len(), 0);
    }

    #[test]
    fn test_search_by_label() {
        let mut g = WasmGraphInner::new();
        g.add_node(&node_json("a", "AuthService")).unwrap();
        g.add_node(&node_json("b", "UserModel")).unwrap();
        let results: Vec<WasmNode> = serde_json::from_str(&g.search("auth", 10)).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "a");
    }

    #[test]
    fn test_search_top_k_limit() {
        let mut g = WasmGraphInner::new();
        for i in 0..10 {
            g.add_node(&node_json(&format!("n{i}"), "handler")).unwrap();
        }
        let results: Vec<WasmNode> = serde_json::from_str(&g.search("handler", 3)).unwrap();
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn test_shortest_path_direct() {
        let mut g = WasmGraphInner::new();
        g.add_node(&node_json("a", "A")).unwrap();
        g.add_node(&node_json("b", "B")).unwrap();
        g.add_edge(&edge_json("a", "b", "calls")).unwrap();
        let path: Vec<String> = serde_json::from_str(&g.shortest_path("a", "b").unwrap()).unwrap();
        assert_eq!(path, vec!["a", "b"]);
    }

    #[test]
    fn test_shortest_path_multihop() {
        let mut g = WasmGraphInner::new();
        for id in ["a", "b", "c"] {
            g.add_node(&node_json(id, id)).unwrap();
        }
        g.add_edge(&edge_json("a", "b", "calls")).unwrap();
        g.add_edge(&edge_json("b", "c", "calls")).unwrap();
        let path: Vec<String> = serde_json::from_str(&g.shortest_path("a", "c").unwrap()).unwrap();
        assert_eq!(path, vec!["a", "b", "c"]);
    }

    #[test]
    fn test_shortest_path_no_path() {
        let mut g = WasmGraphInner::new();
        g.add_node(&node_json("a", "A")).unwrap();
        g.add_node(&node_json("b", "B")).unwrap();
        assert!(g.shortest_path("a", "b").is_none());
    }

    #[test]
    fn test_clear() {
        let mut g = WasmGraphInner::new();
        g.add_node(&node_json("a", "A")).unwrap();
        g.add_node(&node_json("b", "B")).unwrap();
        g.add_edge(&edge_json("a", "b", "calls")).unwrap();
        g.clear();
        assert_eq!(g.node_count(), 0);
        assert_eq!(g.edge_count(), 0);
    }
}

#[cfg(test)]
mod wasm_bindgen_tests {
    use super::*;
    use wasm_bindgen_test::*;

    fn node(id: &str, label: &str) -> String {
        format!(
            r#"{{"id":"{id}","label":"{label}","file_type":"function","source_file":"main.rs"}}"#
        )
    }

    fn edge(src: &str, tgt: &str, rel: &str) -> String {
        format!(r#"{{"source":"{src}","target":"{tgt}","relation":"{rel}","weight":1.0}}"#)
    }

    #[wasm_bindgen_test]
    fn wasm_new_is_empty() {
        let g = WasmGraph::new();
        assert_eq!(g.node_count(), 0);
        assert_eq!(g.edge_count(), 0);
    }

    #[wasm_bindgen_test]
    fn wasm_add_node_increments_count() {
        let mut g = WasmGraph::new();
        g.add_node(&node("a", "Alpha")).unwrap();
        assert_eq!(g.node_count(), 1);
    }

    #[wasm_bindgen_test]
    fn wasm_add_node_invalid_json_returns_err() {
        let mut g = WasmGraph::new();
        assert!(g.add_node("not json").is_err());
    }

    #[wasm_bindgen_test]
    fn wasm_add_edge_increments_count() {
        let mut g = WasmGraph::new();
        g.add_node(&node("a", "A")).unwrap();
        g.add_node(&node("b", "B")).unwrap();
        g.add_edge(&edge("a", "b", "calls")).unwrap();
        assert_eq!(g.edge_count(), 1);
    }

    #[wasm_bindgen_test]
    fn wasm_add_edge_invalid_json_returns_err() {
        let mut g = WasmGraph::new();
        assert!(g.add_edge("{bad}").is_err());
    }

    #[wasm_bindgen_test]
    fn wasm_get_node_found_returns_json() {
        let mut g = WasmGraph::new();
        g.add_node(&node("x", "Xavier")).unwrap();
        let result = g.get_node("x").unwrap();
        let parsed: WasmNode = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed.id, "x");
        assert_eq!(parsed.label, "Xavier");
    }

    #[wasm_bindgen_test]
    fn wasm_get_node_missing_returns_none() {
        let g = WasmGraph::new();
        assert!(g.get_node("nope").is_none());
    }

    #[wasm_bindgen_test]
    fn wasm_get_all_nodes_returns_all() {
        let mut g = WasmGraph::new();
        g.add_node(&node("a", "A")).unwrap();
        g.add_node(&node("b", "B")).unwrap();
        let nodes: Vec<WasmNode> = serde_json::from_str(&g.get_all_nodes()).unwrap();
        assert_eq!(nodes.len(), 2);
    }

    #[wasm_bindgen_test]
    fn wasm_get_all_edges_returns_all() {
        let mut g = WasmGraph::new();
        g.add_node(&node("a", "A")).unwrap();
        g.add_node(&node("b", "B")).unwrap();
        g.add_edge(&edge("a", "b", "calls")).unwrap();
        let edges: Vec<WasmEdge> = serde_json::from_str(&g.get_all_edges()).unwrap();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].relation, "calls");
    }

    #[wasm_bindgen_test]
    fn wasm_neighbors_returns_connected_nodes() {
        let mut g = WasmGraph::new();
        g.add_node(&node("a", "A")).unwrap();
        g.add_node(&node("b", "B")).unwrap();
        g.add_node(&node("c", "C")).unwrap();
        g.add_edge(&edge("a", "b", "calls")).unwrap();
        g.add_edge(&edge("a", "c", "calls")).unwrap();
        let neighbors: Vec<WasmNode> = serde_json::from_str(&g.neighbors("a")).unwrap();
        assert_eq!(neighbors.len(), 2);
    }

    #[wasm_bindgen_test]
    fn wasm_neighbors_unknown_node_returns_empty() {
        let g = WasmGraph::new();
        let neighbors: Vec<WasmNode> = serde_json::from_str(&g.neighbors("ghost")).unwrap();
        assert!(neighbors.is_empty());
    }

    #[wasm_bindgen_test]
    fn wasm_search_matches_label_substring() {
        let mut g = WasmGraph::new();
        g.add_node(&node("1", "AuthService")).unwrap();
        g.add_node(&node("2", "UserModel")).unwrap();
        let results: Vec<WasmNode> = serde_json::from_str(&g.search("auth", 10)).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "1");
    }

    #[wasm_bindgen_test]
    fn wasm_search_respects_top_k() {
        let mut g = WasmGraph::new();
        for i in 0..8 {
            g.add_node(&node(&format!("n{i}"), "handler")).unwrap();
        }
        let results: Vec<WasmNode> = serde_json::from_str(&g.search("handler", 3)).unwrap();
        assert_eq!(results.len(), 3);
    }

    #[wasm_bindgen_test]
    fn wasm_shortest_path_direct_edge() {
        let mut g = WasmGraph::new();
        g.add_node(&node("a", "A")).unwrap();
        g.add_node(&node("b", "B")).unwrap();
        g.add_edge(&edge("a", "b", "calls")).unwrap();
        let path: Vec<String> = serde_json::from_str(&g.shortest_path("a", "b").unwrap()).unwrap();
        assert_eq!(path, vec!["a", "b"]);
    }

    #[wasm_bindgen_test]
    fn wasm_shortest_path_multihop() {
        let mut g = WasmGraph::new();
        for id in ["a", "b", "c"] {
            g.add_node(&node(id, id)).unwrap();
        }
        g.add_edge(&edge("a", "b", "calls")).unwrap();
        g.add_edge(&edge("b", "c", "calls")).unwrap();
        let path: Vec<String> = serde_json::from_str(&g.shortest_path("a", "c").unwrap()).unwrap();
        assert_eq!(path, vec!["a", "b", "c"]);
    }

    #[wasm_bindgen_test]
    fn wasm_shortest_path_no_path_returns_none() {
        let mut g = WasmGraph::new();
        g.add_node(&node("a", "A")).unwrap();
        g.add_node(&node("b", "B")).unwrap();
        assert!(g.shortest_path("a", "b").is_none());
    }

    #[wasm_bindgen_test]
    fn wasm_shortest_path_unknown_node_returns_none() {
        let g = WasmGraph::new();
        assert!(g.shortest_path("x", "y").is_none());
    }

    #[wasm_bindgen_test]
    fn wasm_clear_resets_graph() {
        let mut g = WasmGraph::new();
        g.add_node(&node("a", "A")).unwrap();
        g.add_edge(&edge("a", "a", "self")).unwrap();
        g.clear();
        assert_eq!(g.node_count(), 0);
        assert_eq!(g.edge_count(), 0);
    }
}
