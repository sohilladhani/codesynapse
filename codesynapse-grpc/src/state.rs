use std::collections::{HashMap, VecDeque};

#[derive(Clone, Debug, PartialEq)]
pub struct GraphNode {
    pub id: String,
    pub label: String,
    pub source_file: String,
    pub source_location: String,
    pub community: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GraphEdge {
    pub source: String,
    pub target: String,
    pub relation: String,
    pub confidence: String,
}

#[derive(Default)]
pub struct GraphState {
    nodes: HashMap<String, GraphNode>,
    edges: Vec<GraphEdge>,
    adj: HashMap<String, Vec<String>>,
}

impl GraphState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_node(&mut self, node: GraphNode) {
        self.adj.entry(node.id.clone()).or_default();
        self.nodes.insert(node.id.clone(), node);
    }

    pub fn add_edge(&mut self, edge: GraphEdge) {
        self.adj
            .entry(edge.source.clone())
            .or_default()
            .push(edge.target.clone());
        self.adj.entry(edge.target.clone()).or_default();
        self.edges.push(edge);
    }

    pub fn get_nodes(&self) -> Vec<GraphNode> {
        self.nodes.values().cloned().collect()
    }

    pub fn get_edges(&self) -> Vec<GraphEdge> {
        self.edges.clone()
    }

    pub fn get_node(&self, id: &str) -> Option<&GraphNode> {
        self.nodes.get(id)
    }

    pub fn search_nodes(&self, query: &str, limit: usize) -> Vec<GraphNode> {
        let q = query.to_lowercase();
        self.nodes
            .values()
            .filter(|n| n.label.to_lowercase().contains(&q))
            .take(limit)
            .cloned()
            .collect()
    }

    pub fn shortest_path(&self, source: &str, target: &str) -> Option<Vec<String>> {
        if source == target {
            return Some(vec![source.to_string()]);
        }
        let mut visited: HashMap<String, Option<String>> = HashMap::new();
        let mut queue = VecDeque::new();
        queue.push_back(source.to_string());
        visited.insert(source.to_string(), None);

        while let Some(current) = queue.pop_front() {
            if current == target {
                let mut path = vec![target.to_string()];
                let mut node = target.to_string();
                while let Some(Some(prev)) = visited.get(&node) {
                    path.push(prev.clone());
                    node = prev.clone();
                }
                path.reverse();
                return Some(path);
            }
            if let Some(neighbors) = self.adj.get(&current) {
                for next in neighbors {
                    if !visited.contains_key(next) {
                        visited.insert(next.clone(), Some(current.clone()));
                        queue.push_back(next.clone());
                    }
                }
            }
        }
        None
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: &str, label: &str) -> GraphNode {
        GraphNode {
            id: id.to_string(),
            label: label.to_string(),
            source_file: "test.rs".to_string(),
            source_location: "1:1".to_string(),
            community: 0,
        }
    }

    fn edge(src: &str, tgt: &str) -> GraphEdge {
        GraphEdge {
            source: src.to_string(),
            target: tgt.to_string(),
            relation: "calls".to_string(),
            confidence: "1.0".to_string(),
        }
    }

    #[test]
    fn test_empty_graph() {
        let s = GraphState::new();
        assert_eq!(s.node_count(), 0);
        assert_eq!(s.edge_count(), 0);
    }

    #[test]
    fn test_add_node() {
        let mut s = GraphState::new();
        s.add_node(node("n1", "Foo"));
        assert_eq!(s.node_count(), 1);
        assert!(s.get_node("n1").is_some());
    }

    #[test]
    fn test_add_duplicate_node_overwrites() {
        let mut s = GraphState::new();
        s.add_node(node("n1", "Foo"));
        s.add_node(node("n1", "Bar"));
        assert_eq!(s.node_count(), 1);
        assert_eq!(s.get_node("n1").unwrap().label, "Bar");
    }

    #[test]
    fn test_add_edge() {
        let mut s = GraphState::new();
        s.add_node(node("a", "A"));
        s.add_node(node("b", "B"));
        s.add_edge(edge("a", "b"));
        assert_eq!(s.edge_count(), 1);
    }

    #[test]
    fn test_get_nodes_returns_all() {
        let mut s = GraphState::new();
        s.add_node(node("a", "A"));
        s.add_node(node("b", "B"));
        assert_eq!(s.get_nodes().len(), 2);
    }

    #[test]
    fn test_get_edges_returns_all() {
        let mut s = GraphState::new();
        s.add_node(node("a", "A"));
        s.add_node(node("b", "B"));
        s.add_node(node("c", "C"));
        s.add_edge(edge("a", "b"));
        s.add_edge(edge("b", "c"));
        assert_eq!(s.get_edges().len(), 2);
    }

    #[test]
    fn test_get_node_found() {
        let mut s = GraphState::new();
        s.add_node(node("x", "X"));
        assert_eq!(s.get_node("x").unwrap().label, "X");
    }

    #[test]
    fn test_get_node_not_found() {
        let s = GraphState::new();
        assert!(s.get_node("missing").is_none());
    }

    #[test]
    fn test_search_nodes_exact() {
        let mut s = GraphState::new();
        s.add_node(node("1", "AuthService"));
        s.add_node(node("2", "UserService"));
        let results = s.search_nodes("AuthService", 10);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "1");
    }

    #[test]
    fn test_search_nodes_case_insensitive() {
        let mut s = GraphState::new();
        s.add_node(node("1", "AuthService"));
        s.add_node(node("2", "AuthController"));
        s.add_node(node("3", "UserService"));
        let results = s.search_nodes("auth", 10);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_search_nodes_empty_query_matches_all() {
        let mut s = GraphState::new();
        s.add_node(node("1", "A"));
        s.add_node(node("2", "B"));
        let results = s.search_nodes("", 10);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_search_nodes_no_match() {
        let mut s = GraphState::new();
        s.add_node(node("1", "Foo"));
        let results = s.search_nodes("zzz", 10);
        assert!(results.is_empty());
    }

    #[test]
    fn test_search_nodes_limit_applied() {
        let mut s = GraphState::new();
        for i in 0..5 {
            s.add_node(node(&i.to_string(), &format!("Service{}", i)));
        }
        let results = s.search_nodes("service", 3);
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn test_shortest_path_direct() {
        let mut s = GraphState::new();
        s.add_node(node("a", "A"));
        s.add_node(node("b", "B"));
        s.add_edge(edge("a", "b"));
        let path = s.shortest_path("a", "b").unwrap();
        assert_eq!(path, vec!["a", "b"]);
    }

    #[test]
    fn test_shortest_path_multi_hop() {
        let mut s = GraphState::new();
        for id in ["a", "b", "c"] {
            s.add_node(node(id, id));
        }
        s.add_edge(edge("a", "b"));
        s.add_edge(edge("b", "c"));
        let path = s.shortest_path("a", "c").unwrap();
        assert_eq!(path, vec!["a", "b", "c"]);
    }

    #[test]
    fn test_shortest_path_self() {
        let mut s = GraphState::new();
        s.add_node(node("a", "A"));
        let path = s.shortest_path("a", "a").unwrap();
        assert_eq!(path, vec!["a"]);
    }

    #[test]
    fn test_shortest_path_not_found() {
        let mut s = GraphState::new();
        s.add_node(node("a", "A"));
        s.add_node(node("b", "B"));
        assert!(s.shortest_path("a", "b").is_none());
    }
}
