use crate::error::Result;
use crate::graph::GraphStore;
use crate::types::{Edge, Node, NodeId, QueryResult};
use std::collections::VecDeque;
use std::collections::{HashMap, HashSet};

pub struct QueryEngine {
    store: Box<dyn GraphStore>,
    max_nodes: usize,
}

impl QueryEngine {
    pub fn new(store: Box<dyn GraphStore>) -> Self {
        QueryEngine {
            store,
            max_nodes: 10_000,
        }
    }

    pub fn with_max_nodes(store: Box<dyn GraphStore>, max_nodes: usize) -> Self {
        QueryEngine { store, max_nodes }
    }

    pub fn store_ref(&self) -> &dyn GraphStore {
        self.store.as_ref()
    }

    pub fn query_text(
        &self,
        query: &str,
        mode: &str,
        depth: usize,
        token_budget: Option<usize>,
        context_filter: Option<&str>,
    ) -> Result<QueryResult> {
        let seeds = self.store.search(query, 10)?;
        let seed_nodes: Vec<Node> = seeds.into_iter().map(|(_, n)| n).collect();

        if seed_nodes.is_empty() {
            return Ok(QueryResult {
                seed_nodes: vec![],
                neighborhood: vec![],
                edges: vec![],
                truncated: false,
            });
        }

        let node_count = self.store.node_count()?;
        if node_count > self.max_nodes {
            return Ok(QueryResult {
                seed_nodes: vec![],
                neighborhood: vec![],
                edges: vec![],
                truncated: true,
            });
        }

        match mode {
            "bfs" => self.traverse_bfs(&seed_nodes, depth, context_filter, token_budget),
            "dfs" => self.traverse_dfs(&seed_nodes, depth, context_filter, token_budget),
            _ => self.traverse_bfs(&seed_nodes, depth, context_filter, token_budget),
        }
    }

    fn traverse_bfs(
        &self,
        seed_nodes: &[Node],
        depth: usize,
        context_filter: Option<&str>,
        token_budget: Option<usize>,
    ) -> Result<QueryResult> {
        let mut visited: HashSet<NodeId> = HashSet::new();
        let mut neighborhood: Vec<Node> = Vec::new();
        let mut edges: Vec<Edge> = Vec::new();

        for seed in seed_nodes {
            visited.insert(seed.id.clone());
            neighborhood.push(seed.clone());
        }

        if depth > 0 {
            let mut frontier: Vec<NodeId> = seed_nodes.iter().map(|n| n.id.clone()).collect();
            for _ in 0..depth {
                let mut next_frontier = Vec::new();
                for node_id in &frontier {
                    if let Ok(neighbors) = self.store.neighbors(node_id, context_filter) {
                        for (neighbor, edge) in &neighbors {
                            if visited.insert(neighbor.id.clone()) {
                                neighborhood.push(neighbor.clone());
                                next_frontier.push(neighbor.id.clone());
                            }
                            edges.push(edge.clone());
                        }
                    }
                }
                frontier = next_frontier;
                if frontier.is_empty() {
                    break;
                }
                if let Some(budget) = token_budget {
                    let token_count: usize =
                        neighborhood.iter().map(|n| n.label.len()).sum::<usize>()
                            + edges.iter().map(|e| e.relation.len()).sum::<usize>();
                    if token_count >= budget {
                        return Ok(QueryResult {
                            seed_nodes: seed_nodes.to_vec(),
                            neighborhood,
                            edges,
                            truncated: true,
                        });
                    }
                }
            }
        }

        Ok(QueryResult {
            seed_nodes: seed_nodes.to_vec(),
            neighborhood,
            edges,
            truncated: false,
        })
    }

    fn traverse_dfs(
        &self,
        seed_nodes: &[Node],
        depth: usize,
        context_filter: Option<&str>,
        token_budget: Option<usize>,
    ) -> Result<QueryResult> {
        let mut visited: HashSet<NodeId> = HashSet::new();
        let mut neighborhood: Vec<Node> = Vec::new();
        let mut edges: Vec<Edge> = Vec::new();

        for seed in seed_nodes {
            visited.insert(seed.id.clone());
            neighborhood.push(seed.clone());
        }

        if depth > 0 {
            let mut stack: Vec<(NodeId, usize)> =
                seed_nodes.iter().map(|n| (n.id.clone(), 0)).collect();

            while let Some((node_id, current_depth)) = stack.pop() {
                if current_depth >= depth {
                    continue;
                }

                if let Ok(neighbors) = self.store.neighbors(&node_id, context_filter) {
                    for (neighbor, edge) in &neighbors {
                        if visited.insert(neighbor.id.clone()) {
                            neighborhood.push(neighbor.clone());
                            stack.push((neighbor.id.clone(), current_depth + 1));
                        }
                        edges.push(edge.clone());
                    }
                }

                if let Some(budget) = token_budget {
                    let token_count: usize =
                        neighborhood.iter().map(|n| n.label.len()).sum::<usize>()
                            + edges.iter().map(|e| e.relation.len()).sum::<usize>();
                    if token_count >= budget {
                        return Ok(QueryResult {
                            seed_nodes: seed_nodes.to_vec(),
                            neighborhood,
                            edges,
                            truncated: true,
                        });
                    }
                }
            }
        }

        Ok(QueryResult {
            seed_nodes: seed_nodes.to_vec(),
            neighborhood,
            edges,
            truncated: false,
        })
    }

