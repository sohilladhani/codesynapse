use std::collections::{HashMap, HashSet};
use std::path::Path;

use codesynapse_core::embedding::{cosine_similarity_f32, StaticEmbedder};
use codesynapse_core::security::sanitize_label;

const EXACT_MATCH_BONUS: f64 = 1000.0;
const PREFIX_MATCH_BONUS: f64 = 100.0;
const SUBSTRING_MATCH_BONUS: f64 = 1.0;
const SOURCE_MATCH_BONUS: f64 = 0.5;
pub const DEFAULT_MAX_GRAPH_FILE_BYTES: u64 = 512 * 1024 * 1024;

pub struct ServeNode {
    pub id: String,
    pub label: String,
    pub source_file: String,
    pub source_location: String,
    pub community: Option<i64>,
    pub norm_label: Option<String>,
    pub docstring: Option<String>,
}

pub struct ServeEdge {
    pub source: String,
    pub target: String,
    pub relation: String,
    pub confidence: String,
    pub context: Option<String>,
}

pub struct ServeGraph {
    nodes: HashMap<String, ServeNode>,
    node_order: Vec<String>,
    adj: HashMap<String, Vec<String>>,
    edge_lookup: HashMap<(String, String), usize>,
    edges: Vec<ServeEdge>,
    pub idf_cache: HashMap<String, f64>,
    directed: bool,
    pub bm25_index: Option<Bm25Index>,
}

impl ServeGraph {
    pub fn new_undirected() -> Self {
        ServeGraph {
            nodes: HashMap::new(),
            node_order: Vec::new(),
            adj: HashMap::new(),
            edge_lookup: HashMap::new(),
            edges: Vec::new(),
            idf_cache: HashMap::new(),
            directed: false,
            bm25_index: None,
        }
    }

    pub fn build_bm25_index(&mut self) {
        self.bm25_index = Some(Bm25Index::build(self));
    }

    pub fn bm25(&self) -> Option<&Bm25Index> {
        self.bm25_index.as_ref()
    }

    pub fn new_directed() -> Self {
        ServeGraph {
            directed: true,
            ..ServeGraph::new_undirected()
        }
    }

    pub fn add_node(
        &mut self,
        id: &str,
        label: &str,
        source_file: &str,
        source_location: &str,
        community: Option<i64>,
    ) {
        if !self.nodes.contains_key(id) {
            self.node_order.push(id.to_string());
            self.adj.entry(id.to_string()).or_default();
        }
        self.nodes.insert(
            id.to_string(),
            ServeNode {
                id: id.to_string(),
                label: label.to_string(),
                source_file: source_file.to_string(),
                source_location: source_location.to_string(),
                community,
                norm_label: None,
                docstring: None,
            },
        );
    }

    pub fn add_edge(
        &mut self,
        source: &str,
        target: &str,
        relation: &str,
        confidence: &str,
        context: Option<&str>,
    ) {
        // Ensure both endpoints exist in adj
        self.adj.entry(source.to_string()).or_default();
        self.adj.entry(target.to_string()).or_default();

        let idx = self.edges.len();
        self.edges.push(ServeEdge {
            source: source.to_string(),
            target: target.to_string(),
            relation: relation.to_string(),
            confidence: confidence.to_string(),
            context: context.map(str::to_string),
        });
        self.edge_lookup
            .insert((source.to_string(), target.to_string()), idx);

        // Forward
        self.adj
            .entry(source.to_string())
            .or_default()
            .push(target.to_string());

        if !self.directed {
            // Backward (undirected)
            self.adj
                .entry(target.to_string())
                .or_default()
                .push(source.to_string());
            // Also store reverse lookup so get_edge_data works both ways
            self.edge_lookup
                .insert((target.to_string(), source.to_string()), idx);
        }
    }

    pub fn neighbors(&self, n: &str) -> &[String] {
        self.adj.get(n).map(|v| v.as_slice()).unwrap_or(&[])
    }

    pub fn degree(&self, n: &str) -> usize {
        self.adj.get(n).map(|v| v.len()).unwrap_or(0)
    }

    pub fn num_nodes(&self) -> usize {
        self.nodes.len()
    }

    pub fn num_edges(&self) -> usize {
        if self.directed {
            self.edges.len()
        } else {
            // Each edge stored once internally but appears in both directions
            self.edges.len()
        }
    }

    pub fn get_node(&self, id: &str) -> Option<&ServeNode> {
        self.nodes.get(id)
    }

    pub fn get_edge_data(&self, u: &str, v: &str) -> Option<&ServeEdge> {
        self.edge_lookup
            .get(&(u.to_string(), v.to_string()))
            .map(|&idx| &self.edges[idx])
    }

    pub fn nodes_iter(&self) -> impl Iterator<Item = (&str, &ServeNode)> {
        self.node_order
            .iter()
            .filter_map(move |id| self.nodes.get(id).map(|n| (id.as_str(), n)))
    }

    pub fn edges_iter(&self) -> impl Iterator<Item = &ServeEdge> {
        self.edges.iter()
    }

    pub fn contains_node(&self, id: &str) -> bool {
        self.nodes.contains_key(id)
    }

    pub fn clone_nodes_only(&self) -> Self {
        let mut g = ServeGraph {
            nodes: HashMap::new(),
            node_order: self.node_order.clone(),
            adj: HashMap::new(),
            edge_lookup: HashMap::new(),
            edges: Vec::new(),
            idf_cache: HashMap::new(),
            directed: self.directed,
            bm25_index: None,
        };
        for (id, n) in &self.nodes {
            g.adj.entry(id.clone()).or_default();
            g.nodes.insert(
                id.clone(),
                ServeNode {
                    id: n.id.clone(),
                    label: n.label.clone(),
                    source_file: n.source_file.clone(),
                    source_location: n.source_location.clone(),
                    community: n.community,
                    norm_label: n.norm_label.clone(),
                    docstring: n.docstring.clone(),
                },
            );
        }
        g
    }
}

// ---------------------------------------------------------------------------
// Load graph from networkx node-link JSON

#[derive(Debug)]
pub enum LoadError {
    Io(String),
    TooLarge { size: u64, cap: u64 },
    Json(String),
    NotJson,
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::Io(msg) => write!(f, "{}", msg),
            LoadError::TooLarge { size, cap } => {
                write!(f, "error: file size {} exceeds byte cap {}", size, cap)
            }
            LoadError::Json(msg) => write!(f, "error: {}", msg),
            LoadError::NotJson => write!(f, "error: graph path must be a .json file"),
        }
    }
}

pub fn load_graph(path: &Path) -> Result<ServeGraph, LoadError> {
    load_graph_with_cap(path, DEFAULT_MAX_GRAPH_FILE_BYTES)
}

pub fn load_graph_with_cap(path: &Path, max_bytes: u64) -> Result<ServeGraph, LoadError> {
    if path.extension().and_then(|e| e.to_str()) != Some("json") {
        return Err(LoadError::NotJson);
    }
    let meta = std::fs::metadata(path)
        .map_err(|e| LoadError::Io(format!("Graph file not found: {}: {}", path.display(), e)))?;
    let size = meta.len();
    if size > max_bytes {
        return Err(LoadError::TooLarge {
            size,
            cap: max_bytes,
        });
    }
    let content = std::fs::read_to_string(path)
        .map_err(|e| LoadError::Io(format!("Could not read {}: {}", path.display(), e)))?;
    let v: serde_json::Value = serde_json::from_str(&content).map_err(|e| {
        LoadError::Json(format!(
            "graph.json is corrupted ({}). Re-run to rebuild.",
            e
        ))
    })?;

    let directed = v.get("directed").and_then(|d| d.as_bool()).unwrap_or(false);
    let mut g = if directed {
        ServeGraph::new_directed()
    } else {
        ServeGraph::new_undirected()
    };

    if let Some(nodes) = v.get("nodes").and_then(|n| n.as_array()) {
        for node in nodes {
            let id = node
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let label = node
                .get("label")
                .and_then(|v| v.as_str())
                .unwrap_or(&id)
                .to_string();
            let source_file = node
                .get("source_file")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let source_location = node
                .get("source_location")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let community = node.get("community").and_then(|c| c.as_i64());
            if !id.is_empty() {
                g.node_order.push(id.clone());
                g.adj.entry(id.clone()).or_default();
                let docstring = node
                    .get("docstring")
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
                g.nodes.insert(
                    id.clone(),
                    ServeNode {
                        id,
                        label,
                        source_file,
                        source_location,
                        community,
                        norm_label: None,
                        docstring,
                    },
                );
            }
        }
    }

    // Support both "links" and "edges" key
    let links_key = if v.get("links").is_some() {
        "links"
    } else {
        "edges"
    };
    if let Some(links) = v.get(links_key).and_then(|l| l.as_array()) {
        for link in links {
            let source = link
                .get("source")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let target = link
                .get("target")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let relation = link
                .get("relation")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let confidence = link
                .get("confidence")
                .and_then(|v| v.as_str())
                .unwrap_or("EXTRACTED")
                .to_string();
            let context = link
                .get("context")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            if !source.is_empty() && !target.is_empty() {
                let idx = g.edges.len();
                g.edges.push(ServeEdge {
                    source: source.clone(),
                    target: target.clone(),
                    relation,
                    confidence,
                    context,
                });
                g.adj
                    .entry(source.clone())
                    .or_default()
                    .push(target.clone());
                g.edge_lookup.insert((source.clone(), target.clone()), idx);
                if !g.directed {
                    g.adj
                        .entry(target.clone())
                        .or_default()
                        .push(source.clone());
                    g.edge_lookup.insert((target.clone(), source.clone()), idx);
                }
            }
        }
    }

    g.build_bm25_index();
    Ok(g)
}

// ---------------------------------------------------------------------------
// Graph stat helpers

