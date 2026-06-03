use crate::error::{CodeSynapseError, Result};
use crate::types::{Edge, Node, NodeId};
use std::collections::HashMap;
use std::path::Path;
use std::sync::RwLock;

pub trait GraphStore: Send + Sync {
    fn add_node(&self, node: Node) -> Result<()>;
    fn add_edge(&self, edge: Edge) -> Result<()>;
    fn get_node(&self, id: &str) -> Result<Option<Node>>;
    fn get_all_nodes(&self) -> Result<Vec<Node>>;
    fn get_all_edges(&self) -> Result<Vec<Edge>>;
    fn neighbors(&self, id: &str, relation_filter: Option<&str>) -> Result<Vec<(Node, Edge)>>;
    fn search(&self, query: &str, top_k: usize) -> Result<Vec<(f64, Node)>>;
    fn shortest_path(&self, src: &str, tgt: &str) -> Result<Option<Vec<Node>>>;
    fn dijkstra_shortest_path(&self, src: &str, tgt: &str) -> Result<Option<Vec<Node>>>;
    fn subgraph(&self, node_ids: &[&str]) -> Result<(Vec<Node>, Vec<Edge>)>;
    fn node_count(&self) -> Result<usize>;
    fn edge_count(&self) -> Result<usize>;
    fn remove_node(&self, id: &str) -> Result<()>;
    fn remove_edge(&self, source: &str, target: &str, relation: &str) -> Result<()>;
    fn clear(&self) -> Result<()>;
}

pub enum StoreBackend {
    Sled(SledGraphStore),
    Memory(MemoryGraphStore),
}

impl GraphStore for StoreBackend {
    fn add_node(&self, node: Node) -> Result<()> {
        match self {
            StoreBackend::Sled(s) => s.add_node(node),
            StoreBackend::Memory(m) => m.add_node(node),
        }
    }

    fn add_edge(&self, edge: Edge) -> Result<()> {
        match self {
            StoreBackend::Sled(s) => s.add_edge(edge),
            StoreBackend::Memory(m) => m.add_edge(edge),
        }
    }

    fn get_node(&self, id: &str) -> Result<Option<Node>> {
        match self {
            StoreBackend::Sled(s) => s.get_node(id),
            StoreBackend::Memory(m) => m.get_node(id),
        }
    }

    fn get_all_nodes(&self) -> Result<Vec<Node>> {
        match self {
            StoreBackend::Sled(s) => s.get_all_nodes(),
            StoreBackend::Memory(m) => m.get_all_nodes(),
        }
    }

    fn get_all_edges(&self) -> Result<Vec<Edge>> {
        match self {
            StoreBackend::Sled(s) => s.get_all_edges(),
            StoreBackend::Memory(m) => m.get_all_edges(),
        }
    }

    fn neighbors(&self, id: &str, relation_filter: Option<&str>) -> Result<Vec<(Node, Edge)>> {
        match self {
            StoreBackend::Sled(s) => s.neighbors(id, relation_filter),
            StoreBackend::Memory(m) => m.neighbors(id, relation_filter),
        }
    }

    fn search(&self, query: &str, top_k: usize) -> Result<Vec<(f64, Node)>> {
        match self {
            StoreBackend::Sled(s) => s.search(query, top_k),
            StoreBackend::Memory(m) => m.search(query, top_k),
        }
    }

    fn shortest_path(&self, src: &str, tgt: &str) -> Result<Option<Vec<Node>>> {
        match self {
            StoreBackend::Sled(s) => s.shortest_path(src, tgt),
            StoreBackend::Memory(m) => m.shortest_path(src, tgt),
        }
    }

    fn dijkstra_shortest_path(&self, src: &str, tgt: &str) -> Result<Option<Vec<Node>>> {
        match self {
            StoreBackend::Sled(s) => s.dijkstra_shortest_path(src, tgt),
            StoreBackend::Memory(m) => m.dijkstra_shortest_path(src, tgt),
        }
    }

    fn subgraph(&self, node_ids: &[&str]) -> Result<(Vec<Node>, Vec<Edge>)> {
        match self {
            StoreBackend::Sled(s) => s.subgraph(node_ids),
            StoreBackend::Memory(m) => m.subgraph(node_ids),
        }
    }

    fn node_count(&self) -> Result<usize> {
        match self {
            StoreBackend::Sled(s) => s.node_count(),
            StoreBackend::Memory(m) => m.node_count(),
        }
    }

    fn edge_count(&self) -> Result<usize> {
        match self {
            StoreBackend::Sled(s) => s.edge_count(),
            StoreBackend::Memory(m) => m.edge_count(),
        }
    }

    fn remove_node(&self, id: &str) -> Result<()> {
        match self {
            StoreBackend::Sled(s) => s.remove_node(id),
            StoreBackend::Memory(m) => m.remove_node(id),
        }
    }

    fn remove_edge(&self, source: &str, target: &str, relation: &str) -> Result<()> {
        match self {
            StoreBackend::Sled(s) => s.remove_edge(source, target, relation),
            StoreBackend::Memory(m) => m.remove_edge(source, target, relation),
        }
    }