    pub fn dijkstra_path(&self, src: &str, tgt: &str) -> Result<Option<(Vec<Node>, f64)>> {
        if src == tgt {
            return Ok(self.store.get_node(src)?.map(|n| (vec![n], 0.0)));
        }

        let all_nodes = self.store.get_all_nodes()?;
        let all_edges = self.store.get_all_edges()?;

        let mut dist: HashMap<&str, f64> = HashMap::new();
        let mut prev: HashMap<&str, String> = HashMap::new();
        let mut unvisited: HashSet<&str> = HashSet::new();

        for node in &all_nodes {
            dist.insert(node.id.as_str(), f64::INFINITY);
            unvisited.insert(node.id.as_str());
        }
        dist.insert(src, 0.0);

        while !unvisited.is_empty() {
            let current = unvisited
                .iter()
                .min_by(|a, b| {
                    dist.get(*a)
                        .unwrap_or(&f64::INFINITY)
                        .partial_cmp(dist.get(*b).unwrap_or(&f64::INFINITY))
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .cloned();

            let current = match current {
                Some(c) => c,
                None => break,
            };

            if current == tgt {
                break;
            }

            unvisited.remove(current);

            let current_dist = *dist.get(current).unwrap_or(&f64::INFINITY);
            if current_dist == f64::INFINITY {
                break;
            }

            for edge in &all_edges {
                let neighbor = if edge.source == current {
                    Some(edge.target.as_str())
                } else if edge.target == current {
                    Some(edge.source.as_str())
                } else {
                    None
                };

                if let Some(neighbor) = neighbor {
                    if unvisited.contains(neighbor) {
                        let alt = current_dist + edge.weight;
                        if alt < *dist.get(neighbor).unwrap_or(&f64::INFINITY) {
                            dist.insert(neighbor, alt);
                            prev.insert(neighbor, current.to_string());
                        }
                    }
                }
            }
        }

        if prev.contains_key(tgt) || src == tgt {
            let mut path_nodes = Vec::new();
            let mut current = tgt.to_string();
            while current != src {
                if let Some(node) = self.store.get_node(&current)? {
                    path_nodes.push(node);
                }
                current = prev.get(current.as_str()).cloned().unwrap_or_default();
            }
            if let Some(node) = self.store.get_node(src)? {
                path_nodes.push(node);
            }
            path_nodes.reverse();

            let total_weight = *dist.get(tgt).unwrap_or(&f64::INFINITY);
            Ok(Some((path_nodes, total_weight)))
        } else {
            Ok(None)
        }
    }

    pub fn dfs(&self, src: &str, depth: usize, relation_filter: Option<&str>) -> Result<Vec<Node>> {
        let mut visited: HashSet<NodeId> = HashSet::new();
        let mut result: Vec<Node> = Vec::new();
        let mut stack: Vec<(NodeId, usize)> = vec![(src.to_string(), 0)];

        while let Some((node_id, d)) = stack.pop() {
            if d > depth {
                continue;
            }
            if !visited.insert(node_id.clone()) {
                continue;
            }
            if let Some(node) = self.store.get_node(&node_id)? {
                result.push(node);
            }
            if d < depth {
                if let Ok(neighbors) = self.store.neighbors(&node_id, relation_filter) {
                    for (neighbor, _) in &neighbors {
                        if !visited.contains(&neighbor.id) {
                            stack.push((neighbor.id.clone(), d + 1));
                        }
                    }
                }
            }
        }

        Ok(result)
    }

    pub fn get_node_by_label(&self, label: &str) -> Result<Option<Node>> {
        let nodes = self.store.search(label, 1)?;
        Ok(nodes.into_iter().next().map(|(_, n)| n))
    }

    pub fn get_node_by_id(&self, id: &str) -> Result<Option<Node>> {
        self.store.get_node(id)
    }

    pub fn resolve_node(&self, id_or_label: &str) -> Result<Option<Node>> {
        if let Some(node) = self.store.get_node(id_or_label)? {
            return Ok(Some(node));
        }
        self.get_node_by_label(id_or_label)
    }

    pub fn get_neighbors(
        &self,
        id: &str,
        relation_filter: Option<&str>,
    ) -> Result<Vec<(Node, Edge)>> {
        self.store.neighbors(id, relation_filter)
    }

    pub fn shortest_path(&self, src: &str, tgt: &str) -> Result<Option<Vec<Node>>> {
        self.store.shortest_path(src, tgt)
    }

    pub fn graph_stats(&self) -> Result<String> {
        let node_count = self.store.node_count()?;
        let edge_count = self.store.edge_count()?;
        Ok(format!("Nodes: {}, Edges: {}", node_count, edge_count))
    }

    pub fn get_community(&self, community_id: usize) -> Result<Vec<Node>> {
        let nodes = self.store.get_all_nodes()?;
        Ok(nodes
            .into_iter()
            .filter(|n| n.community == Some(community_id))
            .collect())
    }

    pub fn god_nodes(&self, top_n: usize) -> Result<Vec<Node>> {
        let edges = self.store.get_all_edges()?;
        let all_nodes = self.store.get_all_nodes()?;
        let mut degree: HashMap<NodeId, usize> = HashMap::new();
        for edge in &edges {
            *degree.entry(edge.source.clone()).or_insert(0) += 1;
            *degree.entry(edge.target.clone()).or_insert(0) += 1;
        }
        let mut ranked: Vec<(usize, NodeId)> = degree.into_iter().map(|(k, v)| (v, k)).collect();
        ranked.sort_by_key(|k| std::cmp::Reverse(k.0));
        ranked.truncate(top_n);

        let id_set: HashSet<&str> = ranked.iter().map(|(_, id)| id.as_str()).collect();
        let mut result: Vec<Node> = all_nodes
            .into_iter()
            .filter(|n| id_set.contains(n.id.as_str()))
            .collect();
        result.sort_by(|a, b| {
            let da = ranked
                .iter()
                .find(|(_, id)| id == &a.id)
                .map(|(d, _)| *d)
                .unwrap_or(0);
            let db = ranked
                .iter()
                .find(|(_, id)| id == &b.id)
                .map(|(d, _)| *d)
                .unwrap_or(0);
            db.cmp(&da)
        });
        Ok(result)
    }

    pub fn shortest_path_with_max_hops(
        &self,
        src: &str,
        tgt: &str,
        max_hops: usize,
    ) -> Result<Option<Vec<Node>>> {
        if src == tgt {
            return self.store.get_node(src).map(|n| n.map(|n| vec![n]));
        }

        let mut visited: HashSet<NodeId> = HashSet::new();
        let mut queue: VecDeque<(NodeId, usize)> = VecDeque::new();
        let mut parent: HashMap<NodeId, NodeId> = HashMap::new();

        visited.insert(src.to_string());
        queue.push_back((src.to_string(), 0));

        while let Some((current, depth)) = queue.pop_front() {
            if depth >= max_hops {
                continue;
            }

            let neighbors = self.store.neighbors(&current, None)?;
            for (neighbor, _) in &neighbors {
                if visited.insert(neighbor.id.clone()) {
                    parent.insert(neighbor.id.clone(), current.clone());
                    if neighbor.id == tgt {
                        let mut path = Vec::new();
                        let mut cur = tgt.to_string();
                        while cur != src {
                            if let Some(n) = self.store.get_node(&cur)? {
                                path.push(n);
                            }
                            cur = parent.get(&cur).cloned().unwrap_or_default();
                        }
                        if let Some(n) = self.store.get_node(src)? {
                            path.push(n);
                        }
                        path.reverse();
                        return Ok(Some(path));
                    }
                    queue.push_back((neighbor.id.clone(), depth + 1));
                }
            }
        }

        Ok(None)
    }

    pub fn infer_context_filter(question: &str) -> Option<String> {
        const CONTEXT_HINTS: &[(&str, &[&str])] = &[
            (
                "call",
                &["call", "calls", "called", "invoke", "invokes", "invoked"],
            ),
            (
                "import",
                &["import", "imports", "imported", "module", "modules"],
            ),
            (
                "field",
                &[
                    "field",
                    "fields",
                    "member",
                    "members",
                    "property",
                    "properties",
                ],
            ),
            (
                "parameter_type",
                &[
                    "parameter",
                    "parameters",
                    "param",
                    "params",
                    "argument",
                    "arguments",
                ],
            ),
            ("return_type", &["return", "returns", "returned"]),
            (
                "generic_arg",
                &["generic", "generics", "template", "templates"],
            ),
        ];
        let tokens: std::collections::HashSet<String> = question
            .split(|c: char| !c.is_alphanumeric())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_lowercase())
            .collect();
        for (context, hints) in CONTEXT_HINTS {
            if hints.iter().any(|h| tokens.contains(*h)) {
                return Some(context.to_string());
            }
        }
        None
    }

    pub fn resolve_context_filter(question: &str, explicit: Option<&str>) -> Option<String> {
        const ALIASES: &[(&str, &str)] = &[
            ("param", "parameter_type"),
            ("params", "parameter_type"),
            ("parameter", "parameter_type"),
            ("parameters", "parameter_type"),
            ("argument", "parameter_type"),
            ("arguments", "parameter_type"),
            ("arg", "parameter_type"),
            ("args", "parameter_type"),
            ("return", "return_type"),
            ("returns", "return_type"),
            ("returned", "return_type"),
            ("generic", "generic_arg"),
            ("generics", "generic_arg"),
            ("template", "generic_arg"),
            ("templates", "generic_arg"),
            ("calls", "call"),
            ("called", "call"),
            ("invoke", "call"),
            ("invocation", "call"),
            ("fields", "field"),
            ("property", "field"),
            ("properties", "field"),
            ("member", "field"),
            ("members", "field"),
            ("imports", "import"),
            ("imported", "import"),
            ("module", "import"),
            ("modules", "import"),
            ("exports", "export"),
            ("exported", "export"),
        ];
        if let Some(exp) = explicit {
            let key = exp.trim().to_lowercase();
            let normalized = ALIASES
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| v.to_string())
                .unwrap_or(key);
            return Some(normalized);
        }
        Self::infer_context_filter(question)
    }