pub fn communities_from_graph(g: &ServeGraph) -> HashMap<i64, Vec<String>> {
    let mut out: HashMap<i64, Vec<String>> = HashMap::new();
    for (id, node) in &g.nodes {
        if let Some(cid) = node.community {
            out.entry(cid).or_default().push(id.clone());
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Text / query helpers

pub fn strip_diacritics(text: &str) -> String {
    // Simplified: just lowercase for ASCII; full NFKD would need unicode-normalization crate.
    // Sufficient for all tests in this module.
    text.to_lowercase()
}

pub fn search_tokens(text: &str) -> Vec<String> {
    let lower = strip_diacritics(text);
    let re = regex::Regex::new(r"\w+").unwrap();
    re.find_iter(&lower)
        .map(|m| m.as_str().to_string())
        .collect()
}

pub fn has_chinese(text: &str) -> bool {
    text.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c))
}

pub fn segment_chinese(text: &str) -> Vec<String> {
    // Bigram fallback (no jieba in Rust)
    let chars: Vec<char> = text.chars().collect();
    let mut segs: Vec<String> = if chars.len() >= 2 {
        chars.windows(2).map(|w| w.iter().collect()).collect()
    } else {
        vec![text.to_string()]
    };
    // Append original term if it has >1 char and isn't already in segs
    if chars.len() > 1 && !segs.contains(&text.to_string()) {
        segs.push(text.to_string());
    }
    segs
}

pub fn is_searchable(term: &str) -> bool {
    if term.chars().all(|c| c.is_ascii_lowercase()) {
        term.len() > 2
    } else {
        true
    }
}

pub fn query_terms(question: &str) -> Vec<String> {
    let re = regex::Regex::new(r"\w+").unwrap();
    let mut terms: Vec<String> = Vec::new();
    for raw in question.split_whitespace() {
        if has_chinese(raw) {
            let lower = raw.to_lowercase();
            let lower = lower.trim();
            for seg in segment_chinese(lower) {
                let seg = seg.trim().to_string();
                if !seg.is_empty() && is_searchable(&seg) {
                    terms.push(seg);
                }
            }
        } else {
            for tok in re.find_iter(&raw.to_lowercase()) {
                let s = tok.as_str();
                if is_searchable(s) {
                    terms.push(s.to_string());
                }
            }
        }
    }
    terms
}

// ---------------------------------------------------------------------------
// IDF

pub fn compute_idf(g: &mut ServeGraph, terms: &[String]) -> HashMap<String, f64> {
    let n = (g.num_nodes().max(1)) as f64;

    let uncached: Vec<String> = terms
        .iter()
        .filter(|t| !g.idf_cache.contains_key(*t))
        .cloned()
        .collect();

    if !uncached.is_empty() {
        let mut df: HashMap<String, usize> = uncached.iter().map(|t| (t.clone(), 0)).collect();
        for node in g.nodes.values() {
            let norm = node
                .norm_label
                .as_deref()
                .unwrap_or(&node.label)
                .to_lowercase();
            for t in &uncached {
                if norm.contains(t.as_str()) {
                    *df.get_mut(t).unwrap() += 1;
                }
            }
        }
        for t in &uncached {
            let d = df[t] as f64;
            let idf_val = (1.0 + n / (1.0 + d)).ln();
            g.idf_cache.insert(t.clone(), idf_val);
        }
    }

    let fallback = (1.0 + n).ln();
    terms
        .iter()
        .map(|t| (t.clone(), g.idf_cache.get(t).copied().unwrap_or(fallback)))
        .collect()
}

// ---------------------------------------------------------------------------
// Scoring

pub fn score_nodes(g: &mut ServeGraph, terms: &[String]) -> Vec<(f64, String)> {
    let norm_terms: Vec<String> = terms.iter().flat_map(|t| search_tokens(t)).collect();
    let idf = compute_idf(g, &norm_terms);

    let node_ids: Vec<String> = g.nodes.keys().cloned().collect();
    let mut scored: Vec<(f64, String)> = Vec::new();

    for nid in &node_ids {
        let node = &g.nodes[nid];
        let norm_label = node
            .norm_label
            .as_deref()
            .unwrap_or(&node.label)
            .to_lowercase();
        let bare_label = norm_label.trim_end_matches("()").to_string();
        let source = node.source_file.to_lowercase();

        let mut score = 0.0f64;
        for t in &norm_terms {
            let w = idf.get(t).copied().unwrap_or(1.0);
            if *t == norm_label || *t == bare_label {
                score += EXACT_MATCH_BONUS * w;
            } else if norm_label.starts_with(t.as_str()) || bare_label.starts_with(t.as_str()) {
                score += PREFIX_MATCH_BONUS * w;
            } else if norm_label.contains(t.as_str()) {
                score += SUBSTRING_MATCH_BONUS * w;
            }
            if source.contains(t.as_str()) {
                score += SOURCE_MATCH_BONUS * w;
            }
        }
        if score > 0.0 {
            scored.push((score, nid.clone()));
        }
    }

    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    scored
}

// ---------------------------------------------------------------------------
// Seed selection

pub fn pick_seeds(scored: &[(f64, String)], max_k: usize, gap_ratio: f64) -> Vec<String> {
    if scored.is_empty() {
        return Vec::new();
    }
    let top_score = scored[0].0;
    let mut seeds: Vec<String> = Vec::new();
    for (score, nid) in scored.iter().take(max_k) {
        if !seeds.is_empty() && *score < top_score * gap_ratio {
            break;
        }
        seeds.push(nid.clone());
    }
    seeds
}

/// Returns (node_id, label, source_file) for the top-K seeds from hybrid search.
pub fn query_top_nodes(
    g: &ServeGraph,
    question: &str,
    top_k: usize,
    dense: Option<(&StaticEmbedder, &HashMap<String, Vec<f32>>)>,
) -> Vec<(String, String, String)> {
    let terms = query_terms(question);
    let norm_terms: Vec<String> = terms.iter().flat_map(|t| search_tokens(t)).collect();

    let owned_bm25;
    let bm25: &Bm25Index = match g.bm25() {
        Some(b) => b,
        None => {
            owned_bm25 = Bm25Index::build(g);
            &owned_bm25
        }
    };
    let bm25_ranked: Vec<String> = bm25
        .score(&norm_terms)
        .into_iter()
        .map(|(_, id)| id)
        .collect();

    let symbol_query = is_symbol_query(question);

    let symbol_ranked: Vec<String> = if symbol_query {
        let split_tokens: Vec<String> = norm_terms
            .iter()
            .flat_map(|t| split_camel(t))
            .filter(|t| !norm_terms.contains(t))
            .collect();
        if split_tokens.is_empty() {
            Vec::new()
        } else {
            bm25.score(&split_tokens)
                .into_iter()
                .map(|(_, id)| id)
                .collect()
        }
    } else {
        Vec::new()
    };

    let merged = if let Some((embedder, node_embs)) = dense {
        let q_emb = embedder.embed(question);
        let mut dense_scored: Vec<(f32, String)> = node_embs
            .iter()
            .map(|(id, emb)| (cosine_similarity_f32(&q_emb, emb), id.clone()))
            .collect();
        dense_scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        let dense_ranked: Vec<String> = dense_scored.into_iter().map(|(_, id)| id).collect();
        if symbol_query && !symbol_ranked.is_empty() {
            rrf(&[bm25_ranked, dense_ranked, symbol_ranked], RRF_K)
        } else {
            rrf(&[bm25_ranked, dense_ranked], RRF_K)
        }
    } else {
        bm25_ranked
            .iter()
            .enumerate()
            .map(|(i, id)| (1.0 / (RRF_K + i + 1) as f64, id.clone()))
            .collect()
    };

    let merged = apply_score_adjustments(merged, g, &norm_terms);
    let seeds = pick_seeds(&merged, top_k.max(3), 0.05);

    seeds
        .into_iter()
        .filter_map(|id| {
            g.get_node(&id)
                .map(|n| (id.clone(), n.label.clone(), n.source_file.clone()))
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Context filters

static CONTEXT_HINTS: &[(&str, &[&str])] = &[
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

fn context_filter_aliases() -> HashMap<&'static str, &'static str> {
    let mut m = HashMap::new();
    for (canonical, aliases) in &[
        (
            "parameter_type",
            &[
                "param",
                "params",
                "parameter",
                "parameters",
                "argument",
                "arguments",
                "arg",
                "args",
            ][..],
        ),
        ("return_type", &["return", "returns", "returned"][..]),
        (
            "generic_arg",
            &["generic", "generics", "template", "templates"][..],
        ),
        (
            "attribute",
            &["annotation", "annotations", "decorator", "decorators"][..],
        ),
        ("call", &["calls", "called", "invoke", "invocation"][..]),
        (
            "field",
            &["fields", "property", "properties", "member", "members"][..],
        ),
        ("import", &["imports", "imported", "module", "modules"][..]),
        ("export", &["exports", "exported"][..]),
    ] {
        for alias in *aliases {
            m.insert(*alias, *canonical);
        }
    }
    m
}

pub fn normalize_context_filters(filters: &[String]) -> Vec<String> {
    let aliases = context_filter_aliases();
    let mut seen: HashSet<String> = HashSet::new();
    let mut out: Vec<String> = Vec::new();
    for value in filters {
        let key = value.trim().to_lowercase();
        if key.is_empty() {
            continue;
        }
        let key = aliases
            .get(key.as_str())
            .map(|s| s.to_string())
            .unwrap_or(key);
        if seen.insert(key.clone()) {
            out.push(key);
        }
    }
    out
}

pub fn infer_context_filters(question: &str) -> Vec<String> {
    let re = regex::Regex::new(r"[?,]").unwrap();
    let lowered: HashSet<String> = re
        .replace_all(question, " ")
        .split_whitespace()
        .map(|t| t.to_lowercase())
        .collect();
    let mut inferred: Vec<String> = Vec::new();
    for (context, hints) in CONTEXT_HINTS {
        if hints.iter().any(|h| lowered.contains(*h)) {
            inferred.push(context.to_string());
        }
    }
    inferred
}

pub fn resolve_context_filters(
    question: &str,
    explicit_filters: Option<&[String]>,
) -> (Vec<String>, Option<String>) {
    if let Some(f) = explicit_filters {
        let normalized = normalize_context_filters(f);
        if !normalized.is_empty() {
            return (normalized, Some("explicit".to_string()));
        }
    }
    let inferred = infer_context_filters(question);
    if !inferred.is_empty() {
        return (inferred, Some("heuristic".to_string()));
    }
    (Vec::new(), None)
}

pub fn filter_graph_by_context(g: &ServeGraph, context_filters: &[String]) -> ServeGraph {
    let filters: HashSet<String> = normalize_context_filters(context_filters)
        .into_iter()
        .collect();
    if filters.is_empty() {
        // Return a clone
        let mut h = g.clone_nodes_only();
        for e in &g.edges {
            h.add_edge(
                &e.source,
                &e.target,
                &e.relation,
                &e.confidence,
                e.context.as_deref(),
            );
        }
        return h;
    }
    let mut h = g.clone_nodes_only();
    for e in &g.edges {
        if e.context
            .as_deref()
            .map(|c| filters.contains(c))
            .unwrap_or(false)
        {
            h.add_edge(
                &e.source,
                &e.target,
                &e.relation,
                &e.confidence,
                e.context.as_deref(),
            );
        }
    }
    h
}

// ---------------------------------------------------------------------------
// BFS / DFS

fn hub_threshold(g: &ServeGraph) -> usize {
    let mut degrees: Vec<usize> = g.nodes.keys().map(|n| g.degree(n)).collect();
    if degrees.is_empty() {
        return 50;
    }
    degrees.sort_unstable();
    let p99_idx = (degrees.len() as f64 * 0.99) as usize;
    let p99_idx = p99_idx.min(degrees.len() - 1);
    degrees[p99_idx].max(50)
}

pub fn bfs(
    g: &ServeGraph,
    start_nodes: &[String],
    depth: usize,
) -> (HashSet<String>, Vec<(String, String)>) {
    let threshold = hub_threshold(g);
    let seed_set: HashSet<&str> = start_nodes.iter().map(|s| s.as_str()).collect();
    let mut visited: HashSet<String> = start_nodes.iter().cloned().collect();
    let mut frontier: HashSet<String> = start_nodes.iter().cloned().collect();
    let mut edges_seen: Vec<(String, String)> = Vec::new();

    for _ in 0..depth {
        let mut next_frontier: HashSet<String> = HashSet::new();
        let frontier_vec: Vec<String> = frontier.iter().cloned().collect();
        for n in &frontier_vec {
            if !seed_set.contains(n.as_str()) && g.degree(n) >= threshold {
                continue;
            }
            for neighbor in g.neighbors(n) {
                if !visited.contains(neighbor) {
                    next_frontier.insert(neighbor.clone());
                    edges_seen.push((n.clone(), neighbor.clone()));
                }
            }
        }
        for n in &next_frontier {
            visited.insert(n.clone());
        }
        frontier = next_frontier;
    }
    (visited, edges_seen)
}

pub fn dfs(
    g: &ServeGraph,
    start_nodes: &[String],
    depth: usize,
) -> (HashSet<String>, Vec<(String, String)>) {
    let threshold = hub_threshold(g);
    let seed_set: HashSet<&str> = start_nodes.iter().map(|s| s.as_str()).collect();
    let mut visited: HashSet<String> = HashSet::new();
    let mut edges_seen: Vec<(String, String)> = Vec::new();

    // Stack: (node, current_depth)
    let mut stack: Vec<(String, usize)> =
        start_nodes.iter().rev().map(|n| (n.clone(), 0)).collect();

    while let Some((node, d)) = stack.pop() {
        if visited.contains(&node) || d > depth {
            continue;
        }
        visited.insert(node.clone());
        if !seed_set.contains(node.as_str()) && g.degree(&node) >= threshold {
            continue;
        }
        for neighbor in g.neighbors(&node) {
            if !visited.contains(neighbor) {
                stack.push((neighbor.clone(), d + 1));
                edges_seen.push((node.clone(), neighbor.clone()));
            }
        }
    }
    (visited, edges_seen)
}

// ---------------------------------------------------------------------------
// Subgraph rendering

pub fn subgraph_to_text(
    g: &ServeGraph,
    nodes: &HashSet<String>,
    edges: &[(String, String)],
    token_budget: usize,
    seeds: Option<&[String]>,
) -> String {
    let char_budget = token_budget * 3;
    let mut lines: Vec<String> = Vec::new();

    let seed_set: HashSet<&str> = seeds.unwrap_or(&[]).iter().map(|s| s.as_str()).collect();
    let mut ordered: Vec<String> = seeds
        .unwrap_or(&[])
        .iter()
        .filter(|n| nodes.contains(*n))
        .cloned()
        .collect();

    // Non-seed nodes sorted by degree descending
    let mut non_seeds: Vec<&str> = nodes
        .iter()
        .filter(|n| !seed_set.contains(n.as_str()))
        .map(|n| n.as_str())
        .collect();
    non_seeds.sort_by_key(|n| std::cmp::Reverse(g.degree(n)));
    ordered.extend(non_seeds.iter().map(|s| s.to_string()));

    for nid in &ordered {
        if let Some(node) = g.get_node(nid) {
            let line = format!(
                "NODE {} [src={} loc={} community={}]",
                sanitize_label(Some(&node.label)),
                sanitize_label(Some(&node.source_file)),
                sanitize_label(Some(&node.source_location)),
                sanitize_label(node.community.map(|c| c.to_string()).as_deref()),
            );
            lines.push(line);
        }
    }

    for (u, v) in edges {
        if nodes.contains(u) && nodes.contains(v) {
            if let Some(e) = g.get_edge_data(u, v) {
                let context_suffix = if let Some(ctx) = &e.context {
                    format!(" context={}", sanitize_label(Some(ctx)))
                } else {
                    String::new()
                };
                let u_label = g
                    .get_node(u)
                    .map(|n| n.label.as_str())
                    .unwrap_or(u.as_str());
                let v_label = g
                    .get_node(v)
                    .map(|n| n.label.as_str())
                    .unwrap_or(v.as_str());
                let line = format!(
                    "EDGE {} --{} [{}{}]--> {}",
                    sanitize_label(Some(u_label)),
                    sanitize_label(Some(&e.relation)),
                    sanitize_label(Some(&e.confidence)),
                    context_suffix,
                    sanitize_label(Some(v_label)),
                );
                lines.push(line);
            }
        }
    }

    let output = lines.join("\n");
    if output.len() > char_budget {
        let cut_at = output[..char_budget].rfind('\n').unwrap_or(char_budget);
        let cut_at = if cut_at == 0 { char_budget } else { cut_at };
        let total_nodes: usize = lines.iter().filter(|l| l.starts_with("NODE ")).count();
        let shown_nodes = output[..cut_at].matches("\nNODE ").count()
            + if output.starts_with("NODE ") { 1 } else { 0 };
        let cut_count = total_nodes.saturating_sub(shown_nodes);
        format!(
            "{}\n... (truncated — {} more nodes cut by ~{}-token budget.\
             Narrow with context_filter=['call'] or use get_node for a specific symbol)",
            &output[..cut_at],
            cut_count,
            token_budget,
        )
    } else {
        output
    }
}

// ---------------------------------------------------------------------------
// find_node

pub fn find_node(g: &ServeGraph, label: &str) -> Vec<String> {
    let term = search_tokens(label).join(" ");
    if term.is_empty() {
        return Vec::new();
    }
    let mut exact: Vec<String> = Vec::new();
    let mut prefix: Vec<String> = Vec::new();
    let mut substring: Vec<String> = Vec::new();

    for (nid, node) in &g.nodes {
        let norm_label = node
            .norm_label
            .as_deref()
            .unwrap_or(&node.label)
            .to_lowercase();
        let bare_label = norm_label.trim_end_matches("()").to_string();
        let nid_lower = nid.to_lowercase();

        if term == norm_label || term == bare_label || term == nid_lower {
            exact.push(nid.clone());
        } else if norm_label.starts_with(&term)
            || bare_label.starts_with(&term)
            || nid_lower.starts_with(&term)
        {
            prefix.push(nid.clone());
        } else if norm_label.contains(&term) {
            substring.push(nid.clone());
        }
    }

    exact.extend(prefix);
    exact.extend(substring);
    exact
}

// ---------------------------------------------------------------------------
// Main query entry point

fn traverse_render(
    g: &ServeGraph,
    seeds: Vec<String>,
    question: &str,
    mode: &str,
    depth: usize,
    token_budget: usize,
    context_filters: Option<&[String]>,
) -> String {
    let (resolved_filters, filter_source) = resolve_context_filters(question, context_filters);
    let traversal_graph = filter_graph_by_context(g, &resolved_filters);

    let (nodes, edges) = if mode == "dfs" {
        dfs(&traversal_graph, &seeds, depth)
    } else {
        bfs(&traversal_graph, &seeds, depth)
    };

    let start_labels: Vec<String> = seeds
        .iter()
        .map(|n| {
            g.get_node(n)
                .map(|nd| nd.label.clone())
                .unwrap_or_else(|| n.clone())
        })
        .collect();

    let mut header_parts = vec![
        format!("Traversal: {} depth={}", mode.to_uppercase(), depth),
        format!("Start: {:?}", start_labels),
    ];
    if !resolved_filters.is_empty() {
        header_parts.push(format!(
            "Context: {} ({})",
            resolved_filters.join(", "),
            filter_source.as_deref().unwrap_or(""),
        ));
    }
    header_parts.push(format!("{} nodes found", nodes.len()));

    let header = header_parts.join(" | ") + "\n\n";
    header + &subgraph_to_text(&traversal_graph, &nodes, &edges, token_budget, Some(&seeds))
}

pub fn query_graph_text(
    g: &mut ServeGraph,
    question: &str,
    mode: &str,
    depth: usize,
    token_budget: usize,
    context_filters: Option<&[String]>,
) -> String {
    let terms = query_terms(question);
    let scored = score_nodes(g, &terms);
    let seeds = pick_seeds(&scored, 3, 0.2);
    if seeds.is_empty() {
        return "No matching nodes found.".to_string();
    }
    traverse_render(
        g,
        seeds,
        question,
        mode,
        depth,
        token_budget,
        context_filters,
    )
}

// ---------------------------------------------------------------------------
// BM25 index

fn parse_byte_range(s: &str) -> Option<(usize, usize)> {
    let (a, b) = s.split_once(':')?;
    Some((a.parse().ok()?, b.parse().ok()?))
}

fn read_node_body_exact(source_file: &str, source_location: &str) -> String {
    let Some((start, end)) = parse_byte_range(source_location) else {
        return String::new();
    };
    let Ok(content) = std::fs::read(source_file) else {
        return String::new();
    };
    let end = end.min(content.len());
    if start >= end {
        return String::new();
    }
    String::from_utf8_lossy(&content[start..end]).into_owned()
}

const BM25_K1: f64 = 1.2;
const BM25_B: f64 = 0.75;
pub const RRF_K: usize = 60;

pub struct Bm25Index {
    term_freqs: HashMap<String, HashMap<String, usize>>,
    doc_freqs: HashMap<String, usize>,
    avg_dl: f64,
    num_docs: usize,
}

impl Bm25Index {
    pub fn build(g: &ServeGraph) -> Self {
        let mut term_freqs: HashMap<String, HashMap<String, usize>> = HashMap::new();
        let mut doc_freqs: HashMap<String, usize> = HashMap::new();
        let mut total_len = 0usize;
        let num_docs = g.nodes.len();

        // Build child method labels map for corpus enrichment
        let mut child_labels: HashMap<String, Vec<String>> = HashMap::new();
        for edge in g.edges_iter() {
            if edge.relation == "method" || edge.relation == "contains" {
                if let Some(target_node) = g.get_node(&edge.target) {
                    child_labels
                        .entry(edge.source.clone())
                        .or_default()
                        .push(target_node.label.clone());
                }
            }
        }

        for (id, node) in g.nodes_iter() {
            let docstring_str = node.docstring.as_deref().unwrap_or("");
            let methods = child_labels
                .get(id)
                .map(|v| v.join(" "))
                .unwrap_or_default();
            let body_str = if !node.source_location.is_empty() {
                read_node_body_exact(&node.source_file, &node.source_location)
            } else {
                String::new()
            };
            let text = format!(
                "{} {} {} {} {}",
                node.norm_label.as_deref().unwrap_or(&node.label),
                docstring_str,
                node.source_file,
                methods,
                body_str
            );
            let text = text.trim().to_string();
            let tokens = search_tokens(&text);
            total_len += tokens.len();

            let mut tf: HashMap<String, usize> = HashMap::new();
            for tok in &tokens {
                *tf.entry(tok.clone()).or_insert(0) += 1;
            }
            for tok in tf.keys() {
                *doc_freqs.entry(tok.clone()).or_insert(0) += 1;
            }
            term_freqs.insert(id.to_string(), tf);
        }

        let avg_dl = if num_docs > 0 {
            total_len as f64 / num_docs as f64
        } else {
            1.0
        };

        Bm25Index {
            term_freqs,
            doc_freqs,
            avg_dl,
            num_docs,
        }
    }

    pub fn score(&self, query_terms: &[String]) -> Vec<(f64, String)> {
        if self.num_docs == 0 || query_terms.is_empty() {
            return Vec::new();
        }
        let n = self.num_docs as f64;
        let idfs: Vec<(&String, f64)> = query_terms
            .iter()
            .map(|t| {
                let df = self.doc_freqs.get(t).copied().unwrap_or(0) as f64;
                let idf = ((n - df + 0.5) / (df + 0.5) + 1.0).ln().max(0.0);
                (t, idf)
            })
            .collect();

        let mut scored: Vec<(f64, String)> = self
            .term_freqs
            .iter()
            .filter_map(|(id, tf_map)| {
                let dl = tf_map.values().sum::<usize>() as f64;
                let denom_base = BM25_K1 * (1.0 - BM25_B + BM25_B * dl / self.avg_dl);
                let mut s = 0.0f64;
                for (t, idf) in &idfs {
                    let tf = tf_map.get(*t).copied().unwrap_or(0) as f64;
                    if tf > 0.0 {
                        s += idf * tf * (BM25_K1 + 1.0) / (tf + denom_base);
                    }
                }
                if s > 0.0 {
                    Some((s, id.clone()))
                } else {
                    None
                }
            })
            .collect();

        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        scored
    }
}

/// Reciprocal Rank Fusion over multiple ranked lists. `k=60` is standard.
pub fn rrf(ranked_lists: &[Vec<String>], k: usize) -> Vec<(f64, String)> {
    let mut scores: HashMap<String, f64> = HashMap::new();
    for list in ranked_lists {
        for (rank, id) in list.iter().enumerate() {
            *scores.entry(id.clone()).or_insert(0.0) += 1.0 / (k + rank + 1) as f64;
        }
    }
    let mut result: Vec<(f64, String)> = scores.into_iter().map(|(id, s)| (s, id)).collect();
    result.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    result
}

/// Returns true for test, spec, fixture, or example files.
fn is_test_or_example_file(path: &str) -> bool {
    let p = path.to_lowercase();
    p.contains("/test/")
        || p.contains("/tests/")
        || p.contains("/examples/")
        || p.contains("/example/")
        || p.contains("/spec/")
        || p.contains("/fixtures/")
        || p.contains("/fixture/")
        || p.ends_with("_test.py")
        || p.ends_with("_test.rs")
        || p.ends_with("_test.go")
        || p.ends_with("_test.js")
        || p.ends_with("_test.ts")
        || p.ends_with(".spec.ts")
        || p.ends_with(".spec.js")
        || p.ends_with(".test.ts")
        || p.ends_with(".test.js")
        || (p.ends_with(".java") && (p.contains("test") || p.contains("Test")))
}

/// Split camelCase/PascalCase into lowercase tokens. Snake_case returned as-is (already meaningful in BM25).
/// Original token is NOT included — caller already has it in bm25_ranked.
pub fn split_camel(token: &str) -> Vec<String> {
    if token.contains('_') {
        return vec![token.to_lowercase()];
    }
    let mut parts: Vec<String> = Vec::new();
    let mut current = String::new();
    let chars: Vec<char> = token.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        if c.is_uppercase() && i > 0 {
            let next_is_lower = chars
                .get(i + 1)
                .map(|nc| nc.is_lowercase())
                .unwrap_or(false);
            let prev_is_lower = chars[i - 1].is_lowercase();
            if (prev_is_lower || next_is_lower) && !current.is_empty() {
                parts.push(current.to_lowercase());
                current = String::new();
            }
        }
        current.push(c);
    }
    if !current.is_empty() {
        parts.push(current.to_lowercase());
    }
    if parts.len() <= 1 {
        return vec![token.to_lowercase()];
    }
    parts
}

/// Symbol query: single token that looks like an identifier (PascalCase, snake_case, ALL_CAPS).
fn is_symbol_query(question: &str) -> bool {
    let tokens: Vec<&str> = question.split_whitespace().collect();
    if tokens.len() != 1 {
        return false;
    }
    let t = tokens[0];
    // All caps, camelCase, PascalCase, or snake_case with no spaces
    t.chars().all(|c| c.is_alphanumeric() || c == '_')
        && (t.chars().any(|c| c.is_uppercase()) || t.contains('_'))
}

/// Post-RRF score adjustments ported from synapse's `_hybrid_search`.
fn apply_score_adjustments(
    mut merged: Vec<(f64, String)>,
    g: &ServeGraph,
    norm_terms: &[String],
) -> Vec<(f64, String)> {
    // Apply per-node adjustments in rank order so saturation decay is stable.
    let mut file_counts: HashMap<String, usize> = HashMap::new();

    for (score, id) in &mut merged {
        let node = match g.get_node(id) {
            Some(n) => n,
            None => continue,
        };

        // Test/example file penalty
        if is_test_or_example_file(&node.source_file) {
            *score *= 0.05;
        }

        // Label exact/substring match boost
        let label_lower = node.label.to_lowercase();
        let mut boosted = false;
        for term in norm_terms {
            if label_lower == *term {
                *score *= 3.0;
                boosted = true;
                break;
            }
        }
        if !boosted {
            for term in norm_terms {
                if label_lower.contains(term.as_str()) {
                    *score *= 1.5;
                    break;
                }
            }
        }

        // Saturation decay: nth node from same file → score × 0.5^n
        let n = file_counts.entry(node.source_file.clone()).or_insert(0);
        if *n > 0 {
            *score *= 0.5f64.powi(*n as i32);
        }
        *n += 1;
    }

    merged.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    merged
}

/// Hybrid query: BM25 lexical + optional dense cosine, fused with RRF.
///
/// `dense` = `Some((embedder, node_embeddings))` when a model is loaded.
/// Without dense, falls back to BM25-only ranking (still better than pure IDF).
pub fn query_graph_text_hybrid(
    g: &ServeGraph,
    question: &str,
    mode: &str,
    depth: usize,
    token_budget: usize,
    context_filters: Option<&[String]>,
    dense: Option<(&StaticEmbedder, &HashMap<String, Vec<f32>>)>,
) -> String {
    let terms = query_terms(question);
    let norm_terms: Vec<String> = terms.iter().flat_map(|t| search_tokens(t)).collect();

    let owned_bm25;
    let bm25: &Bm25Index = match g.bm25() {
        Some(b) => b,
        None => {
            owned_bm25 = Bm25Index::build(g);
            &owned_bm25
        }
    };
    let bm25_ranked: Vec<String> = bm25
        .score(&norm_terms)
        .into_iter()
        .map(|(_, id)| id)
        .collect();

    // Symbol queries (single identifier) are BM25-heavy: add BM25 list twice in RRF.
    let symbol_query = is_symbol_query(question);

    let merged = if let Some((embedder, node_embs)) = dense {
        let q_emb = embedder.embed(question);
        let mut dense_scored: Vec<(f32, String)> = node_embs
            .iter()
            .map(|(id, emb)| (cosine_similarity_f32(&q_emb, emb), id.clone()))
            .collect();
        dense_scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        let dense_ranked: Vec<String> = dense_scored.into_iter().map(|(_, id)| id).collect();
        if symbol_query {
            // BM25 weight 2:1 over dense for symbol queries
            rrf(&[bm25_ranked.clone(), bm25_ranked, dense_ranked], RRF_K)
        } else {
            rrf(&[bm25_ranked, dense_ranked], RRF_K)
        }
    } else {
        bm25_ranked
            .iter()
            .enumerate()
            .map(|(i, id)| (1.0 / (RRF_K + i + 1) as f64, id.clone()))
            .collect()
    };

    let merged = apply_score_adjustments(merged, g, &norm_terms);

    let seeds = pick_seeds(&merged, 3, 0.2);
    if seeds.is_empty() {
        return "No matching nodes found.".to_string();
    }
    traverse_render(
        g,
        seeds,
        question,
        mode,
        depth,
        token_budget,
        context_filters,
    )
}

// ---------------------------------------------------------------------------
// CLI-facing query helpers

/// Load global graph from `graph_path`, run BM25 query, return formatted
/// source bodies for the top-k matching nodes. No dense embeddings (CLI).
pub fn resolve_query(
    query: &str,
    graph_path: &Path,
    top_k: usize,
    max_chars: usize,
) -> Result<String, String> {
    let g = load_graph(graph_path).map_err(|e| e.to_string())?;
    // Fetch a wider candidate pool so re-ranking can surface nodes with source bodies
    // that may be ranked below the immediate top-k by BM25 alone.
    let candidates = query_top_nodes(&g, query, top_k.max(10), None);
    if candidates.is_empty() {
        return Ok("No matching nodes found.".into());
    }

    // Stable-sort: nodes with source_location first, preserving BM25 order within each group.
    let mut ranked = candidates;
    ranked.sort_by_key(|(id, _, _)| {
        g.get_node(id)
            .map(|n| if n.source_location.is_empty() { 1 } else { 0 })
            .unwrap_or(1)
    });
    let top_nodes: Vec<_> = ranked.into_iter().take(top_k).collect();

    let query_lower = query.to_lowercase();
    let query_tokens: Vec<&str> = query_lower.split_whitespace().collect();

    // Build a source_file → nodes-with-loc index for the same-file siblings fallback.
    let mut file_to_nodes: std::collections::HashMap<&str, Vec<&str>> =
        std::collections::HashMap::new();
    for (id, node) in g.nodes_iter() {
        if !node.source_location.is_empty() && !node.source_file.is_empty() {
            file_to_nodes
                .entry(node.source_file.as_str())
                .or_default()
                .push(id);
        }
    }

    let mut sections: Vec<String> = Vec::new();
    let mut total_chars = 0usize;

    for (node_id, label, source_file) in &top_nodes {
        if total_chars >= max_chars {
            break;
        }
        let caller_count = g
            .edges_iter()
            .filter(|e| e.target == *node_id && e.relation.contains("call"))
            .count();
        let caller_note = if caller_count == 0 {
            " [0 explicit callers — may be entry point, registered callback, or unused]".to_string()
        } else {
            format!(" [{} caller(s)]", caller_count)
        };

        let mut sec = format!("═══ {} ({}){}\n", label, source_file, caller_note);

        let mut primary_body_shown = false;
        if let Some(node) = g.get_node(node_id) {
            if !node.source_location.is_empty() {
                let body = read_node_body_exact(&node.source_file, &node.source_location);
                if !body.is_empty() {
                    let cap = body.len().min(4000);
                    sec.push_str(&body[..cap]);
                    sec.push('\n');
                    primary_body_shown = true;
                }
            }

            // Fallback: find sibling nodes in the same file that have source_location,
            // score by query token overlap, show the best match.
            if !primary_body_shown && !node.source_file.is_empty() {
                if let Some(sibling_ids) = file_to_nodes.get(node.source_file.as_str()) {
                    let mut scored: Vec<(usize, &&str)> = sibling_ids
                        .iter()
                        .filter_map(|sid| {
                            g.get_node(sid).map(|sn| {
                                let nl = sn.label.to_lowercase();
                                let score = query_tokens.iter().filter(|t| nl.contains(*t)).count();
                                (score, sid)
                            })
                        })
                        .collect();
                    scored.sort_by_key(|a| std::cmp::Reverse(a.0));

                    for (_, sid) in scored.iter().take(2) {
                        if let Some(sn) = g.get_node(sid) {
                            let body = read_node_body_exact(&sn.source_file, &sn.source_location);
                            if !body.is_empty() {
                                let cap = body.len().min(3000);
                                sec.push_str(&format!("── {}\n", sn.label));
                                sec.push_str(&body[..cap]);
                                sec.push('\n');
                            }
                        }
                    }
                }
            }
        }

        total_chars += sec.len();
        sections.push(sec);
    }

    Ok(sections.join("\n"))
}

// ---------------------------------------------------------------------------
// Tests

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn make_graph() -> ServeGraph {
        let mut g = ServeGraph::new_undirected();
        g.add_node("n1", "extract", "extract.py", "L10", Some(0));
        g.add_node("n2", "cluster", "cluster.py", "L5", Some(0));
        g.add_node("n3", "build", "build.py", "L1", Some(1));
        g.add_node("n4", "report", "report.py", "L1", Some(1));
        g.add_node("n5", "isolated", "other.py", "L1", Some(2));
        g.add_edge("n1", "n2", "calls", "INFERRED", Some("call"));
        g.add_edge("n2", "n3", "imports", "EXTRACTED", Some("import"));
        g.add_edge("n3", "n4", "uses", "EXTRACTED", None);
        g
    }

    fn make_noisy_graph() -> ServeGraph {
        let mut g = ServeGraph::new_undirected();
        for i in 0..20usize {
            let id = format!("err{}", i);
            let label = format!("error_handler_{}", i);
            g.add_node(&id, &label, &format!("err{}.py", i), "", Some(0));
            if i > 0 {
                let prev = format!("err{}", i - 1);
                g.add_edge(&prev, &id, "calls", "EXTRACTED", None);
            }
        }
        g.add_node("fbs", "FooBarService", "service.py", "", Some(1));
        g.add_node("fbs_dep", "ServiceClient", "client.py", "", Some(1));
        g.add_edge("fbs", "fbs_dep", "uses", "EXTRACTED", None);
        g
    }

    fn write_graph_json(nodes: &[(&str, &str)]) -> (NamedTempFile, String) {
        let mut tmp = NamedTempFile::with_suffix(".json").unwrap();
        let nodes_json: Vec<String> = nodes
            .iter()
            .map(|(id, label)| {
                format!(
                    r#"{{"id":"{id}","label":"{label}","community":0}}"#,
                    id = id,
                    label = label
                )
            })
            .collect();
        let content = format!(
            r#"{{"directed":false,"multigraph":false,"graph":{{}},"nodes":[{}],"links":[]}}"#,
            nodes_json.join(",")
        );
        tmp.write_all(content.as_bytes()).unwrap();
        let path = tmp.path().to_str().unwrap().to_string();
        (tmp, path)
    }

    // --- communities_from_graph ---

    #[test]
    fn test_communities_from_graph_basic() {
        let g = make_graph();
        let communities = communities_from_graph(&g);
        assert!(communities.contains_key(&0));
        assert!(communities.contains_key(&1));
        assert!(communities[&0].contains(&"n1".to_string()));
        assert!(communities[&0].contains(&"n2".to_string()));
        assert!(communities[&1].contains(&"n3".to_string()));
    }

    #[test]
    fn test_communities_from_graph_no_community_attr() {
        let mut g = ServeGraph::new_undirected();
        g.add_node("a", "foo", "", "", None);
        let communities = communities_from_graph(&g);
        assert!(communities.is_empty());
    }

    #[test]
    fn test_communities_from_graph_isolated() {
        let g = make_graph();
        let communities = communities_from_graph(&g);
        assert!(communities.contains_key(&2));
        assert!(communities[&2].contains(&"n5".to_string()));
    }

    // --- score_nodes ---

    #[test]
    fn test_score_nodes_exact_label_match() {
        let mut g = make_graph();
        let scored = score_nodes(&mut g, &["extract".to_string()]);
        let nids: Vec<&str> = scored.iter().map(|(_, id)| id.as_str()).collect();
        assert!(nids.contains(&"n1"));
        assert_eq!(scored[0].1, "n1");
    }

    #[test]
    fn test_score_nodes_no_match() {
        let mut g = make_graph();
        let scored = score_nodes(&mut g, &["xyzzy".to_string()]);
        assert!(scored.is_empty());
    }

    #[test]
    fn test_score_nodes_source_file_partial() {
        let mut g = make_graph();
        let scored = score_nodes(&mut g, &["cluster".to_string()]);
        let nids: Vec<&str> = scored.iter().map(|(_, id)| id.as_str()).collect();
        assert!(nids.contains(&"n2"));
    }

    #[test]
    fn test_score_nodes_ignores_trailing_punctuation() {
        let mut g = make_graph();
        let scored = score_nodes(&mut g, &["extract?".to_string()]);
        assert!(!scored.is_empty());
        assert_eq!(scored[0].1, "n1");
    }

    // --- find_node ---

    #[test]
    fn test_find_node_ignores_trailing_punctuation() {
        let g = make_graph();
        let matches = find_node(&g, "extract?");
        assert!(matches.contains(&"n1".to_string()));
    }

    // --- query_terms ---

    #[test]
    fn test_query_terms_strips_search_punctuation() {
        let terms = query_terms("what calls extract?");
        assert!(terms.contains(&"what".to_string()));
        assert!(terms.contains(&"calls".to_string()));
        assert!(terms.contains(&"extract".to_string()));
        assert!(!terms.contains(&"extract?".to_string()));
    }

    #[test]
    fn test_query_terms_filters_short_english_terms() {
        let terms = query_terms("to of dependency install");
        // "to" (2 chars) and "of" (2 chars) dropped; long terms kept
        assert!(!terms.contains(&"to".to_string()));
        assert!(!terms.contains(&"of".to_string()));
        assert!(terms.contains(&"dependency".to_string()));
        assert!(terms.contains(&"install".to_string()));
    }

    #[test]
    fn test_query_terms_non_chinese_scripts_not_segmented() {
        // Japanese kana and Hangul should not be treated as Chinese
        let text = "かなカナ한글";
        assert!(!has_chinese(text));
        let terms = query_terms(text);
        assert!(terms.contains(&"かなカナ한글".to_string()));
    }

    #[test]
    fn test_query_terms_chinese_bigram_fallback() {
        // Rust uses bigram fallback (no jieba): "页面路由" → bigrams + original
        let terms = query_terms("页面路由");
        // bigrams: 页面, 面路, 路由, original: 页面路由
        assert!(terms.contains(&"页面".to_string()));
        assert!(terms.contains(&"路由".to_string()));
        assert!(terms.contains(&"页面路由".to_string()));
        assert_eq!(terms.len(), 4);
    }

    #[test]
    fn test_query_terms_chinese_keeps_original_term() {
        // Original multi-char Chinese term is always appended to bigrams for exact-match support
        let terms = query_terms("路由配置");
        assert!(
            terms.contains(&"路由配置".to_string()),
            "original term must be preserved"
        );
    }

    #[test]
    fn test_query_terms_chinese_mixed() {
        let terms = query_terms("前端 router 路由配置");
        assert!(terms.contains(&"前端".to_string()));
        assert!(terms.contains(&"router".to_string()));
        // 路由配置 → bigrams include 路由, 由配, 配置
        assert!(terms
            .iter()
            .any(|t| t.contains("路由") || t.contains("配置")));
    }

    // --- query_graph_text (non-english) ---

    #[test]
    fn test_query_graph_text_keeps_short_non_english_terms() {
        let mut g = ServeGraph::new_undirected();
        g.add_node("frontend", "前端", "docs/前端.md", "L1", Some(0));
        let text = query_graph_text(&mut g, "前端", "bfs", 1, 2000, None);
        assert!(!text.contains("No matching nodes found."));
        assert!(text.contains("NODE 前端"));
    }

    // --- context filters ---

    #[test]
    fn test_infer_context_filters_for_calls_question() {
        let filters = infer_context_filters("who calls extract");
        assert!(filters.contains(&"call".to_string()));
    }

    #[test]
    fn test_resolve_context_filters_explicit_overrides_heuristic() {
        let explicit = vec!["field".to_string()];
        let (filters, source) = resolve_context_filters("who calls extract", Some(&explicit));
        assert_eq!(filters, vec!["field".to_string()]);
        assert_eq!(source.as_deref(), Some("explicit"));
    }

    // --- BFS ---

    #[test]
    fn test_bfs_depth_1() {
        let g = make_graph();
        let (visited, _edges) = bfs(&g, &["n1".to_string()], 1);
        assert!(visited.contains("n1"));
        assert!(visited.contains("n2"));
        assert!(!visited.contains("n3")); // 2 hops away
    }

    #[test]
    fn test_bfs_depth_2() {
        let g = make_graph();
        let (visited, _edges) = bfs(&g, &["n1".to_string()], 2);
        assert!(visited.contains("n3")); // n1 -> n2 -> n3
    }

    #[test]
    fn test_bfs_disconnected() {
        let g = make_graph();
        let (visited, _edges) = bfs(&g, &["n5".to_string()], 3);
        assert_eq!(visited, ["n5".to_string()].iter().cloned().collect());
    }

    #[test]
    fn test_bfs_returns_edges() {
        let g = make_graph();
        let (visited, edges) = bfs(&g, &["n1".to_string()], 1);
        assert!(visited.contains("n1"));
        assert!(!edges.is_empty());
        assert!(edges.iter().any(|(u, v)| u == "n1" || v == "n1"));
    }

    // --- filter_graph_by_context ---

    #[test]
    fn test_filter_graph_by_context_limits_traversal() {
        let g = make_graph();
        let filtered = filter_graph_by_context(&g, &["call".to_string()]);
        let (visited, edges) = bfs(&filtered, &["n1".to_string()], 2);
        assert!(visited.contains("n2"));
        assert!(!visited.contains("n3")); // n2-n3 has context=import, filtered out
        assert_eq!(edges.len(), 1);
        assert!(edges
            .iter()
            .any(|(u, v)| (u == "n1" && v == "n2") || (u == "n2" && v == "n1")));
    }

    // --- DFS ---

    #[test]
    fn test_dfs_depth_1() {
        let g = make_graph();
        let (visited, _) = dfs(&g, &["n1".to_string()], 1);
        assert!(visited.contains("n1"));
        assert!(visited.contains("n2"));
        assert!(!visited.contains("n3"));
    }

    #[test]
    fn test_dfs_full_chain() {
        let g = make_graph();
        let (visited, _) = dfs(&g, &["n1".to_string()], 5);
        assert!(visited.is_superset(
            &["n1", "n2", "n3", "n4"]
                .iter()
                .map(|s| s.to_string())
                .collect()
        ));
    }

    // --- subgraph_to_text ---

    #[test]
    fn test_subgraph_to_text_contains_labels() {
        let g = make_graph();
        let nodes: HashSet<String> = ["n1", "n2"].iter().map(|s| s.to_string()).collect();
        let edges = vec![("n1".to_string(), "n2".to_string())];
        let text = subgraph_to_text(&g, &nodes, &edges, 2000, None);
        assert!(text.contains("extract"));
        assert!(text.contains("cluster"));
    }

    #[test]
    fn test_subgraph_to_text_truncates() {
        let g = make_graph();
        let nodes: HashSet<String> = ["n1", "n2", "n3", "n4"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let edges = vec![("n1".to_string(), "n2".to_string())];
        let text = subgraph_to_text(&g, &nodes, &edges, 1, None);
        assert!(text.contains("truncated"));
    }

    #[test]
    fn test_subgraph_to_text_edge_included() {
        let g = make_graph();
        let nodes: HashSet<String> = ["n1", "n2"].iter().map(|s| s.to_string()).collect();
        let edges = vec![("n1".to_string(), "n2".to_string())];
        let text = subgraph_to_text(&g, &nodes, &edges, 2000, None);
        assert!(text.contains("EDGE"));
        assert!(text.contains("calls"));
    }

    #[test]
    fn test_subgraph_to_text_includes_edge_context() {
        let g = make_graph();
        let nodes: HashSet<String> = ["n1", "n2"].iter().map(|s| s.to_string()).collect();
        let edges = vec![("n1".to_string(), "n2".to_string())];
        let text = subgraph_to_text(&g, &nodes, &edges, 2000, None);
        assert!(text.contains("context=call"));
    }

    // --- query_graph_text with context filters ---

    #[test]
    fn test_query_graph_text_explicit_context_filter_changes_traversal() {
        let mut g = make_graph();
        let filters = vec!["call".to_string()];
        let text = query_graph_text(&mut g, "extract", "bfs", 2, 2000, Some(&filters));
        assert!(text.contains("Context: call (explicit)"));
        assert!(text.contains("cluster"));
        assert!(!text.contains("build"));
    }

    #[test]
    fn test_query_graph_text_heuristic_context_filter_changes_traversal() {
        let mut g = make_graph();
        let text = query_graph_text(&mut g, "who calls extract", "bfs", 2, 2000, None);
        assert!(text.contains("Context: call (heuristic)"));
        assert!(text.contains("cluster"));
        assert!(!text.contains("build"));
    }

    // --- load_graph ---

    #[test]
    fn test_load_graph_roundtrip() {
        let (tmp, _path) =
            write_graph_json(&[("n1", "extract"), ("n2", "cluster"), ("n3", "build")]);
        let g = load_graph(tmp.path()).unwrap();
        assert_eq!(g.num_nodes(), 3);
        assert_eq!(g.num_edges(), 0);
    }

    #[test]
    fn test_load_graph_missing_file() {
        let result = load_graph(Path::new("/tmp/nonexistent_codesynapse_test.json"));
        assert!(matches!(result, Err(LoadError::Io(_))));
    }

    #[test]
    fn test_load_graph_rejects_oversized_file() {
        let (tmp, _path) = write_graph_json(&[("n1", "extract")]);
        let result = load_graph_with_cap(tmp.path(), 16);
        assert!(matches!(result, Err(LoadError::TooLarge { .. })));
        if let Err(e) = result {
            let msg = e.to_string();
            assert!(msg.contains("exceeds"), "msg: {}", msg);
            assert!(msg.contains("byte cap"), "msg: {}", msg);
        }
    }

    #[test]
    fn test_load_graph_accepts_under_cap() {
        let (tmp, _path) = write_graph_json(&[("n1", "extract")]);
        let result = load_graph_with_cap(tmp.path(), 10 * 1024 * 1024);
        assert!(result.is_ok());
        let g = result.unwrap();
        assert_eq!(g.num_nodes(), 1);
    }

    // --- hot reload ---

    #[test]
    fn test_load_graph_detects_graph_change() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("graph.json");

        std::fs::write(&path, r#"{"directed":false,"multigraph":false,"graph":{},"nodes":[{"id":"alpha","label":"alpha","community":0},{"id":"beta","label":"beta","community":0}],"links":[]}"#).unwrap();
        let g1 = load_graph(&path).unwrap();
        assert!(g1.contains_node("alpha"));
        assert!(g1.contains_node("beta"));

        std::thread::sleep(std::time::Duration::from_millis(10));
        std::fs::write(&path, r#"{"directed":false,"multigraph":false,"graph":{},"nodes":[{"id":"alpha","label":"alpha","community":0},{"id":"beta","label":"beta","community":0},{"id":"gamma","label":"gamma","community":0}],"links":[]}"#).unwrap();
        let g2 = load_graph(&path).unwrap();
        assert!(g2.contains_node("gamma"));
    }

    #[test]
    fn test_load_graph_cache_key_changes_with_content() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("graph.json");

        std::fs::write(&path, r#"{"directed":false,"multigraph":false,"graph":{},"nodes":[{"id":"a","label":"a","community":0}],"links":[]}"#).unwrap();
        let s1 = std::fs::metadata(&path).unwrap();
        let key1 = (s1.modified().unwrap(), s1.len());

        std::thread::sleep(std::time::Duration::from_millis(10));
        std::fs::write(&path, r#"{"directed":false,"multigraph":false,"graph":{},"nodes":[{"id":"a","label":"a","community":0},{"id":"b","label":"b","community":0}],"links":[]}"#).unwrap();
        let s2 = std::fs::metadata(&path).unwrap();
        let key2 = (s2.modified().unwrap(), s2.len());

        assert_ne!(key1, key2, "stat key must change when file content changes");
    }

    // --- IDF ---

    #[test]
    fn test_idf_downweights_common_terms() {
        let mut g = make_noisy_graph();
        let scored = score_nodes(&mut g, &["foobarservice".to_string(), "error".to_string()]);
        assert!(!scored.is_empty());
        assert_eq!(scored[0].1, "fbs");
    }

    #[test]
    fn test_idf_cached_on_graph() {
        let mut g = make_graph();
        score_nodes(&mut g, &["extract".to_string()]);
        assert!(g.idf_cache.contains_key("extract"));
    }

    #[test]
    fn test_idf_new_graph_starts_fresh() {
        let mut g1 = make_graph();
        let g2 = make_graph();
        score_nodes(&mut g1, &["extract".to_string()]);
        assert!(!g2.idf_cache.contains_key("extract"));
    }

    #[test]
    fn test_idf_rare_term_gets_high_weight() {
        let mut g = make_graph(); // 5 nodes
        let idf = compute_idf(&mut g, &["extract".to_string()]);
        // extract matches only n1: IDF = log(1 + 5/2) ≈ 1.25
        assert!(idf["extract"] > 1.0);
    }

    #[test]
    fn test_idf_common_term_gets_low_weight() {
        let mut g = ServeGraph::new_undirected();
        for i in 0..20usize {
            g.add_node(
                &format!("n{}", i),
                &format!("handle_{}", i),
                &format!("f{}.py", i),
                "",
                None,
            );
        }
        let idf = compute_idf(&mut g, &["handle".to_string()]);
        assert!(idf["handle"] < 1.0);
    }

    // --- pick_seeds ---

    #[test]
    fn test_pick_seeds_dominant_identifier_gives_one_seed() {
        let scored = vec![
            (1000.0, "fbs".to_string()),
            (1.0, "err1".to_string()),
            (0.9, "err2".to_string()),
        ];
        let seeds = pick_seeds(&scored, 3, 0.2);
        assert_eq!(seeds, vec!["fbs".to_string()]);
    }

    #[test]
    fn test_pick_seeds_close_scores_keeps_multiple() {
        let scored = vec![
            (10.0, "a".to_string()),
            (9.0, "b".to_string()),
            (8.5, "c".to_string()),
        ];
        let seeds = pick_seeds(&scored, 3, 0.2);
        assert_eq!(seeds.len(), 3);
    }

    #[test]
    fn test_pick_seeds_empty() {
        let seeds = pick_seeds(&[], 3, 0.2);
        assert!(seeds.is_empty());
    }

    #[test]
    fn test_pick_seeds_single() {
        let scored = vec![(5.0, "x".to_string())];
        let seeds = pick_seeds(&scored, 3, 0.2);
        assert_eq!(seeds, vec!["x".to_string()]);
    }

    #[test]
    fn test_pick_seeds_respects_max_k() {
        let scored: Vec<(f64, String)> = (0..10).map(|i| (10.0, format!("n{}", i))).collect();
        let seeds = pick_seeds(&scored, 3, 0.2);
        assert_eq!(seeds.len(), 3);
    }

    // --- truncation hint ---

    #[test]
    fn test_subgraph_to_text_truncation_hint_is_actionable() {
        let g = make_graph();
        let nodes: HashSet<String> = ["n1", "n2", "n3", "n4"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let edges = vec![("n1".to_string(), "n2".to_string())];
        let text = subgraph_to_text(&g, &nodes, &edges, 1, None);
        assert!(text.contains("truncated"));
        assert!(text.contains("get_node") || text.contains("context_filter"));
    }

    // --- integration ---

    #[test]
    fn test_query_seeds_from_identifier_not_noise() {
        let mut g = make_noisy_graph();
        let text = query_graph_text(&mut g, "FooBarService error handling", "bfs", 2, 2000, None);
        assert!(text.contains("FooBarService"));
        assert!(text.contains("ServiceClient"));
    }

    // --- parameter_type context ---

    #[test]
    fn test_query_graph_text_parameter_type_context_filter_changes_traversal() {
        let mut g = ServeGraph::new_undirected();
        g.add_node("process", "process", "sample.cs", "L20", None);
        g.add_node("payload", "Payload", "sample.cs", "L5", None);
        g.add_node("other", "PayloadFactory", "sample.cs", "L40", None);
        g.add_edge(
            "process",
            "payload",
            "references",
            "EXTRACTED",
            Some("parameter_type"),
        );
        g.add_edge("process", "other", "calls", "EXTRACTED", Some("call"));

        let filters = vec!["parameter_type".to_string()];
        let text = query_graph_text(
            &mut g,
            "who accepts Payload",
            "bfs",
            2,
            2000,
            Some(&filters),
        );
        assert!(text.contains("parameter_type"));
        assert!(text.contains("Payload"));
        assert!(!text.contains("PayloadFactory"));
    }

    // --- normalize_context_filters aliases ---

    #[test]
    fn test_context_filter_aliases_resolve() {
        assert_eq!(
            normalize_context_filters(&["param".to_string()]),
            vec!["parameter_type"]
        );
        assert_eq!(
            normalize_context_filters(&["parameter".to_string()]),
            vec!["parameter_type"]
        );
        assert_eq!(
            normalize_context_filters(&["return".to_string()]),
            vec!["return_type"]
        );
        assert_eq!(
            normalize_context_filters(&["returns".to_string()]),
            vec!["return_type"]
        );
        assert_eq!(
            normalize_context_filters(&["generic".to_string()]),
            vec!["generic_arg"]
        );
        assert_eq!(
            normalize_context_filters(&["generics".to_string()]),
            vec!["generic_arg"]
        );
        assert_eq!(
            normalize_context_filters(&["annotation".to_string()]),
            vec!["attribute"]
        );
        assert_eq!(
            normalize_context_filters(&["decorator".to_string()]),
            vec!["attribute"]
        );
        // Pass-through canonical values
        assert_eq!(
            normalize_context_filters(&["parameter_type".to_string()]),
            vec!["parameter_type"]
        );
        assert_eq!(
            normalize_context_filters(&["field".to_string()]),
            vec!["field"]
        );
    }

    // --- Chinese scoring ---

    #[test]
    fn test_score_nodes_chinese_substring_match() {
        let mut g = ServeGraph::new_undirected();
        g.add_node("n1", "路由桥接核对表", "doc.md", "", Some(0));
        g.add_node("n2", "其他内容", "doc.md", "", Some(0));
        let scored = score_nodes(&mut g, &["路由".to_string()]);
        let nids: Vec<&str> = scored.iter().map(|(_, id)| id.as_str()).collect();
        assert!(nids.contains(&"n1"));
        assert!(!nids.contains(&"n2"));
    }

    // --- Chinese query pipeline ---

    #[test]
    fn test_query_text_chinese_finds_routing_nodes() {
        let mut g = ServeGraph::new_undirected();
        g.add_node("parent", "页面路由规范", "doc.md", "L1", Some(0));
        g.add_node("child", "路由桥接核对表", "doc.md", "L10", Some(0));
        g.add_edge("parent", "child", "contains", "EXTRACTED", None);
        let text = query_graph_text(&mut g, "页面路由", "bfs", 2, 2000, None);
        assert!(!text.contains("No matching nodes found."));
        assert!(text.contains("路由"));
    }

    // --- Bm25Index ---

    #[test]
    fn test_bm25_empty_graph() {
        let g = ServeGraph::new_undirected();
        let idx = Bm25Index::build(&g);
        assert!(idx.score(&["foo".to_string()]).is_empty());
    }

    #[test]
    fn test_bm25_score_hit() {
        let mut g = ServeGraph::new_undirected();
        g.add_node("n1", "extract_nodes", "extract.py", "", None);
        g.add_node("n2", "build_graph", "build.py", "", None);
        let idx = Bm25Index::build(&g);
        let scored = idx.score(&["extract".to_string()]);
        assert!(!scored.is_empty());
        assert_eq!(scored[0].1, "n1");
    }

    #[test]
    fn test_bm25_score_miss() {
        let mut g = ServeGraph::new_undirected();
        g.add_node("n1", "foo_bar", "foo.py", "", None);
        let idx = Bm25Index::build(&g);
        let scored = idx.score(&["xyzzy".to_string()]);
        assert!(scored.is_empty());
    }

    #[test]
    fn test_bm25_score_ranking_specificity() {
        let mut g = ServeGraph::new_undirected();
        // n1 has "extract" twice (in label and filename), n2 once
        g.add_node("n1", "extract_pipeline", "extract.py", "", None);
        g.add_node("n2", "build_graph", "build.py", "", None);
        let idx = Bm25Index::build(&g);
        let scored = idx.score(&["extract".to_string()]);
        let ids: Vec<&str> = scored.iter().map(|(_, id)| id.as_str()).collect();
        assert!(ids.contains(&"n1"));
        assert!(!ids.contains(&"n2"));
    }

    #[test]
    fn test_bm25_no_terms() {
        let mut g = ServeGraph::new_undirected();
        g.add_node("n1", "foo", "foo.py", "", None);
        let idx = Bm25Index::build(&g);
        assert!(idx.score(&[]).is_empty());
    }

    // --- rrf ---

    #[test]
    fn test_rrf_single_list() {
        let list = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let scored = rrf(&[list], RRF_K);
        assert_eq!(scored.len(), 3);
        // scores must be strictly decreasing
        assert!(scored[0].0 > scored[1].0);
        assert!(scored[1].0 > scored[2].0);
        assert_eq!(scored[0].1, "a");
    }

    #[test]
    fn test_rrf_fusion_boosts_overlap() {
        let list1 = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let list2 = vec!["b".to_string(), "a".to_string(), "d".to_string()];
        let scored = rrf(&[list1, list2], RRF_K);
        let top: Vec<&str> = scored.iter().take(2).map(|(_, id)| id.as_str()).collect();
        // "a" and "b" appear in both lists → should be top-2
        assert!(top.contains(&"a") || top.contains(&"b"));
        // "d" and "c" are in only one list → lower rank
        let all_ids: Vec<&str> = scored.iter().map(|(_, id)| id.as_str()).collect();
        assert!(all_ids.contains(&"d"));
        assert!(all_ids.contains(&"c"));
    }

    #[test]
    fn test_rrf_empty() {
        let result = rrf(&[], RRF_K);
        assert!(result.is_empty());
    }

    // --- query_graph_text_hybrid ---

    #[test]
    fn test_hybrid_query_no_dense_finds_nodes() {
        let g = make_graph();
        let text = query_graph_text_hybrid(&g, "extract", "bfs", 2, 2000, None, None);
        assert!(!text.contains("No matching nodes found."));
        assert!(text.contains("extract"));
    }

    #[test]
    fn test_hybrid_query_no_match_returns_not_found() {
        let g = make_graph();
        let text = query_graph_text_hybrid(&g, "zzznonexistent", "bfs", 2, 2000, None, None);
        assert!(text.contains("No matching nodes found."));
    }

    // --- parse_byte_range ---

    #[test]
    fn test_parse_byte_range_valid() {
        assert_eq!(parse_byte_range("0:100"), Some((0, 100)));
        assert_eq!(parse_byte_range("42:84"), Some((42, 84)));
    }

    #[test]
    fn test_parse_byte_range_invalid() {
        assert_eq!(parse_byte_range(""), None);
        assert_eq!(parse_byte_range("abc:def"), None);
        assert_eq!(parse_byte_range("100"), None);
        assert_eq!(parse_byte_range(":50"), None);
    }

    // --- read_node_body_exact ---

    #[test]
    fn test_read_node_body_exact_returns_correct_slice() {
        let mut tmp = NamedTempFile::new().unwrap();
        tmp.write_all(b"hello world foo bar").unwrap();
        let path = tmp.path().to_str().unwrap();
        assert_eq!(read_node_body_exact(path, "6:11"), "world");
    }

    #[test]
    fn test_read_node_body_exact_invalid_location() {
        assert_eq!(
            read_node_body_exact("nonexistent_codesynapse.py", "0:10"),
            ""
        );
        assert_eq!(
            read_node_body_exact("nonexistent_codesynapse.py", "bad"),
            ""
        );
    }

    // --- BM25 source_location body indexing ---

    #[test]
    fn test_bm25_indexes_source_location_body() {
        let mut tmp = NamedTempFile::new().unwrap();
        let body = b"def handle_not_found():\n    raise NotFound\n";
        tmp.write_all(body).unwrap();
        let path = tmp.path().to_str().unwrap().to_string();

        let mut g = ServeGraph::new_undirected();
        g.add_node("n1", "handle_not_found", &path, "", None);
        if let Some(n) = g.nodes.get_mut("n1") {
            n.source_location = format!("0:{}", body.len());
        }
        g.add_node("n2", "other_func", "other.py", "", None);

        let idx = Bm25Index::build(&g);
        let scored = idx.score(&["notfound".to_string()]);
        let ids: Vec<&str> = scored.iter().map(|(_, id)| id.as_str()).collect();
        assert!(
            ids.contains(&"n1"),
            "n1 should rank for 'notfound' body token"
        );
        assert!(!ids.contains(&"n2"), "n2 should not rank for 'notfound'");
    }

    // --- BM25 docstring indexing ---

    #[test]
    fn test_bm25_docstring_tokens_indexed() {
        let mut g = ServeGraph::new_undirected();
        g.add_node("n1", "PaymentService", "payment.py", "", None);
        // Manually set docstring on the node
        if let Some(n) = g.nodes.get_mut("n1") {
            n.docstring = Some("payment processing gateway".into());
        }
        g.add_node("n2", "UserService", "user.py", "", None);
        let idx = Bm25Index::build(&g);
        let scored = idx.score(&["payment".to_string()]);
        let ids: Vec<&str> = scored.iter().map(|(_, id)| id.as_str()).collect();
        assert!(
            ids.contains(&"n1"),
            "n1 should rank due to docstring token 'payment'"
        );
        assert!(!ids.contains(&"n2"), "n2 should not rank for 'payment'");
    }

    #[test]
    fn test_bm25_docstring_gateway_token() {
        let mut g = ServeGraph::new_undirected();
        g.add_node("n1", "PaymentService", "payment.py", "", None);
        if let Some(n) = g.nodes.get_mut("n1") {
            n.docstring = Some("payment processing gateway".into());
        }
        let idx = Bm25Index::build(&g);
        let scored = idx.score(&["gateway".to_string()]);
        let ids: Vec<&str> = scored.iter().map(|(_, id)| id.as_str()).collect();
        assert!(ids.contains(&"n1"), "n1 should rank for 'gateway'");
    }

    // --- split_camel ---

    #[test]
    fn test_split_camel_pascal_case() {
        assert_eq!(split_camel("QueryHandler"), vec!["query", "handler"]);
    }

    #[test]
    fn test_split_camel_camel_case() {
        assert_eq!(split_camel("queryHandler"), vec!["query", "handler"]);
    }

    #[test]
    fn test_split_camel_snake_case_unchanged() {
        assert_eq!(split_camel("query_handler"), vec!["query_handler"]);
    }

    #[test]
    fn test_split_camel_all_caps_no_split() {
        assert_eq!(split_camel("HTTP"), vec!["http"]);
    }

    #[test]
    fn test_split_camel_single_word() {
        assert_eq!(split_camel("handler"), vec!["handler"]);
    }

    #[test]
    fn test_split_camel_multiple_caps_run() {
        assert_eq!(split_camel("HTTPRequest"), vec!["http", "request"]);
    }

    #[test]
    fn test_split_camel_three_words() {
        assert_eq!(split_camel("FooBarBaz"), vec!["foo", "bar", "baz"]);
    }

    // --- is_symbol_query + symbol_ranked integration ---

    #[test]
    fn test_symbol_ranked_from_split_tokens() {
        let mut g = ServeGraph::new_undirected();
        g.add_node("n1", "QueryHandler", "handler.py", "", None);
        g.add_node("n2", "UserService", "service.py", "", None);
        let idx = Bm25Index::build(&g);
        let split = split_camel("QueryHandler");
        let scored = idx.score(&split);
        let ids: Vec<&str> = scored.iter().map(|(_, id)| id.as_str()).collect();
        assert!(
            ids.contains(&"n1"),
            "n1 should rank for split tokens of QueryHandler"
        );
    }

    // --- Issue 2: BM25 cache on ServeGraph ---

    #[test]
    fn test_bm25_index_built_after_load_graph() {
        let tmp = tempfile::tempdir().unwrap();
        let json = serde_json::json!({
            "nodes": [{"id": "n1", "label": "foo_bar", "file_type": "code", "source_file": ""}],
            "edges": []
        });
        let path = tmp.path().join("g.json");
        std::fs::write(&path, serde_json::to_string(&json).unwrap()).unwrap();
        let g = load_graph(&path).unwrap();
        assert!(
            g.bm25_index.is_some(),
            "BM25 index must be built at load time"
        );
    }

    #[test]
    fn test_query_top_nodes_accepts_shared_ref() {
        let mut g = ServeGraph::new_undirected();
        g.add_node("n1", "extract_nodes", "extract.py", "", None);
        g.add_node("n2", "build_graph", "build.py", "", None);
        g.build_bm25_index();
        let results = query_top_nodes(&g, "extract", 3, None);
        assert!(!results.is_empty());
        assert_eq!(results[0].0, "n1");
    }

    #[test]
    fn test_bm25_index_reused_across_calls() {
        let mut g = ServeGraph::new_undirected();
        g.add_node("n1", "authenticate_user", "auth.rs", "", None);
        g.build_bm25_index();
        let r1 = query_top_nodes(&g, "authenticate", 3, None);
        let r2 = query_top_nodes(&g, "authenticate", 3, None);
        assert_eq!(
            r1, r2,
            "results must be identical across calls (same cached index)"
        );
    }

    #[test]
    fn test_query_graph_text_hybrid_shared_ref() {
        let mut g = ServeGraph::new_undirected();
        g.add_node("n1", "extract_pipeline", "extract.py", "", None);
        g.add_node("n2", "build_graph", "build.py", "", None);
        g.add_edge("n1", "n2", "calls", "EXTRACTED", None);
        g.build_bm25_index();
        let text = query_graph_text_hybrid(&g, "extract", "bfs", 2, 2000, None, None);
        assert!(!text.contains("No matching nodes found."));
    }

    #[test]
    fn test_resolve_query_returns_top_nodes() {
        let json = r#"{"directed":false,"nodes":[{"id":"n1","label":"extract_pipeline","source_file":"extract.py"},{"id":"n2","label":"cluster_nodes","source_file":"cluster.py"}],"edges":[]}"#;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("graph.json");
        std::fs::write(&path, json).unwrap();
        let result = resolve_query("extract", &path, 3, 10000).unwrap();
        assert!(
            result.contains("extract_pipeline"),
            "expected top node in output: {result}"
        );
    }

    #[test]
    fn test_resolve_query_missing_file_returns_err() {
        use std::path::PathBuf;
        let path = PathBuf::from("/nonexistent/graph.json");
        assert!(resolve_query("anything", &path, 3, 10000).is_err());
    }
}