    fn clear(&self) -> Result<()> {
        match self {
            StoreBackend::Sled(s) => s.clear(),
            StoreBackend::Memory(m) => m.clear(),
        }
    }
}

pub struct SledGraphStore {
    db: sled::Db,
    inverted_index: RwLock<HashMap<String, Vec<String>>>,
}

impl SledGraphStore {
    pub fn open(path: &Path) -> Result<Self> {
        let db = sled::open(path).map_err(|e| CodeSynapseError::Database(e.to_string()))?;
        let store = SledGraphStore {
            db,
            inverted_index: RwLock::new(HashMap::new()),
        };
        store.rebuild_index()?;
        Ok(store)
    }

    pub fn temporary() -> Result<Self> {
        let dir = std::env::temp_dir().join(format!("codesynapse-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).ok();
        Self::open(&dir)
    }

    fn node_key(id: &str) -> Vec<u8> {
        format!("node:{}", id).into_bytes()
    }

    fn edge_key(id: &str) -> Vec<u8> {
        format!("edge:{}", id).into_bytes()
    }

    fn adjacency_key(src: &str, target: &str) -> Vec<u8> {
        format!("adj:{}:{}", src, target).into_bytes()
    }

    fn rebuild_index(&self) -> Result<()> {
        let mut index = self.inverted_index.write().unwrap();
        index.clear();
        for result in self.db.iter() {
            let (key, value) = result.map_err(|e| CodeSynapseError::Database(e.to_string()))?;
            let key_str = String::from_utf8_lossy(&key);
            if key_str.starts_with("node:") {
                let node: Node = bincode::deserialize(&value).map_err(CodeSynapseError::Bincode)?;
                for token in tokenize(&node.label) {
                    index.entry(token).or_default().push(node.id.clone());
                }
                for token in tokenize(&node.id) {
                    index.entry(token).or_default().push(node.id.clone());
                }
            }
        }
        Ok(())
    }
}

impl GraphStore for SledGraphStore {
    fn add_node(&self, node: Node) -> Result<()> {
        let key = Self::node_key(&node.id);
        let value = bincode::serialize(&node).map_err(CodeSynapseError::Bincode)?;
        self.db
            .insert(key, value)
            .map_err(|e| CodeSynapseError::Database(e.to_string()))?;

        // Update inverted index
        let mut index = self.inverted_index.write().unwrap();
        for token in tokenize(&node.label) {
            index.entry(token).or_default().push(node.id.clone());
        }
        for token in tokenize(&node.id) {
            index.entry(token).or_default().push(node.id.clone());
        }

        self.db
            .flush()
            .map_err(|e| CodeSynapseError::Database(e.to_string()))?;
        Ok(())
    }

    fn add_edge(&self, edge: Edge) -> Result<()> {
        let edge_id = format!("{}->{}:{}", edge.source, edge.target, edge.relation);
        let key = Self::edge_key(&edge_id);
        let value = bincode::serialize(&edge).map_err(CodeSynapseError::Bincode)?;
        self.db
            .insert(key, value)
            .map_err(|e| CodeSynapseError::Database(e.to_string()))?;

        // Adjacency index
        let adj_key = Self::adjacency_key(&edge.source, &edge.target);
        let adj_bytes = bincode::serialize(&edge).map_err(CodeSynapseError::Bincode)?;
        self.db
            .insert(adj_key, adj_bytes)
            .map_err(|e| CodeSynapseError::Database(e.to_string()))?;

        Ok(())
    }

    fn get_node(&self, id: &str) -> Result<Option<Node>> {
        let key = Self::node_key(id);
        match self.db.get(key) {
            Ok(Some(value)) => {
                let node: Node = bincode::deserialize(&value).map_err(CodeSynapseError::Bincode)?;
                Ok(Some(node))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(CodeSynapseError::Database(e.to_string())),
        }
    }

    fn get_all_nodes(&self) -> Result<Vec<Node>> {
        let mut nodes = Vec::new();
        for result in self.db.iter() {
            let (key, value) = result.map_err(|e| CodeSynapseError::Database(e.to_string()))?;
            let key_str = String::from_utf8_lossy(&key);
            if key_str.starts_with("node:") {
                let node: Node = bincode::deserialize(&value).map_err(CodeSynapseError::Bincode)?;
                nodes.push(node);
            }
        }
        Ok(nodes)
    }

    fn get_all_edges(&self) -> Result<Vec<Edge>> {
        let mut edges = Vec::new();
        for result in self.db.iter() {
            let (key, value) = result.map_err(|e| CodeSynapseError::Database(e.to_string()))?;
            let key_str = String::from_utf8_lossy(&key);
            if key_str.starts_with("edge:") {
                let edge: Edge = bincode::deserialize(&value).map_err(CodeSynapseError::Bincode)?;
                edges.push(edge);
            }
        }
        Ok(edges)
    }

    fn neighbors(&self, id: &str, _relation_filter: Option<&str>) -> Result<Vec<(Node, Edge)>> {
        let mut result = Vec::new();
        // Scan adjacency keys starting with adj:{id}:
        let prefix = format!("adj:{}:", id);
        for entry in self.db.scan_prefix(prefix.as_bytes()) {
            let (_key, value) = entry.map_err(|e| CodeSynapseError::Database(e.to_string()))?;
            let edge: Edge = bincode::deserialize(&value).map_err(CodeSynapseError::Bincode)?;
            let neighbor_id = if edge.source == id {
                &edge.target
            } else {
                &edge.source
            };
            if let Some(node) = self.get_node(neighbor_id)? {
                result.push((node, edge));
            }
        }
        Ok(result)
    }

    fn search(&self, query: &str, top_k: usize) -> Result<Vec<(f64, Node)>> {
        let tokens = tokenize(query);
        if tokens.is_empty() {
            return Ok(vec![]);
        }

        let index = self.inverted_index.read().unwrap();
        let mut scores: HashMap<String, f64> = HashMap::new();

        // Use inverted index if available, fall back to full scan
        if !index.is_empty() {
            for token in &tokens {
                if let Some(ids) = index.get(token) {
                    for id in ids {
                        *scores.entry(id.clone()).or_insert(0.0) += 1.0;
                    }
                }
            }
        } else {
            // Full scan fallback
            for result in self.db.iter() {
                let (_key, value) =
                    result.map_err(|e| CodeSynapseError::Database(e.to_string()))?;
                let key_str = String::from_utf8_lossy(&_key);
                if key_str.starts_with("node:") {
                    let node: Node =
                        bincode::deserialize(&value).map_err(CodeSynapseError::Bincode)?;
                    let mut score = 0.0;
                    for token in &tokens {
                        if node.label.to_lowercase().contains(token) {
                            score += 1.0;
                        }
                        if node.id.to_lowercase().contains(token) {
                            score += 0.5;
                        }
                    }
                    if score > 0.0 {
                        scores.insert(node.id.clone(), score);
                    }
                }
            }
        }

        let mut ranked: Vec<(f64, Node)> = scores
            .into_iter()
            .filter_map(|(id, score)| self.get_node(&id).ok().flatten().map(|n| (score, n)))
            .collect();

        ranked.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        ranked.truncate(top_k);
        Ok(ranked)
    }

    fn shortest_path(&self, src: &str, tgt: &str) -> Result<Option<Vec<Node>>> {
        if src == tgt {
            return self.get_node(src).map(|n| n.map(|n| vec![n]));
        }

        let mut visited = std::collections::HashSet::new();
        let mut queue = std::collections::VecDeque::new();
        let mut parent: HashMap<String, String> = HashMap::new();

        visited.insert(src.to_string());
        queue.push_back(src.to_string());

        while let Some(current) = queue.pop_front() {
            if current == tgt {
                let mut path = Vec::new();
                let mut node_id = tgt.to_string();
                while node_id != src {
                    if let Some(n) = self.get_node(&node_id)? {
                        path.push(n);
                    }
                    node_id = parent.get(&node_id).cloned().unwrap_or_default();
                }
                if let Some(n) = self.get_node(src)? {
                    path.push(n);
                }
                path.reverse();
                return Ok(Some(path));
            }

            if let Ok(neighbors) = self.neighbors(&current, None) {
                for (neighbor, _) in neighbors {
                    if visited.insert(neighbor.id.clone()) {
                        parent.insert(neighbor.id.clone(), current.clone());
                        queue.push_back(neighbor.id.clone());
                    }
                }
            }
        }

        Ok(None)
    }

    fn dijkstra_shortest_path(&self, src: &str, tgt: &str) -> Result<Option<Vec<Node>>> {
        use std::collections::BinaryHeap;

        if src == tgt {
            return self.get_node(src).map(|n| n.map(|n| vec![n]));
        }

        let mut distances: HashMap<String, f64> = HashMap::new();
        let mut parents: HashMap<String, String> = HashMap::new();
        let mut heap = BinaryHeap::new();

        distances.insert(src.to_string(), 0.0);

        #[derive(PartialEq)]
        struct State(String, f64);
        impl Eq for State {}
        impl PartialOrd for State {
            fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
                Some(self.cmp(other))
            }
        }
        impl Ord for State {
            fn cmp(&self, other: &Self) -> std::cmp::Ordering {
                other
                    .1
                    .partial_cmp(&self.1)
                    .unwrap_or(std::cmp::Ordering::Equal)
            }
        }

        heap.push(State(src.to_string(), 0.0));

        while let Some(State(current, cost)) = heap.pop() {
            if current == tgt {
                let mut path = Vec::new();
                let mut node_id = tgt.to_string();
                while node_id != src {
                    if let Some(n) = self.get_node(&node_id)? {
                        path.push(n);
                    }
                    node_id = parents.get(&node_id).cloned().unwrap_or_default();
                }
                if let Some(n) = self.get_node(src)? {
                    path.push(n);
                }
                path.reverse();
                return Ok(Some(path));
            }

            if let Some(&best) = distances.get(&current) {
                if cost > best {
                    continue;
                }
            }

            if let Ok(neighbors) = self.neighbors(&current, None) {
                for (neighbor, edge) in neighbors {
                    let next_cost = cost + edge.weight;
                    let is_better = distances.get(&neighbor.id).is_none_or(|&d| next_cost < d);
                    if is_better {
                        distances.insert(neighbor.id.clone(), next_cost);
                        parents.insert(neighbor.id.clone(), current.clone());
                        heap.push(State(neighbor.id.clone(), next_cost));
                    }
                }
            }
        }

        Ok(None)
    }

    fn subgraph(&self, node_ids: &[&str]) -> Result<(Vec<Node>, Vec<Edge>)> {
        let mut nodes = Vec::new();
        let mut edges = Vec::new();
        let id_set: std::collections::HashSet<&str> = node_ids.iter().copied().collect();

        for id in node_ids {
            if let Some(node) = self.get_node(id)? {
                nodes.push(node);
            }
        }

        let all_edges = self.get_all_edges()?;
        for edge in all_edges {
            if id_set.contains(edge.source.as_str()) && id_set.contains(edge.target.as_str()) {
                edges.push(edge);
            }
        }

        Ok((nodes, edges))
    }

    fn node_count(&self) -> Result<usize> {
        let mut count = 0;
        for result in self.db.iter() {
            let (key, _) = result.map_err(|e| CodeSynapseError::Database(e.to_string()))?;
            if String::from_utf8_lossy(&key).starts_with("node:") {
                count += 1;
            }
        }
        Ok(count)
    }

    fn edge_count(&self) -> Result<usize> {
        let mut count = 0;
        for result in self.db.iter() {
            let (key, _) = result.map_err(|e| CodeSynapseError::Database(e.to_string()))?;
            if String::from_utf8_lossy(&key).starts_with("edge:") {
                count += 1;
            }
        }
        Ok(count)
    }

    fn remove_node(&self, id: &str) -> Result<()> {
        let key = Self::node_key(id);
        self.db
            .remove(key)
            .map_err(|e| CodeSynapseError::Database(e.to_string()))?;
        Ok(())
    }

    fn remove_edge(&self, source: &str, target: &str, relation: &str) -> Result<()> {
        let edge_id = format!("{}->{}:{}", source, target, relation);
        let key = Self::edge_key(&edge_id);
        self.db
            .remove(key)
            .map_err(|e| CodeSynapseError::Database(e.to_string()))?;
        let adj_key = Self::adjacency_key(source, target);
        self.db
            .remove(adj_key)
            .map_err(|e| CodeSynapseError::Database(e.to_string()))?;
        Ok(())
    }

    fn clear(&self) -> Result<()> {
        self.db
            .clear()
            .map_err(|e| CodeSynapseError::Database(e.to_string()))?;
        self.inverted_index.write().unwrap().clear();
        Ok(())
    }
}

pub struct MemoryGraphStore {
    nodes: RwLock<HashMap<NodeId, Node>>,
    edges: RwLock<Vec<Edge>>,
    inverted_index: RwLock<HashMap<String, Vec<String>>>,
}

impl MemoryGraphStore {
    pub fn new() -> Self {
        MemoryGraphStore {
            nodes: RwLock::new(HashMap::new()),
            edges: RwLock::new(Vec::new()),
            inverted_index: RwLock::new(HashMap::new()),
        }
    }
}

impl Default for MemoryGraphStore {
    fn default() -> Self {
        Self::new()
    }
}

impl GraphStore for MemoryGraphStore {
    fn add_node(&self, node: Node) -> Result<()> {
        let mut nodes = self.nodes.write().unwrap();
        nodes.insert(node.id.clone(), node.clone());

        let mut index = self.inverted_index.write().unwrap();
        for token in tokenize(&node.label) {
            index.entry(token).or_default().push(node.id.clone());
        }
        for token in tokenize(&node.id) {
            index.entry(token).or_default().push(node.id.clone());
        }

        Ok(())
    }

    fn add_edge(&self, edge: Edge) -> Result<()> {
        self.edges.write().unwrap().push(edge);
        Ok(())
    }

    fn get_node(&self, id: &str) -> Result<Option<Node>> {
        Ok(self.nodes.read().unwrap().get(id).cloned())
    }

    fn get_all_nodes(&self) -> Result<Vec<Node>> {
        Ok(self.nodes.read().unwrap().values().cloned().collect())
    }

    fn get_all_edges(&self) -> Result<Vec<Edge>> {
        Ok(self.edges.read().unwrap().clone())
    }

    fn neighbors(&self, id: &str, relation_filter: Option<&str>) -> Result<Vec<(Node, Edge)>> {
        let edges = self.edges.read().unwrap();
        let nodes = self.nodes.read().unwrap();
        let mut result = Vec::new();
        for edge in edges.iter() {
            if let Some(filter) = relation_filter {
                if edge.relation != filter {
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

    fn search(&self, query: &str, top_k: usize) -> Result<Vec<(f64, Node)>> {
        let tokens = tokenize(query);
        if tokens.is_empty() {
            return Ok(vec![]);
        }

        let nodes = self.nodes.read().unwrap();
        let index = self.inverted_index.read().unwrap();

        let mut scores: HashMap<String, f64> = HashMap::new();

        if !index.is_empty() {
            for token in &tokens {
                if let Some(ids) = index.get(token) {
                    for id in ids {
                        *scores.entry(id.clone()).or_insert(0.0) += 1.0;
                    }
                }
            }
        } else {
            for node in nodes.values() {
                let mut score = 0.0;
                for token in &tokens {
                    if node.label.to_lowercase().contains(token) {
                        score += 1.0;
                    }
                    if node.id.to_lowercase().contains(token) {
                        score += 0.5;
                    }
                }
                if score > 0.0 {
                    scores.insert(node.id.clone(), score);
                }
            }
        }

        let mut ranked: Vec<(f64, Node)> = scores
            .into_iter()
            .filter_map(|(id, score)| nodes.get(&id).map(|n| (score, n.clone())))
            .collect();

        ranked.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        ranked.truncate(top_k);
        Ok(ranked)
    }

    fn shortest_path(&self, src: &str, tgt: &str) -> Result<Option<Vec<Node>>> {
        if src == tgt {
            return Ok(self
                .nodes
                .read()
                .unwrap()
                .get(src)
                .cloned()
                .map(|n| vec![n]));
        }

        let nodes = self.nodes.read().unwrap();
        let edges = self.edges.read().unwrap();

        let mut visited = std::collections::HashSet::new();
        let mut queue = std::collections::VecDeque::new();
        let mut parent: HashMap<String, String> = HashMap::new();

        visited.insert(src.to_string());
        queue.push_back(src.to_string());

        while let Some(current) = queue.pop_front() {
            if current == tgt {
                let mut path = Vec::new();
                let mut node_id = tgt.to_string();
                while node_id != src {
                    if let Some(n) = nodes.get(&node_id) {
                        path.push(n.clone());
                    }
                    node_id = parent.get(&node_id).cloned().unwrap_or_default();
                }
                if let Some(n) = nodes.get(src) {
                    path.push(n.clone());
                }
                path.reverse();
                return Ok(Some(path));
            }

            for edge in edges.iter() {
                let neighbor = if edge.source == current {
                    Some(&edge.target)
                } else if edge.target == current {
                    Some(&edge.source)
                } else {
                    None
                };

                if let Some(neighbor) = neighbor {
                    if visited.insert(neighbor.clone()) {
                        parent.insert(neighbor.clone(), current.clone());
                        queue.push_back(neighbor.clone());
                    }
                }
            }
        }

        Ok(None)
    }

    fn dijkstra_shortest_path(&self, src: &str, tgt: &str) -> Result<Option<Vec<Node>>> {
        if src == tgt {
            return Ok(self
                .nodes
                .read()
                .unwrap()
                .get(src)
                .cloned()
                .map(|n| vec![n]));
        }

        let nodes = self.nodes.read().unwrap();
        let edges = self.edges.read().unwrap();
        let mut distances: HashMap<String, f64> = HashMap::new();
        let mut parents: HashMap<String, String> = HashMap::new();
        let mut heap = std::collections::BinaryHeap::new();

        distances.insert(src.to_string(), 0.0);

        #[derive(PartialEq)]
        struct State(String, f64);
        impl Eq for State {}
        impl PartialOrd for State {
            fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
                Some(self.cmp(other))
            }
        }
        impl Ord for State {
            fn cmp(&self, other: &Self) -> std::cmp::Ordering {
                other
                    .1
                    .partial_cmp(&self.1)
                    .unwrap_or(std::cmp::Ordering::Equal)
            }
        }

        heap.push(State(src.to_string(), 0.0));

        while let Some(State(current, cost)) = heap.pop() {
            if current == tgt {
                let mut path = Vec::new();
                let mut node_id = tgt.to_string();
                while node_id != src {
                    if let Some(n) = nodes.get(&node_id) {
                        path.push(n.clone());
                    }
                    node_id = parents.get(&node_id).cloned().unwrap_or_default();
                }
                if let Some(n) = nodes.get(src) {
                    path.push(n.clone());
                }
                path.reverse();
                return Ok(Some(path));
            }

            if let Some(&best) = distances.get(&current) {
                if cost > best {
                    continue;
                }
            }

            for edge in edges.iter() {
                let neighbor = if edge.source == current {
                    Some(&edge.target)
                } else if edge.target == current {
                    Some(&edge.source)
                } else {
                    None
                };
                if let Some(neighbor) = neighbor {
                    let next_cost = cost + edge.weight;
                    let is_better = distances.get(neighbor).is_none_or(|&d| next_cost < d);
                    if is_better {
                        distances.insert(neighbor.clone(), next_cost);
                        parents.insert(neighbor.clone(), current.clone());
                        heap.push(State(neighbor.clone(), next_cost));
                    }
                }
            }
        }

        Ok(None)
    }

    fn subgraph(&self, node_ids: &[&str]) -> Result<(Vec<Node>, Vec<Edge>)> {
        let nodes = self.nodes.read().unwrap();
        let edges = self.edges.read().unwrap();
        let id_set: std::collections::HashSet<&str> = node_ids.iter().copied().collect();

        let sg_nodes: Vec<Node> = nodes
            .values()
            .filter(|n| id_set.contains(n.id.as_str()))
            .cloned()
            .collect();
        let sg_edges: Vec<Edge> = edges
            .iter()
            .filter(|e| id_set.contains(e.source.as_str()) && id_set.contains(e.target.as_str()))
            .cloned()
            .collect();

        Ok((sg_nodes, sg_edges))
    }

    fn node_count(&self) -> Result<usize> {
        Ok(self.nodes.read().unwrap().len())
    }

    fn edge_count(&self) -> Result<usize> {
        Ok(self.edges.read().unwrap().len())
    }

    fn remove_node(&self, id: &str) -> Result<()> {
        self.nodes.write().unwrap().remove(id);
        Ok(())
    }

    fn remove_edge(&self, source: &str, target: &str, relation: &str) -> Result<()> {
        let mut edges = self.edges.write().unwrap();
        edges.retain(|e| !(e.source == source && e.target == target && e.relation == relation));
        Ok(())
    }

    fn clear(&self) -> Result<()> {
        self.nodes.write().unwrap().clear();
        self.edges.write().unwrap().clear();
        self.inverted_index.write().unwrap().clear();
        Ok(())
    }
}

fn tokenize(s: &str) -> Vec<String> {
    s.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty() && t.len() >= 2)
        .map(|t| t.to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use tempfile;

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

    fn make_edge(src: &str, tgt: &str, relation: &str) -> Edge {
        Edge {
            source: src.to_string(),
            target: tgt.to_string(),
            relation: relation.to_string(),
            confidence: "EXTRACTED".to_string(),
            source_file: Some("test.py".to_string()),
            weight: 1.0,
            context: None,
        }
    }

    #[test]
    fn test_memory_store_add_get_node() {
        let store = MemoryGraphStore::new();
        store.add_node(make_node("a", "Alpha")).unwrap();
        let node = store.get_node("a").unwrap().unwrap();
        assert_eq!(node.label, "Alpha");
    }

    #[test]
    fn test_memory_store_search() {
        let store = MemoryGraphStore::new();
        store.add_node(make_node("auth", "AuthService")).unwrap();
        store.add_node(make_node("user", "UserService")).unwrap();

        let results = store.search("auth", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].1.label, "AuthService");
    }

    #[test]
    fn test_memory_store_neighbors() {
        let store = MemoryGraphStore::new();
        store.add_node(make_node("a", "A")).unwrap();
        store.add_node(make_node("b", "B")).unwrap();
        store.add_edge(make_edge("a", "b", "connects")).unwrap();

        let neighbors = store.neighbors("a", None).unwrap();
        assert_eq!(neighbors.len(), 1);
        assert_eq!(neighbors[0].0.label, "B");
    }

    #[test]
    fn test_memory_store_shortest_path() {
        let store = MemoryGraphStore::new();
        store.add_node(make_node("a", "A")).unwrap();
        store.add_node(make_node("b", "B")).unwrap();
        store.add_node(make_node("c", "C")).unwrap();
        store.add_edge(make_edge("a", "b", "connects")).unwrap();
        store.add_edge(make_edge("b", "c", "connects")).unwrap();

        let path = store.shortest_path("a", "c").unwrap().unwrap();
        assert_eq!(path.len(), 3);
        assert_eq!(path[0].label, "A");
        assert_eq!(path[1].label, "B");
        assert_eq!(path[2].label, "C");
    }

    #[test]
    fn test_memory_store_shortest_path_same_node() {
        let store = MemoryGraphStore::new();
        store.add_node(make_node("a", "A")).unwrap();
        let path = store.shortest_path("a", "a").unwrap().unwrap();
        assert_eq!(path.len(), 1);
    }

    #[test]
    fn test_memory_store_shortest_path_no_path() {
        let store = MemoryGraphStore::new();
        store.add_node(make_node("a", "A")).unwrap();
        store.add_node(make_node("b", "B")).unwrap();
        let path = store.shortest_path("a", "b").unwrap();
        assert!(path.is_none());
    }

    #[test]
    fn test_memory_store_subgraph() {
        let store = MemoryGraphStore::new();
        store.add_node(make_node("a", "A")).unwrap();
        store.add_node(make_node("b", "B")).unwrap();
        store.add_node(make_node("c", "C")).unwrap();
        store.add_edge(make_edge("a", "b", "connects")).unwrap();
        store.add_edge(make_edge("b", "c", "connects")).unwrap();

        let (sg_nodes, sg_edges) = store.subgraph(&["a", "b"]).unwrap();
        assert_eq!(sg_nodes.len(), 2);
        assert_eq!(sg_edges.len(), 1);
    }

    #[test]
    fn test_tokenize() {
        let tokens = tokenize("AuthService");
        assert!(tokens.contains(&"authservice".to_string()));

        let tokens = tokenize("Hello World");
        assert!(tokens.contains(&"hello".to_string()));
        assert!(tokens.contains(&"world".to_string()));
    }

    #[test]
    fn test_sled_store_basic_ops() {
        let store = SledGraphStore::temporary().unwrap();
        store.add_node(make_node("a", "Alpha")).unwrap();
        store.add_node(make_node("b", "Beta")).unwrap();
        store.add_edge(make_edge("a", "b", "connects")).unwrap();

        assert_eq!(store.node_count().unwrap(), 2);
        assert_eq!(store.edge_count().unwrap(), 1);

        let node = store.get_node("a").unwrap().unwrap();
        assert_eq!(node.label, "Alpha");
    }

    #[test]
    fn test_sled_store_shortest_path() {
        let store = SledGraphStore::temporary().unwrap();
        store.add_node(make_node("a", "A")).unwrap();
        store.add_node(make_node("b", "B")).unwrap();
        store.add_node(make_node("c", "C")).unwrap();
        store.add_edge(make_edge("a", "b", "connects")).unwrap();
        store.add_edge(make_edge("b", "c", "connects")).unwrap();

        let path = store.shortest_path("a", "c").unwrap().unwrap();
        assert_eq!(path.len(), 3);
        assert_eq!(path[0].label, "A");
        assert_eq!(path[1].label, "B");
        assert_eq!(path[2].label, "C");
    }

    #[test]
    fn test_sled_store_shortest_path_no_path() {
        let store = SledGraphStore::temporary().unwrap();
        store.add_node(make_node("a", "A")).unwrap();
        store.add_node(make_node("b", "B")).unwrap();
        // no edge between a and b
        let path = store.shortest_path("a", "b").unwrap();
        assert!(path.is_none());
    }

    #[test]
    fn test_sled_store_rebuild_index() {
        let store = SledGraphStore::temporary().unwrap();
        store.add_node(make_node("auth", "AuthService")).unwrap();
        store.rebuild_index().unwrap();

        let results = store.search("auth", 10).unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_store_backend_memory() {
        let backend = StoreBackend::Memory(MemoryGraphStore::new());
        backend.add_node(make_node("a", "A")).unwrap();
        assert_eq!(backend.node_count().unwrap(), 1);
    }

    // --- Edge case tests ---

    #[test]
    fn test_memory_get_node_nonexistent() {
        let store = MemoryGraphStore::new();
        let node = store.get_node("nonexistent").unwrap();
        assert!(node.is_none());
    }

    #[test]
    fn test_memory_neighbors_nonexistent() {
        let store = MemoryGraphStore::new();
        let nbrs = store.neighbors("ghost", None).unwrap();
        assert!(nbrs.is_empty());
    }

    #[test]
    fn test_memory_neighbors_relation_filter() {
        let store = MemoryGraphStore::new();
        store.add_node(make_node("a", "A")).unwrap();
        store.add_node(make_node("b", "B")).unwrap();
        store.add_edge(make_edge("a", "b", "calls")).unwrap();
        store.add_edge(make_edge("a", "b", "imports")).unwrap();

        let calls = store.neighbors("a", Some("calls")).unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].1.relation, "calls");

        let imports = store.neighbors("a", Some("imports")).unwrap();
        assert_eq!(imports.len(), 1);

        let none = store.neighbors("a", Some("inherits")).unwrap();
        assert!(none.is_empty());
    }

    #[test]
    fn test_memory_search_empty_query() {
        let store = MemoryGraphStore::new();
        store.add_node(make_node("a", "Alpha")).unwrap();
        let results = store.search("", 10).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_memory_search_single_char() {
        let store = MemoryGraphStore::new();
        store.add_node(make_node("a", "Alpha")).unwrap();
        let results = store.search("A", 10).unwrap();
        assert!(results.is_empty(), "single char should be tokenized away");
    }

    #[test]
    fn test_memory_search_no_match() {
        let store = MemoryGraphStore::new();
        store.add_node(make_node("a", "Alpha")).unwrap();
        let results = store.search("Zeta", 10).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_memory_remove_node_nonexistent() {
        let store = MemoryGraphStore::new();
        store.remove_node("ghost").unwrap();
        // no panic — should be a no-op
        assert_eq!(store.node_count().unwrap(), 0);
    }

    #[test]
    fn test_memory_remove_edge_nonexistent() {
        let store = MemoryGraphStore::new();
        store.remove_edge("a", "b", "calls").unwrap();
        assert_eq!(store.edge_count().unwrap(), 0);
    }

    #[test]
    fn test_memory_add_duplicate_edge() {
        let store = MemoryGraphStore::new();
        store.add_node(make_node("a", "A")).unwrap();
        store.add_node(make_node("b", "B")).unwrap();
        let e1 = make_edge("a", "b", "calls");
        let e2 = make_edge("a", "b", "calls");
        store.add_edge(e1).unwrap();
        store.add_edge(e2).unwrap();
        assert_eq!(
            store.edge_count().unwrap(),
            2,
            "duplicate edges are allowed"
        );
    }

    #[test]
    fn test_memory_subgraph_nonexistent_ids() {
        let store = MemoryGraphStore::new();
        let (nodes, edges) = store.subgraph(&["ghost1", "ghost2"]).unwrap();
        assert!(nodes.is_empty());
        assert!(edges.is_empty());
    }

    #[test]
    fn test_memory_shortest_path_missing_src() {
        let store = MemoryGraphStore::new();
        store.add_node(make_node("b", "B")).unwrap();
        let path = store.shortest_path("a", "b").unwrap();
        assert!(path.is_none(), "no path when src node missing");
    }

    #[test]
    fn test_memory_clear_then_ops() {
        let store = MemoryGraphStore::new();
        store.add_node(make_node("a", "A")).unwrap();
        store.add_edge(make_edge("a", "b", "calls")).unwrap();
        store.clear().unwrap();
        assert_eq!(store.node_count().unwrap(), 0);
        assert_eq!(store.edge_count().unwrap(), 0);
        // After clear, adding should still work
        store.add_node(make_node("b", "B")).unwrap();
        assert_eq!(store.node_count().unwrap(), 1);
    }

    #[test]
    fn test_sled_store_clear() {
        let store = SledGraphStore::temporary().unwrap();
        store.add_node(make_node("a", "A")).unwrap();
        store.clear().unwrap();
        assert_eq!(store.node_count().unwrap(), 0);
    }

    #[test]
    fn test_dijkstra_shortest_path_weighted() {
        let store = MemoryGraphStore::new();
        store.add_node(make_node("a", "A")).unwrap();
        store.add_node(make_node("b", "B")).unwrap();
        store.add_node(make_node("c", "C")).unwrap();
        store.add_node(make_node("d", "D")).unwrap();
        // a -> b (1.0), a -> c (10.0), b -> d (1.0), c -> d (1.0)
        store
            .add_edge(make_edge_weighted("a", "b", "connects", 1.0))
            .unwrap();
        store
            .add_edge(make_edge_weighted("a", "c", "connects", 10.0))
            .unwrap();
        store
            .add_edge(make_edge_weighted("b", "d", "connects", 1.0))
            .unwrap();
        store
            .add_edge(make_edge_weighted("c", "d", "connects", 1.0))
            .unwrap();

        let path = store.dijkstra_shortest_path("a", "d").unwrap().unwrap();
        assert_eq!(path.len(), 3, "shortest path should be a->b->d");
        assert_eq!(path[0].label, "A");
        assert_eq!(path[1].label, "B");
        assert_eq!(path[2].label, "D");
    }

    #[test]
    fn test_dijkstra_no_path() {
        let store = MemoryGraphStore::new();
        store.add_node(make_node("a", "A")).unwrap();
        store.add_node(make_node("b", "B")).unwrap();
        let path = store.dijkstra_shortest_path("a", "b").unwrap();
        assert!(path.is_none());
    }

    #[test]
    fn test_dijkstra_same_node() {
        let store = MemoryGraphStore::new();
        store.add_node(make_node("a", "A")).unwrap();
        let path = store.dijkstra_shortest_path("a", "a").unwrap().unwrap();
        assert_eq!(path.len(), 1);
    }

    fn make_edge_weighted(src: &str, tgt: &str, relation: &str, weight: f64) -> Edge {
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
    fn test_sled_store_persistence() {
        let tmpdir = tempfile::tempdir().unwrap();
        let db_path = tmpdir.path().join("sled.db");

        {
            let store = SledGraphStore::open(&db_path).unwrap();
            store.add_node(make_node("persist_a", "PersistA")).unwrap();
            store.add_node(make_node("persist_b", "PersistB")).unwrap();
            store
                .add_edge(make_edge("persist_a", "persist_b", "persists"))
                .unwrap();
            assert_eq!(store.node_count().unwrap(), 2);
            assert_eq!(store.edge_count().unwrap(), 1);
            drop(store);
        }

        {
            let store = SledGraphStore::open(&db_path).unwrap();
            assert_eq!(store.node_count().unwrap(), 2);
            assert_eq!(store.edge_count().unwrap(), 1);
            let node = store.get_node("persist_a").unwrap().unwrap();
            assert_eq!(node.label, "PersistA");
            let all_nodes = store.get_all_nodes().unwrap();
            let ids: Vec<&str> = all_nodes.iter().map(|n| n.id.as_str()).collect();
            assert!(ids.contains(&"persist_a"));
            assert!(ids.contains(&"persist_b"));
        }
    }

    #[test]
    fn test_sled_store_reopen_empty() {
        let tmpdir = tempfile::tempdir().unwrap();
        let db_path = tmpdir.path().join("sled.db");

        {
            let store = SledGraphStore::open(&db_path).unwrap();
            assert_eq!(store.node_count().unwrap(), 0);
            assert_eq!(store.edge_count().unwrap(), 0);
            drop(store);
        }

        {
            let store = SledGraphStore::open(&db_path).unwrap();
            assert_eq!(store.node_count().unwrap(), 0);
            assert_eq!(store.edge_count().unwrap(), 0);
        }
    }

    #[test]
    fn test_sled_store_search_after_reopen_with_new_add() {
        let tmpdir = tempfile::tempdir().unwrap();
        let db_path = tmpdir.path().join("sled.db");

        {
            let store = SledGraphStore::open(&db_path).unwrap();
            store.add_node(make_node("auth", "AuthService")).unwrap();
            drop(store);
        }

        {
            let store = SledGraphStore::open(&db_path).unwrap();
            // Adding a new node makes the in-memory index non-empty.
            // Without rebuild on open, search uses the index and misses "auth".
            store.add_node(make_node("new", "NewService")).unwrap();
            let results = store.search("auth", 10).unwrap();
            assert_eq!(
                results.len(),
                1,
                "persisted node must be found after reopen+add"
            );
        }
    }
}
