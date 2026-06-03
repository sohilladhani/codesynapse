use std::collections::{HashMap, VecDeque};

#[derive(Clone, Debug, PartialEq)]
pub struct GqlNode {
    pub id: String,
    pub label: String,
    pub source_file: String,
    pub source_location: Option<String>,
    pub community: Option<i64>,
    pub file_type: String,
    pub rationale: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GqlEdge {
    pub source: String,
    pub target: String,
    pub relation: String,
    pub confidence: String,
    pub source_file: Option<String>,
    pub weight: f64,
    pub context: Option<String>,
}

#[derive(Default)]
pub struct GraphState {
    nodes: HashMap<String, GqlNode>,
    edges: Vec<GqlEdge>,
    out_adj: HashMap<String, Vec<String>>,
}

impl GraphState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_node(&mut self, node: GqlNode) {
        self.out_adj.entry(node.id.clone()).or_default();
        self.nodes.insert(node.id.clone(), node);
    }

    pub fn add_edge(&mut self, edge: GqlEdge) {
        self.out_adj
            .entry(edge.source.clone())
            .or_default()
            .push(edge.target.clone());
        self.out_adj.entry(edge.target.clone()).or_default();
        self.edges.push(edge);
    }

    pub fn reset(&mut self) {
        self.nodes.clear();
        self.edges.clear();
        self.out_adj.clear();
    }

    pub fn get_node(&self, id: &str) -> Option<&GqlNode> {
        self.nodes.get(id)
    }

    pub fn get_nodes(&self) -> Vec<GqlNode> {
        self.nodes.values().cloned().collect()
    }

    pub fn get_edges(&self) -> Vec<GqlEdge> {
        self.edges.clone()
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    pub fn community_count(&self) -> usize {
        let mut seen = std::collections::HashSet::new();
        for n in self.nodes.values() {
            if let Some(c) = n.community {
                seen.insert(c);
            }
        }
        seen.len()
    }

    pub fn search_nodes(&self, query: &str, limit: usize) -> Vec<GqlNode> {
        let q = query.to_lowercase();
        self.nodes
            .values()
            .filter(|n| n.label.to_lowercase().contains(&q) || n.id.to_lowercase().contains(&q))
            .take(limit)
            .cloned()
            .collect()
    }

    pub fn neighbors(&self, start: &str, depth: usize) -> Vec<GqlNode> {
        let mut visited = std::collections::HashSet::new();
        let mut queue = VecDeque::new();
        queue.push_back((start.to_string(), 0usize));
        visited.insert(start.to_string());
        let mut result = Vec::new();

        while let Some((cur, d)) = queue.pop_front() {
            if d >= depth {
                continue;
            }
            if let Some(neighbors) = self.out_adj.get(&cur) {
                for nid in neighbors {
                    if !visited.contains(nid) {
                        visited.insert(nid.clone());
                        if let Some(n) = self.nodes.get(nid) {
                            result.push(n.clone());
                        }
                        queue.push_back((nid.clone(), d + 1));
                    }
                }
            }
        }
        result
    }

    pub fn community_nodes(&self, id: i64) -> Vec<GqlNode> {
        self.nodes
            .values()
            .filter(|n| n.community == Some(id))
            .cloned()
            .collect()
    }

    pub fn shortest_path(&self, source: &str, target: &str) -> Option<Vec<String>> {
        if source == target {
            return if self.nodes.contains_key(source) {
                Some(vec![source.to_string()])
            } else {
                None
            };
        }
        let mut visited: HashMap<String, Option<String>> = HashMap::new();
        let mut queue = VecDeque::new();
        queue.push_back(source.to_string());
        visited.insert(source.to_string(), None);

        while let Some(cur) = queue.pop_front() {
            if cur == target {
                let mut path = vec![target.to_string()];
                let mut node = target.to_string();
                while let Some(Some(prev)) = visited.get(&node) {
                    path.push(prev.clone());
                    node = prev.clone();
                }
                path.reverse();
                return Some(path);
            }
            if let Some(nbrs) = self.out_adj.get(&cur) {
                for next in nbrs {
                    if !visited.contains_key(next) {
                        visited.insert(next.clone(), Some(cur.clone()));
                        queue.push_back(next.clone());
                    }
                }
            }
        }
        None
    }

    pub fn god_nodes(&self, top_n: usize) -> Vec<GqlNode> {
        let mut degree: HashMap<String, usize> = HashMap::new();
        for e in &self.edges {
            *degree.entry(e.source.clone()).or_insert(0) += 1;
            *degree.entry(e.target.clone()).or_insert(0) += 1;
        }
        let mut ranked: Vec<GqlNode> = self.nodes.values().cloned().collect();
        ranked.sort_by(|a, b| {
            let da = degree.get(&a.id).copied().unwrap_or(0);
            let db = degree.get(&b.id).copied().unwrap_or(0);
            db.cmp(&da).then(a.id.cmp(&b.id))
        });
        ranked.into_iter().take(top_n).collect()
    }
}