    pub fn detailed_stats(&self) -> Result<String> {
        let nodes = self.store.get_all_nodes()?;
        let edges = self.store.get_all_edges()?;

        let mut file_types: HashMap<&str, usize> = HashMap::new();
        for node in &nodes {
            let ext = node.source_file.rsplit('.').next().unwrap_or("unknown");
            *file_types.entry(ext).or_insert(0) += 1;
        }

        let mut rel_types: HashMap<&str, usize> = HashMap::new();
        for edge in &edges {
            *rel_types.entry(edge.relation.as_str()).or_insert(0) += 1;
        }

        let mut stats = String::new();
        stats.push_str(&format!("Nodes: {}, Edges: {}\n", nodes.len(), edges.len()));
        stats.push_str(&format!("File types: {:?}\n", file_types));
        stats.push_str(&format!("Relation types: {:?}\n", rel_types));
        Ok(stats)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Node;
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

        fn neighbors(&self, id: &str, filter: Option<&str>) -> Result<Vec<(Node, Edge)>> {
            let edges = self.edges.lock().unwrap();
            let nodes = self.nodes.lock().unwrap();
            let mut result = Vec::new();
            for edge in edges.iter() {
                if let Some(f) = filter {
                    if edge.relation != f {
                        continue;
                    }
                }
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

        fn shortest_path(&self, src: &str, tgt: &str) -> Result<Option<Vec<Node>>> {
            if src == tgt {
                return Ok(self
                    .nodes
                    .lock()
                    .unwrap()
                    .get(src)
                    .cloned()
                    .map(|n| vec![n]));
            }

            let nodes = self.nodes.lock().unwrap();
            let edges = self.edges.lock().unwrap();

            let mut visited = HashSet::new();
            let mut queue = VecDeque::new();
            let mut parent: HashMap<String, String> = HashMap::new();

            visited.insert(src.to_string());
            queue.push_back(src.to_string());

            while let Some(current) = queue.pop_front() {
                if current == tgt {
                    let mut path = Vec::new();
                    let mut cur = tgt.to_string();
                    while cur != src {
                        if let Some(n) = nodes.get(&cur) {
                            path.push(n.clone());
                        }
                        cur = parent.get(&cur).cloned().unwrap_or_default();
                    }
                    if let Some(n) = nodes.get(src) {
                        path.push(n.clone());
                    }
                    path.reverse();
                    return Ok(Some(path));
                }

                for edge in edges.iter() {
                    let neighbor = if edge.source == current {
                        Some(edge.target.clone())
                    } else if edge.target == current {
                        Some(edge.source.clone())
                    } else {
                        None
                    };
                    if let Some(neighbor) = neighbor {
                        if visited.insert(neighbor.clone()) {
                            parent.insert(neighbor.clone(), current.clone());
                            queue.push_back(neighbor);
                        }
                    }
                }
            }

            Ok(None)
        }

        fn subgraph(&self, _node_ids: &[&str]) -> Result<(Vec<Node>, Vec<Edge>)> {
            Ok((vec![], vec![]))
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

    fn make_node(id: &str, label: &str) -> Node {
        Node {
            id: id.to_string(),
            label: label.to_string(),
            file_type: "code".to_string(),
            source_file: "test.py".to_string(),
            source_location: None,
            community: None,
            rationale: None,
            docstring: None,
            metadata: HashMap::new(),
        }
    }

    fn make_edge(src: &str, tgt: &str, relation: &str, weight: f64) -> Edge {
        Edge {
            source: src.to_string(),
            target: tgt.to_string(),
            relation: relation.to_string(),
            confidence: "EXTRACTED".to_string(),
            source_file: Some("test.py".to_string()),
            weight,
            context: None,
        }
    }

    #[test]
    fn test_get_node_by_id() {
        let store = MockStore::new();
        store.add_node(make_node("auth", "AuthService")).unwrap();
        let engine = QueryEngine::new(Box::new(store));
        let node = engine.get_node_by_id("auth").unwrap();
        assert!(node.is_some());
        assert_eq!(node.unwrap().label, "AuthService");
    }

    #[test]
    fn test_get_node_not_found() {
        let store = MockStore::new();
        let engine = QueryEngine::new(Box::new(store));
        let node = engine.get_node_by_id("nonexistent").unwrap();
        assert!(node.is_none());
    }

    #[test]
    fn test_graph_stats() {
        let store = MockStore::new();
        store.add_node(make_node("a", "A")).unwrap();
        store.add_node(make_node("b", "B")).unwrap();
        let engine = QueryEngine::new(Box::new(store));
        let stats = engine.graph_stats().unwrap();
        assert!(stats.contains("Nodes: 2"));
    }

    #[test]
    fn test_detailed_stats() {
        let store = MockStore::new();
        store.add_node(make_node("a", "A")).unwrap();
        store.add_node(make_node("b", "B")).unwrap();
        store
            .add_edge(make_edge("a", "b", "connects", 1.0))
            .unwrap();
        let engine = QueryEngine::new(Box::new(store));
        let stats = engine.detailed_stats().unwrap();
        assert!(stats.contains("Nodes: 2"));
        assert!(stats.contains("Edges: 1"));
    }

    #[test]
    fn test_bfs_traversal() {
        let store = MockStore::new();
        store.add_node(make_node("a", "A")).unwrap();
        store.add_node(make_node("b", "B")).unwrap();
        store.add_node(make_node("c", "C")).unwrap();
        store
            .add_edge(make_edge("a", "b", "connects", 1.0))
            .unwrap();
        store
            .add_edge(make_edge("b", "c", "connects", 1.0))
            .unwrap();

        let engine = QueryEngine::new(Box::new(store));
        let result = engine.query_text("a", "bfs", 2, None, None).unwrap();
        assert_eq!(result.seed_nodes.len(), 1);
        assert_eq!(result.seed_nodes[0].id, "a");
        assert!(result.neighborhood.len() >= 2);
    }

    #[test]
    fn test_dfs_traversal() {
        let store = MockStore::new();
        store.add_node(make_node("a", "A")).unwrap();
        store.add_node(make_node("b", "B")).unwrap();
        store.add_node(make_node("c", "C")).unwrap();
        store
            .add_edge(make_edge("a", "b", "connects", 1.0))
            .unwrap();
        store
            .add_edge(make_edge("b", "c", "connects", 1.0))
            .unwrap();

        let engine = QueryEngine::new(Box::new(store));
        let result = engine.query_text("a", "dfs", 2, None, None).unwrap();
        assert_eq!(result.seed_nodes.len(), 1);
        assert!(result.neighborhood.len() >= 2);
    }

    #[test]
    fn test_dfs_method() {
        let store = MockStore::new();
        store.add_node(make_node("a", "A")).unwrap();
        store.add_node(make_node("b", "B")).unwrap();
        store.add_node(make_node("c", "C")).unwrap();
        store
            .add_edge(make_edge("a", "b", "connects", 1.0))
            .unwrap();
        store
            .add_edge(make_edge("b", "c", "connects", 1.0))
            .unwrap();

        let engine = QueryEngine::new(Box::new(store));
        let nodes = engine.dfs("a", 3, None).unwrap();
        assert_eq!(nodes.len(), 3);
    }

    #[test]
    fn test_dijkstra_path() {
        let store = MockStore::new();
        store.add_node(make_node("a", "A")).unwrap();
        store.add_node(make_node("b", "B")).unwrap();
        store.add_node(make_node("c", "C")).unwrap();
        store
            .add_edge(make_edge("a", "b", "connects", 1.5))
            .unwrap();
        store
            .add_edge(make_edge("b", "c", "connects", 2.5))
            .unwrap();

        let engine = QueryEngine::new(Box::new(store));
        let result = engine.dijkstra_path("a", "c").unwrap();
        assert!(result.is_some());
        let (path, weight) = result.unwrap();
        assert_eq!(path.len(), 3);
        assert!((weight - 4.0).abs() < 0.01);
    }

    #[test]
    fn test_dijkstra_path_same_node() {
        let store = MockStore::new();
        store.add_node(make_node("a", "A")).unwrap();
        let engine = QueryEngine::new(Box::new(store));
        let result = engine.dijkstra_path("a", "a").unwrap();
        assert!(result.is_some());
        let (path, weight) = result.unwrap();
        assert_eq!(path.len(), 1);
        assert!((weight - 0.0).abs() < 0.01);
    }

    #[test]
    fn test_dijkstra_path_no_path() {
        let store = MockStore::new();
        store.add_node(make_node("a", "A")).unwrap();
        store.add_node(make_node("b", "B")).unwrap();
        let engine = QueryEngine::new(Box::new(store));
        let result = engine.dijkstra_path("a", "b").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_context_filter() {
        let store = MockStore::new();
        store.add_node(make_node("a", "A")).unwrap();
        store.add_node(make_node("b", "B")).unwrap();
        store.add_node(make_node("c", "C")).unwrap();
        store.add_edge(make_edge("a", "b", "imports", 1.0)).unwrap();
        store.add_edge(make_edge("a", "c", "calls", 1.0)).unwrap();

        let engine = QueryEngine::new(Box::new(store));
        let neighbors = engine.get_neighbors("a", Some("imports")).unwrap();
        assert_eq!(neighbors.len(), 1);
        assert_eq!(neighbors[0].0.label, "B");
    }

    #[test]
    fn test_token_budget() {
        let store = MockStore::new();
        store.add_node(make_node("a", "A")).unwrap();
        let engine = QueryEngine::new(Box::new(store));
        let result = engine.query_text("a", "bfs", 1, Some(5), None).unwrap();
        assert_eq!(result.seed_nodes.len(), 1);
    }

    // --- Gap test #102: exact match ---

    #[test]
    fn test_query_exact_match() {
        let store = MockStore::new();
        store.add_node(make_node("a1", "AuthService")).unwrap();
        store.add_node(make_node("u1", "UserService")).unwrap();
        let engine = QueryEngine::new(Box::new(store));
        let result = engine
            .query_text("AuthService", "bfs", 0, None, None)
            .unwrap();
        assert_eq!(result.seed_nodes.len(), 1);
        assert_eq!(result.seed_nodes[0].label, "AuthService");
    }

    // --- Gap test #103: prefix match ---

    #[test]
    fn test_query_prefix_match() {
        let store = MockStore::new();
        store.add_node(make_node("a1", "AuthService")).unwrap();
        store.add_node(make_node("a2", "AuthMiddleware")).unwrap();
        store.add_node(make_node("u1", "UserService")).unwrap();
        let engine = QueryEngine::new(Box::new(store));
        let result = engine.query_text("Auth", "bfs", 0, None, None).unwrap();
        assert_eq!(result.seed_nodes.len(), 2);
        for n in &result.seed_nodes {
            assert!(n.label.starts_with("Auth"));
        }
    }

    // --- Gap test #104: substring match ---

    #[test]
    fn test_query_substring_match() {
        let store = MockStore::new();
        store.add_node(make_node("a1", "AuthService")).unwrap();
        store.add_node(make_node("u1", "UserService")).unwrap();
        store.add_node(make_node("r1", "RateLimiter")).unwrap();
        let engine = QueryEngine::new(Box::new(store));
        let result = engine.query_text("Service", "bfs", 0, None, None).unwrap();
        assert_eq!(result.seed_nodes.len(), 2);
        for n in &result.seed_nodes {
            assert!(n.label.contains("Service"));
        }
    }

    // --- Gap test #105-106: BFS depth ---

    #[test]
    fn test_query_bfs_depth_1() {
        let store = MockStore::new();
        store.add_node(make_node("a", "A")).unwrap();
        store.add_node(make_node("b", "B")).unwrap();
        store.add_node(make_node("c", "C")).unwrap();
        store
            .add_edge(make_edge("a", "b", "connects", 1.0))
            .unwrap();
        store
            .add_edge(make_edge("b", "c", "connects", 1.0))
            .unwrap();
        let engine = QueryEngine::new(Box::new(store));
        let result = engine.query_text("a", "bfs", 1, None, None).unwrap();
        assert_eq!(
            result.neighborhood.len(),
            2,
            "depth 1: seed + immediate neighbor"
        );
        assert!(result.neighborhood.iter().any(|n| n.id == "b"));
        assert!(
            !result.neighborhood.iter().any(|n| n.id == "c"),
            "C should not be reachable at depth 1"
        );
    }

    #[test]
    fn test_query_bfs_depth_3() {
        let store = MockStore::new();
        store.add_node(make_node("a", "A")).unwrap();
        store.add_node(make_node("b", "B")).unwrap();
        store.add_node(make_node("c", "C")).unwrap();
        store
            .add_edge(make_edge("a", "b", "connects", 1.0))
            .unwrap();
        store
            .add_edge(make_edge("b", "c", "connects", 1.0))
            .unwrap();
        let engine = QueryEngine::new(Box::new(store));
        let result = engine.query_text("a", "bfs", 3, None, None).unwrap();
        assert_eq!(result.neighborhood.len(), 3, "depth 3: all nodes reachable");
        assert!(
            result.neighborhood.iter().any(|n| n.id == "c"),
            "C should be reachable at depth 3"
        );
    }

    // --- Gap test #108: context filter in query ---

    #[test]
    fn test_query_context_filter_call() {
        let store = MockStore::new();
        store.add_node(make_node("a", "A")).unwrap();
        store.add_node(make_node("b", "B")).unwrap();
        store.add_node(make_node("c", "C")).unwrap();
        store.add_edge(make_edge("a", "b", "imports", 1.0)).unwrap();
        store.add_edge(make_edge("a", "c", "calls", 1.0)).unwrap();
        let engine = QueryEngine::new(Box::new(store));
        let result = engine
            .query_text("a", "bfs", 1, None, Some("calls"))
            .unwrap();
        assert_eq!(result.seed_nodes.len(), 1);
        let neighbor_ids: Vec<&str> = result.neighborhood.iter().map(|n| n.id.as_str()).collect();
        assert!(
            neighbor_ids.contains(&"c"),
            "call target should be in neighborhood"
        );
        assert!(
            !neighbor_ids.contains(&"b"),
            "import target should be filtered out"
        );
    }

    // --- Gap test #109: context filter inferred from question ---

    #[test]
    fn test_query_context_filter_inferred() {
        assert_eq!(
            QueryEngine::infer_context_filter("who calls AuthService"),
            Some("call".to_string())
        );
        assert_eq!(
            QueryEngine::infer_context_filter("what imports UserModule"),
            Some("import".to_string())
        );
        assert_eq!(
            QueryEngine::infer_context_filter("list fields of Foo"),
            Some("field".to_string())
        );
        assert_eq!(
            QueryEngine::infer_context_filter("show me AuthService"),
            None
        );
    }

    // --- Gap test #110: explicit context filter overrides inferred ---

    #[test]
    fn test_query_context_filter_explicit() {
        assert_eq!(
            QueryEngine::resolve_context_filter("who calls AuthService", Some("imports")),
            Some("import".to_string())
        );
        assert_eq!(
            QueryEngine::resolve_context_filter("who calls AuthService", None),
            Some("call".to_string())
        );
        assert_eq!(
            QueryEngine::resolve_context_filter("show me AuthService", None),
            None
        );
    }

    // --- Gap test #112: get_node_by_label ---

    #[test]
    fn test_get_node_by_label() {
        let store = MockStore::new();
        store.add_node(make_node("a1", "AuthService")).unwrap();
        let engine = QueryEngine::new(Box::new(store));
        let node = engine.get_node_by_label("AuthService").unwrap();
        assert!(node.is_some());
        assert_eq!(node.unwrap().label, "AuthService");
    }

    // --- Gap test #115: get_neighbors_basic ---

    #[test]
    fn test_get_neighbors_basic() {
        let store = MockStore::new();
        store.add_node(make_node("a", "A")).unwrap();
        store.add_node(make_node("b", "B")).unwrap();
        store.add_node(make_node("c", "C")).unwrap();
        store
            .add_edge(make_edge("a", "b", "connects", 1.0))
            .unwrap();
        store.add_edge(make_edge("a", "c", "calls", 1.0)).unwrap();
        let engine = QueryEngine::new(Box::new(store));
        let neighbors = engine.get_neighbors("a", None).unwrap();
        assert_eq!(neighbors.len(), 2);
    }

    // --- Gap test #117: get_community ---

    #[test]
    fn test_get_community() {
        let store = MockStore::new();
        let mut n1 = make_node("a", "A");
        n1.community = Some(1);
        let mut n2 = make_node("b", "B");
        n2.community = Some(1);
        let n3 = make_node("c", "C");
        store.add_node(n1).unwrap();
        store.add_node(n2).unwrap();
        store.add_node(n3).unwrap();
        let engine = QueryEngine::new(Box::new(store));
        let members = engine.get_community(1).unwrap();
        assert_eq!(members.len(), 2);
        for n in &members {
            assert_eq!(n.community, Some(1));
        }
    }

    // --- Gap test #118: god_nodes ---

    #[test]
    fn test_god_nodes() {
        let store = MockStore::new();
        store.add_node(make_node("a", "A")).unwrap();
        store.add_node(make_node("b", "B")).unwrap();
        store.add_node(make_node("c", "C")).unwrap();
        store
            .add_edge(make_edge("a", "b", "connects", 1.0))
            .unwrap();
        store
            .add_edge(make_edge("a", "c", "connects", 1.0))
            .unwrap();
        store
            .add_edge(make_edge("b", "c", "connects", 1.0))
            .unwrap();
        let engine = QueryEngine::new(Box::new(store));
        let gods = engine.god_nodes(2).unwrap();
        assert_eq!(gods.len(), 2);
    }

    // --- Gap test #120: shortest_path_found ---

    #[test]
    fn test_shortest_path_found() {
        let store = MockStore::new();
        store.add_node(make_node("a", "A")).unwrap();
        store.add_node(make_node("b", "B")).unwrap();
        store.add_node(make_node("c", "C")).unwrap();
        store
            .add_edge(make_edge("a", "b", "connects", 1.0))
            .unwrap();
        store
            .add_edge(make_edge("b", "c", "connects", 1.0))
            .unwrap();
        let engine = QueryEngine::new(Box::new(store));
        let path = engine.shortest_path("a", "c").unwrap();
        assert!(path.is_some());
        assert_eq!(path.unwrap().len(), 3);
    }

    // --- Gap test #123: shortest_path_max_hops ---

    #[test]
    fn test_shortest_path_max_hops() {
        let store = MockStore::new();
        store.add_node(make_node("a", "A")).unwrap();
        store.add_node(make_node("b", "B")).unwrap();
        store.add_node(make_node("c", "C")).unwrap();
        store.add_node(make_node("d", "D")).unwrap();
        store
            .add_edge(make_edge("a", "b", "connects", 1.0))
            .unwrap();
        store
            .add_edge(make_edge("b", "c", "connects", 1.0))
            .unwrap();
        store
            .add_edge(make_edge("c", "d", "connects", 1.0))
            .unwrap();

        let engine = QueryEngine::new(Box::new(store));

        // Path a-d requires 3 hops, max_hops=2 → not found
        let limited = engine.shortest_path_with_max_hops("a", "d", 2).unwrap();
        assert!(limited.is_none(), "path exceeds max_hops");

        // Path a-d with max_hops=3 → found
        let found = engine.shortest_path_with_max_hops("a", "d", 3).unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().len(), 4);
    }

    #[test]
    fn test_resolve_node_by_label_when_id_missing() {
        let store = MockStore::new();
        store
            .add_node(make_node("api_authservice", "AuthService"))
            .unwrap();
        let engine = QueryEngine::new(Box::new(store));

        // Exact id lookup fails — should fall back to label search
        let node = engine.resolve_node("AuthService").unwrap();
        assert!(node.is_some(), "resolve_node must find node by label");
        assert_eq!(node.unwrap().id, "api_authservice");
    }

    #[test]
    fn test_resolve_node_by_exact_id() {
        let store = MockStore::new();
        store
            .add_node(make_node("api_authservice", "AuthService"))
            .unwrap();
        let engine = QueryEngine::new(Box::new(store));

        let node = engine.resolve_node("api_authservice").unwrap();
        assert!(node.is_some());
        assert_eq!(node.unwrap().id, "api_authservice");
    }

    #[test]
    fn test_resolve_node_not_found() {
        let store = MockStore::new();
        let engine = QueryEngine::new(Box::new(store));
        let node = engine.resolve_node("ghost").unwrap();
        assert!(node.is_none());
    }
}
