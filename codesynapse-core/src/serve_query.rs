use crate::security::{sanitize_label, MAX_GRAPH_FILE_BYTES};
use std::collections::{HashMap, HashSet};

// ---- Data structures ----

#[derive(Clone, Debug)]
pub struct NodeData {
    pub label: String,
    pub source_file: String,
    pub source_location: String,
    pub community: Option<i64>,
}

#[derive(Clone, Debug)]
pub struct EdgeData {
    pub relation: String,
    pub confidence: String,
    pub context: Option<String>,
}

#[derive(Debug)]
pub struct QueryGraph {
    pub nodes: HashMap<String, NodeData>,
    adj: HashMap<String, Vec<(String, EdgeData)>>,
    pub idf_cache: HashMap<String, f64>,
}

impl Default for QueryGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl QueryGraph {
    pub fn new() -> Self {
        QueryGraph {
            nodes: HashMap::new(),
            adj: HashMap::new(),
            idf_cache: HashMap::new(),
        }
    }

    pub fn add_node(&mut self, id: impl Into<String>, data: NodeData) {
        let id = id.into();
        self.adj.entry(id.clone()).or_default();
        self.nodes.insert(id, data);
    }

    pub fn add_edge(&mut self, u: impl Into<String>, v: impl Into<String>, data: EdgeData) {
        let u: String = u.into();
        let v: String = v.into();
        let reverse = EdgeData {
            relation: data.relation.clone(),
            confidence: data.confidence.clone(),
            context: data.context.clone(),
        };
        self.adj
            .entry(u.clone())
            .or_default()
            .push((v.clone(), data));
        self.adj.entry(v.clone()).or_default().push((u, reverse));
        self.nodes.entry(v).or_insert_with(|| NodeData {
            label: String::new(),
            source_file: String::new(),
            source_location: String::new(),
            community: None,
        });
    }

    pub fn neighbors(&self, id: &str) -> Vec<(&str, &EdgeData)> {
        self.adj
            .get(id)
            .map(|v| v.iter().map(|(n, e)| (n.as_str(), e)).collect())
            .unwrap_or_default()
    }

    pub fn degree(&self, id: &str) -> usize {
        self.adj.get(id).map(|v| v.len()).unwrap_or(0)
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    fn all_degrees(&self) -> Vec<usize> {
        self.nodes.keys().map(|id| self.degree(id)).collect()
    }

    fn hub_threshold(&self) -> usize {
        let mut degrees = self.all_degrees();
        if degrees.is_empty() {
            return 50;
        }
        degrees.sort_unstable();
        let p99_idx = (degrees.len() as f64 * 0.99) as usize;
        let p99_idx = p99_idx.min(degrees.len() - 1);
        degrees[p99_idx].max(50)
    }
}

// ---- Text processing ----

pub fn has_chinese(text: &str) -> bool {
    text.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c))
}

fn search_tokens(text: &str) -> Vec<String> {
    let lower = text.to_lowercase();
    let mut tokens = Vec::new();
    let mut current = String::new();
    for c in lower.chars() {
        if c.is_alphanumeric() || c == '_' {
            current.push(c);
        } else if !current.is_empty() {
            tokens.push(current.clone());
            current.clear();
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

fn is_searchable(term: &str) -> bool {
    let all_ascii_alpha = term.chars().all(|c| c.is_ascii_lowercase());
    if all_ascii_alpha {
        term.len() > 2
    } else {
        true
    }
}

fn segment_chinese_bigrams(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut segments: Vec<String> = chars
        .windows(2)
        .map(|w| w.iter().collect::<String>())
        .collect();
    if segments.is_empty() && !chars.is_empty() {
        segments.push(text.to_string());
    }
    if text.len() > 1 && !segments.contains(&text.to_string()) {
        segments.push(text.to_string());
    }
    segments
}

pub fn query_terms(question: &str) -> Vec<String> {
    let mut terms: Vec<String> = Vec::new();
    for raw in question.split_whitespace() {
        if has_chinese(raw) {
            let lower = raw.to_lowercase();
            let lower = lower.trim();
            for seg in segment_chinese_bigrams(lower) {
                let seg = seg.trim().to_string();
                if !seg.is_empty() && is_searchable(&seg) {
                    terms.push(seg);
                }
            }
        } else {
            for tok in search_tokens(raw) {
                if is_searchable(&tok) {
                    terms.push(tok);
                }
            }
        }
    }
    terms
}

// ---- IDF ----

const EXACT_MATCH_BONUS: f64 = 1000.0;
const PREFIX_MATCH_BONUS: f64 = 100.0;
const SUBSTRING_MATCH_BONUS: f64 = 1.0;
const SOURCE_MATCH_BONUS: f64 = 0.5;

pub fn compute_idf(g: &mut QueryGraph, terms: &[String]) -> HashMap<String, f64> {
    let n = g.node_count().max(1) as f64;
    let uncached: Vec<String> = terms
        .iter()
        .filter(|t| !g.idf_cache.contains_key(*t))
        .cloned()
        .collect();

    if !uncached.is_empty() {
        let mut df: HashMap<String, usize> = uncached.iter().map(|t| (t.clone(), 0)).collect();
        for data in g.nodes.values() {
            let norm_label = data.label.to_lowercase();
            for t in &uncached {
                if norm_label.contains(t.as_str()) {
                    *df.get_mut(t).unwrap() += 1;
                }
            }
        }
        for t in &uncached {
            let d = *df.get(t).unwrap_or(&0) as f64;
            g.idf_cache.insert(t.clone(), (1.0 + n / (1.0 + d)).ln());
        }
    }

    terms
        .iter()
        .map(|t| {
            let w = g
                .idf_cache
                .get(t)
                .copied()
                .unwrap_or_else(|| (1.0 + n).ln());
            (t.clone(), w)
        })
        .collect()
}

pub fn score_nodes(g: &mut QueryGraph, terms: &[String]) -> Vec<(f64, String)> {
    let norm_terms: Vec<String> = terms.iter().flat_map(|t| search_tokens(t)).collect();
    if norm_terms.is_empty() {
        return vec![];
    }
    let idf = compute_idf(g, &norm_terms);

    let node_ids: Vec<String> = g.nodes.keys().cloned().collect();
    let mut scored: Vec<(f64, String)> = Vec::new();
    for nid in node_ids {
        let data = g.nodes.get(&nid).unwrap();
        let norm_label = data.label.to_lowercase();
        let bare_label = norm_label.trim_end_matches(['(', ')']).to_string();
        let source = data.source_file.to_lowercase();
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
            scored.push((score, nid));
        }
    }
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    scored
}

pub fn pick_seeds(scored: &[(f64, String)], max_k: usize, gap_ratio: f64) -> Vec<String> {
    if scored.is_empty() {
        return vec![];
    }
    let top_score = scored[0].0;
    let mut seeds = Vec::new();
    for (score, nid) in scored.iter().take(max_k) {
        if !seeds.is_empty() && *score < top_score * gap_ratio {
            break;
        }
        seeds.push(nid.clone());
    }
    seeds
}

// ---- Context filters ----

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

fn context_alias(key: &str) -> &str {
    match key {
        "param" | "params" | "parameter" | "parameters" | "argument" | "arguments" | "arg"
        | "args" => "parameter_type",
        "return" | "returns" | "returned" => "return_type",
        "generic" | "generics" | "template" | "templates" => "generic_arg",
        "annotation" | "annotations" | "decorator" | "decorators" => "attribute",
        "calls" | "called" | "invoke" | "invocation" => "call",
        "fields" | "property" | "properties" | "member" | "members" => "field",
        "imports" | "imported" | "module" | "modules" => "import",
        "exports" | "exported" => "export",
        other => other,
    }
}

pub fn normalize_context_filters(filters: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    for v in filters {
        let key = v.trim().to_lowercase();
        if key.is_empty() {
            continue;
        }
        let canonical = context_alias(&key).to_string();
        if seen.insert(canonical.clone()) {
            result.push(canonical);
        }
    }
    result
}

pub fn infer_context_filters(question: &str) -> Vec<String> {
    let lowered: HashSet<String> = question
        .replace(['?', ','], " ")
        .split_whitespace()
        .map(|tok| tok.to_lowercase())
        .collect();
    let mut inferred = Vec::new();
    for (context, hints) in CONTEXT_HINTS {
        if hints.iter().any(|h| lowered.contains(*h)) {
            inferred.push(context.to_string());
        }
    }
    inferred
}

pub fn resolve_context_filters(
    question: &str,
    explicit_filters: &[String],
) -> (Vec<String>, Option<String>) {
    let normalized = normalize_context_filters(explicit_filters);
    if !normalized.is_empty() {
        return (normalized, Some("explicit".to_string()));
    }
    let inferred = infer_context_filters(question);
    if !inferred.is_empty() {
        return (inferred, Some("heuristic".to_string()));
    }
    (vec![], None)
}

pub fn filter_graph_by_context(g: &QueryGraph, filters: &[String]) -> QueryGraph {
    let filter_set: HashSet<String> = normalize_context_filters(filters).into_iter().collect();
    if filter_set.is_empty() {
        // return a copy
        let mut h = QueryGraph::new();
        for (id, data) in &g.nodes {
            h.add_node(id.clone(), data.clone());
        }
        for (u, neighbors) in &g.adj {
            for (v, edge) in neighbors {
                // avoid double-insertion: only add when u < v (lexicographic)
                if u < v {
                    h.adj
                        .entry(u.clone())
                        .or_default()
                        .push((v.clone(), edge.clone()));
                    h.adj.entry(v.clone()).or_default().push((
                        u.clone(),
                        EdgeData {
                            relation: edge.relation.clone(),
                            confidence: edge.confidence.clone(),
                            context: edge.context.clone(),
                        },
                    ));
                }
            }
        }
        return h;
    }

    let mut h = QueryGraph::new();
    for (id, data) in &g.nodes {
        h.add_node(id.clone(), data.clone());
    }
    let mut added: HashSet<(String, String)> = HashSet::new();
    for (u, neighbors) in &g.adj {
        for (v, edge) in neighbors {
            let matches = edge
                .context
                .as_deref()
                .map(|c| filter_set.contains(c))
                .unwrap_or(false);
            if matches {
                let key = if u <= v {
                    (u.clone(), v.clone())
                } else {
                    (v.clone(), u.clone())
                };
                if added.insert(key) {
                    h.adj
                        .entry(u.clone())
                        .or_default()
                        .push((v.clone(), edge.clone()));
                    h.adj.entry(v.clone()).or_default().push((
                        u.clone(),
                        EdgeData {
                            relation: edge.relation.clone(),
                            confidence: edge.confidence.clone(),
                            context: edge.context.clone(),
                        },
                    ));
                }
            }
        }
    }
    h
}

// ---- BFS / DFS ----

pub fn bfs(
    g: &QueryGraph,
    start_nodes: &[String],
    depth: usize,
) -> (HashSet<String>, Vec<(String, String)>) {
    let hub_threshold = g.hub_threshold();
    let seed_set: HashSet<&str> = start_nodes.iter().map(|s| s.as_str()).collect();
    let mut visited: HashSet<String> = start_nodes.iter().cloned().collect();
    let mut frontier: HashSet<String> = start_nodes.iter().cloned().collect();
    let mut edges_seen: Vec<(String, String)> = Vec::new();

    for _ in 0..depth {
        let mut next_frontier = HashSet::new();
        for n in &frontier {
            if !seed_set.contains(n.as_str()) && g.degree(n) >= hub_threshold {
                continue;
            }
            for (neighbor, _) in g.neighbors(n) {
                if !visited.contains(neighbor) {
                    next_frontier.insert(neighbor.to_string());
                    edges_seen.push((n.clone(), neighbor.to_string()));
                }
            }
        }
        visited.extend(next_frontier.iter().cloned());
        frontier = next_frontier;
        if frontier.is_empty() {
            break;
        }
    }
    (visited, edges_seen)
}

pub fn dfs(
    g: &QueryGraph,
    start_nodes: &[String],
    depth: usize,
) -> (HashSet<String>, Vec<(String, String)>) {
    let hub_threshold = g.hub_threshold();
    let seed_set: HashSet<&str> = start_nodes.iter().map(|s| s.as_str()).collect();
    let mut visited: HashSet<String> = HashSet::new();
    let mut edges_seen: Vec<(String, String)> = Vec::new();
    let mut stack: Vec<(String, usize)> =
        start_nodes.iter().rev().map(|n| (n.clone(), 0)).collect();

    while let Some((node, d)) = stack.pop() {
        if visited.contains(&node) || d > depth {
            continue;
        }
        visited.insert(node.clone());
        if !seed_set.contains(node.as_str()) && g.degree(&node) >= hub_threshold {
            continue;
        }
        for (neighbor, _) in g.neighbors(&node) {
            if !visited.contains(neighbor) {
                stack.push((neighbor.to_string(), d + 1));
                edges_seen.push((node.clone(), neighbor.to_string()));
            }
        }
    }
    (visited, edges_seen)
}

// ---- Subgraph to text ----

pub fn subgraph_to_text(
    g: &QueryGraph,
    nodes: &HashSet<String>,
    edges: &[(String, String)],
    token_budget: usize,
    seeds: Option<&[String]>,
) -> String {
    let char_budget = token_budget * 3;
    let mut lines: Vec<String> = Vec::new();
    let _seed_set: HashSet<&str> = seeds.unwrap_or(&[]).iter().map(|s| s.as_str()).collect();

    let mut ordered: Vec<String> = seeds
        .unwrap_or(&[])
        .iter()
        .filter(|n| nodes.contains(*n))
        .cloned()
        .collect();
    let rest_set: HashSet<&str> = ordered.iter().map(|s| s.as_str()).collect();
    let mut rest: Vec<String> = nodes
        .iter()
        .filter(|n| !rest_set.contains(n.as_str()))
        .cloned()
        .collect();
    rest.sort_by_key(|n| std::cmp::Reverse(g.degree(n)));
    ordered.extend(rest);

    for nid in &ordered {
        let d = g.nodes.get(nid.as_str());
        let (label, src, loc, community) = match d {
            Some(nd) => (
                nd.label.as_str(),
                nd.source_file.as_str(),
                nd.source_location.as_str(),
                nd.community.map(|c| c.to_string()),
            ),
            None => (nid.as_str(), "", "", None),
        };
        let line = format!(
            "NODE {} [src={} loc={} community={}]",
            sanitize_label(Some(label)),
            sanitize_label(Some(src)),
            sanitize_label(Some(loc)),
            sanitize_label(community.as_deref()),
        );
        lines.push(line);
    }

    for (u, v) in edges {
        if nodes.contains(u) && nodes.contains(v) {
            let edge = g
                .adj
                .get(u)
                .and_then(|adj| adj.iter().find(|(n, _)| n == v))
                .map(|(_, e)| e);
            let (relation, confidence, context) = match edge {
                Some(e) => (
                    e.relation.as_str(),
                    e.confidence.as_str(),
                    e.context.as_deref(),
                ),
                None => ("", "", None),
            };
            let context_suffix = if let Some(ctx) = context {
                format!(" context={}", sanitize_label(Some(ctx)))
            } else {
                String::new()
            };
            let u_label = g
                .nodes
                .get(u)
                .map(|d| d.label.as_str())
                .unwrap_or(u.as_str());
            let v_label = g
                .nodes
                .get(v)
                .map(|d| d.label.as_str())
                .unwrap_or(v.as_str());
            let line = format!(
                "EDGE {} --{} [{}{}]--> {}",
                sanitize_label(Some(u_label)),
                sanitize_label(Some(relation)),
                sanitize_label(Some(confidence)),
                context_suffix,
                sanitize_label(Some(v_label)),
            );
            lines.push(line);
        }
    }

    let output = lines.join("\n");
    if output.len() > char_budget {
        let cut_at = output[..char_budget]
            .rfind('\n')
            .filter(|&p| p > 0)
            .unwrap_or(char_budget);
        let total_nodes = lines.iter().filter(|l| l.starts_with("NODE ")).count();
        let shown_nodes = output[..cut_at]
            .split('\n')
            .filter(|l| l.starts_with("NODE "))
            .count();
        let cut_count = total_nodes.saturating_sub(shown_nodes);
        return format!(
            "{}\n... (truncated — {} more nodes cut by ~{}-token budget. Narrow with context_filter=['call'] or use get_node for a specific symbol)",
            &output[..cut_at],
            cut_count,
            token_budget,
        );
    }
    output
}

// ---- Find node ----

pub fn find_node(g: &QueryGraph, label: &str) -> Vec<String> {
    let tokens = search_tokens(label);
    let term = tokens.join(" ");
    if term.is_empty() {
        return vec![];
    }
    let mut exact = Vec::new();
    let mut prefix = Vec::new();
    let mut substring = Vec::new();
    for (nid, data) in &g.nodes {
        let norm_label = data.label.to_lowercase();
        let bare_label = norm_label.trim_end_matches(['(', ')']).to_string();
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
    let mut result = exact;
    result.extend(prefix);
    result.extend(substring);
    result
}

// ---- Communities ----

pub fn communities_from_graph(g: &QueryGraph) -> HashMap<i64, Vec<String>> {
    let mut communities: HashMap<i64, Vec<String>> = HashMap::new();
    for (nid, data) in &g.nodes {
        if let Some(cid) = data.community {
            communities.entry(cid).or_default().push(nid.clone());
        }
    }
    communities
}

// ---- Full query pipeline ----

pub fn query_graph_text(
    g: &mut QueryGraph,
    question: &str,
    mode: &str,
    depth: usize,
    token_budget: usize,
    context_filters: &[String],
) -> String {
    let terms = query_terms(question);
    let scored = score_nodes(g, &terms);
    let start_nodes = pick_seeds(&scored, 3, 0.2);
    if start_nodes.is_empty() {
        return "No matching nodes found.".to_string();
    }
    let (resolved_filters, filter_source) = resolve_context_filters(question, context_filters);
    let traversal_graph = filter_graph_by_context(g, &resolved_filters);
    let (nodes, edges) = if mode == "dfs" {
        dfs(&traversal_graph, &start_nodes, depth)
    } else {
        bfs(&traversal_graph, &start_nodes, depth)
    };

    let seed_labels: Vec<String> = start_nodes
        .iter()
        .map(|n| {
            g.nodes
                .get(n)
                .map(|d| d.label.clone())
                .unwrap_or_else(|| n.clone())
        })
        .collect();

    let mut header_parts = vec![
        format!("Traversal: {} depth={}", mode.to_uppercase(), depth),
        format!("Start: {:?}", seed_labels),
    ];
    if !resolved_filters.is_empty() {
        let src = filter_source.as_deref().unwrap_or("unknown");
        header_parts.push(format!(
            "Context: {} ({})",
            resolved_filters.join(", "),
            src
        ));
    }
    header_parts.push(format!("{} nodes found", nodes.len()));
    let header = header_parts.join(" | ") + "\n\n";
    header
        + &subgraph_to_text(
            &traversal_graph,
            &nodes,
            &edges,
            token_budget,
            Some(&start_nodes),
        )
}

// ---- Load graph from JSON ----

pub fn load_graph_with_cap(path: &str, max_bytes: u64) -> Result<QueryGraph, String> {
    use std::path::Path;
    let resolved = Path::new(path);
    if resolved.extension().and_then(|e| e.to_str()) != Some("json") {
        return Err(format!("Graph path must be a .json file, got: {:?}", path));
    }
    if !resolved.exists() {
        return Err(format!("Graph file not found: {}", path));
    }
    let meta = std::fs::metadata(resolved).map_err(|e| e.to_string())?;
    if meta.len() > max_bytes {
        let msg = format!(
            "error: graph.json ({} bytes) exceeds {} byte cap",
            meta.len(),
            max_bytes
        );
        eprintln!("{}", msg);
        return Err(msg);
    }
    let content = std::fs::read_to_string(resolved).map_err(|e| e.to_string())?;
    let data: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| format!("graph.json is corrupted ({}). Re-run to rebuild.", e))?;

    let mut g = QueryGraph::new();

    if let Some(nodes) = data.get("nodes").and_then(|v| v.as_array()) {
        for node in nodes {
            let id = node
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if id.is_empty() {
                continue;
            }
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
            let community = node.get("community").and_then(|v| v.as_i64());
            g.add_node(
                id,
                NodeData {
                    label,
                    source_file,
                    source_location,
                    community,
                },
            );
        }
    }

    let links_key = if data.get("links").is_some() {
        "links"
    } else {
        "edges"
    };
    if let Some(links) = data.get(links_key).and_then(|v| v.as_array()) {
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
            if source.is_empty() || target.is_empty() {
                continue;
            }
            let relation = link
                .get("relation")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let confidence = link
                .get("confidence")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let context = link
                .get("context")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            g.add_edge(
                source,
                target,
                EdgeData {
                    relation,
                    confidence,
                    context,
                },
            );
        }
    }

    Ok(g)
}

pub fn load_graph(path: &str) -> Result<QueryGraph, String> {
    load_graph_with_cap(path, MAX_GRAPH_FILE_BYTES)
}

// ---- Tests ----

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn make_graph() -> QueryGraph {
        let mut g = QueryGraph::new();
        g.add_node(
            "n1",
            NodeData {
                label: "extract".into(),
                source_file: "extract.py".into(),
                source_location: "L10".into(),
                community: Some(0),
            },
        );
        g.add_node(
            "n2",
            NodeData {
                label: "cluster".into(),
                source_file: "cluster.py".into(),
                source_location: "L5".into(),
                community: Some(0),
            },
        );
        g.add_node(
            "n3",
            NodeData {
                label: "build".into(),
                source_file: "build.py".into(),
                source_location: "L1".into(),
                community: Some(1),
            },
        );
        g.add_node(
            "n4",
            NodeData {
                label: "report".into(),
                source_file: "report.py".into(),
                source_location: "L1".into(),
                community: Some(1),
            },
        );
        g.add_node(
            "n5",
            NodeData {
                label: "isolated".into(),
                source_file: "other.py".into(),
                source_location: "L1".into(),
                community: Some(2),
            },
        );
        g.add_edge(
            "n1",
            "n2",
            EdgeData {
                relation: "calls".into(),
                confidence: "INFERRED".into(),
                context: Some("call".into()),
            },
        );
        g.add_edge(
            "n2",
            "n3",
            EdgeData {
                relation: "imports".into(),
                confidence: "EXTRACTED".into(),
                context: Some("import".into()),
            },
        );
        g.add_edge(
            "n3",
            "n4",
            EdgeData {
                relation: "uses".into(),
                confidence: "EXTRACTED".into(),
                context: None,
            },
        );
        g
    }

    fn write_graph_json(nodes: &[&str]) -> NamedTempFile {
        let nodes_json: Vec<serde_json::Value> = nodes
            .iter()
            .map(|n| {
                serde_json::json!({
                    "id": n,
                    "label": n,
                    "community": 0
                })
            })
            .collect();
        let data = serde_json::json!({
            "directed": false,
            "nodes": nodes_json,
            "links": []
        });
        let mut f = NamedTempFile::with_suffix(".json").unwrap();
        f.write_all(data.to_string().as_bytes()).unwrap();
        f
    }

    // --- communities_from_graph ---

    #[test]
    fn test_communities_from_graph_basic() {
        let g = make_graph();
        let c = communities_from_graph(&g);
        assert!(c.contains_key(&0));
        assert!(c.contains_key(&1));
        assert!(c[&0].contains(&"n1".to_string()));
        assert!(c[&0].contains(&"n2".to_string()));
        assert!(c[&1].contains(&"n3".to_string()));
    }

    #[test]
    fn test_communities_from_graph_no_community_attr() {
        let mut g = QueryGraph::new();
        g.add_node(
            "a",
            NodeData {
                label: "foo".into(),
                source_file: "".into(),
                source_location: "".into(),
                community: None,
            },
        );
        let c = communities_from_graph(&g);
        assert!(c.is_empty());
    }

    #[test]
    fn test_communities_from_graph_isolated() {
        let g = make_graph();
        let c = communities_from_graph(&g);
        assert!(c.contains_key(&2));
        assert!(c[&2].contains(&"n5".to_string()));
    }

    // --- score_nodes ---

    #[test]
    fn test_score_nodes_exact_label_match() {
        let mut g = make_graph();
        let scored = score_nodes(&mut g, &["extract".to_string()]);
        let nids: Vec<&str> = scored.iter().map(|(_, n)| n.as_str()).collect();
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
        let nids: Vec<&str> = scored.iter().map(|(_, n)| n.as_str()).collect();
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
        let result = find_node(&g, "extract?");
        assert!(result.contains(&"n1".to_string()));
    }

    // --- query_terms ---

    #[test]
    fn test_query_terms_strips_search_punctuation() {
        let terms = query_terms("what calls extract?");
        assert!(terms.contains(&"what".to_string()));
        assert!(terms.contains(&"calls".to_string()));
        assert!(terms.contains(&"extract".to_string()));
        assert!(!terms.iter().any(|t| t.contains('?')));
    }

    #[test]
    fn test_query_graph_text_keeps_short_non_english_terms() {
        let mut g = QueryGraph::new();
        g.add_node(
            "frontend",
            NodeData {
                label: "前端".into(),
                source_file: "docs/前端.md".into(),
                source_location: "L1".into(),
                community: Some(0),
            },
        );
        let text = query_graph_text(&mut g, "前端", "bfs", 1, 2000, &[]);
        assert!(!text.contains("No matching nodes found."));
        assert!(text.contains("NODE 前端"));
    }

    #[test]
    fn test_infer_context_filters_for_calls_question() {
        let filters = infer_context_filters("who calls extract");
        assert!(filters.contains(&"call".to_string()));
    }

    #[test]
    fn test_resolve_context_filters_explicit_overrides_heuristic() {
        let (filters, source) =
            resolve_context_filters("who calls extract", &["field".to_string()]);
        assert_eq!(filters, vec!["field"]);
        assert_eq!(source.as_deref(), Some("explicit"));
    }

    // --- bfs ---

    #[test]
    fn test_bfs_depth_1() {
        let g = make_graph();
        let (visited, _) = bfs(&g, &["n1".to_string()], 1);
        assert!(visited.contains("n1"));
        assert!(visited.contains("n2"));
        assert!(!visited.contains("n3"));
    }

    #[test]
    fn test_bfs_depth_2() {
        let g = make_graph();
        let (visited, _) = bfs(&g, &["n1".to_string()], 2);
        assert!(visited.contains("n3"));
    }

    #[test]
    fn test_bfs_disconnected() {
        let g = make_graph();
        let (visited, _) = bfs(&g, &["n5".to_string()], 3);
        assert_eq!(visited, HashSet::from(["n5".to_string()]));
    }

    #[test]
    fn test_bfs_returns_edges() {
        let g = make_graph();
        let (_, edges) = bfs(&g, &["n1".to_string()], 1);
        assert!(!edges.is_empty());
        assert!(edges.iter().any(|(u, v)| u == "n1" || v == "n1"));
    }

    #[test]
    fn test_filter_graph_by_context_limits_traversal() {
        let g = make_graph();
        let filtered = filter_graph_by_context(&g, &["call".to_string()]);
        let (visited, edges) = bfs(&filtered, &["n1".to_string()], 2);
        assert!(visited.contains("n2"));
        assert!(!visited.contains("n3"));
        assert_eq!(edges.len(), 1);
    }

    // --- dfs ---

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
        for n in &["n1", "n2", "n3", "n4"] {
            assert!(visited.contains(*n), "missing {}", n);
        }
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

    #[test]
    fn test_query_graph_text_explicit_context_filter_changes_traversal() {
        let mut g = make_graph();
        let text = query_graph_text(&mut g, "extract", "bfs", 2, 2000, &["call".to_string()]);
        assert!(text.contains("Context: call (explicit)"));
        assert!(text.contains("cluster"));
        assert!(!text.contains("build"));
    }

    #[test]
    fn test_query_graph_text_heuristic_context_filter_changes_traversal() {
        let mut g = make_graph();
        let text = query_graph_text(&mut g, "who calls extract", "bfs", 2, 2000, &[]);
        assert!(text.contains("Context: call (heuristic)"));
        assert!(text.contains("cluster"));
        assert!(!text.contains("build"));
    }

    // --- load_graph ---

    #[test]
    fn test_load_graph_roundtrip() {
        let f = write_graph_json(&["alpha", "beta"]);
        let g = load_graph(f.path().to_str().unwrap()).unwrap();
        assert_eq!(g.node_count(), 2);
    }

    #[test]
    fn test_load_graph_missing_file() {
        let result = load_graph("/tmp/does_not_exist_codesynapse.json");
        assert!(result.is_err());
    }

    #[test]
    fn test_load_graph_rejects_oversized_file() {
        let f = write_graph_json(&["a"]);
        let result = load_graph_with_cap(f.path().to_str().unwrap(), 16);
        assert!(result.is_err());
        let msg = result.unwrap_err();
        assert!(msg.contains("exceeds") && msg.contains("byte cap"));
    }

    #[test]
    fn test_load_graph_accepts_under_cap() {
        let f = write_graph_json(&["a"]);
        let result = load_graph_with_cap(f.path().to_str().unwrap(), 10 * 1024 * 1024);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().node_count(), 1);
    }

    #[test]
    fn test_maybe_reload_detects_graph_change() {
        let f1 = write_graph_json(&["alpha", "beta"]);
        let path = f1.path().to_str().unwrap();
        let g1 = load_graph(path).unwrap();
        assert_eq!(g1.node_count(), 2);
        // Write new content to same path
        let nodes_json: Vec<serde_json::Value> = ["alpha", "beta", "gamma"]
            .iter()
            .map(|n| serde_json::json!({"id": n, "label": n, "community": 0}))
            .collect();
        let data = serde_json::json!({"directed": false, "nodes": nodes_json, "links": []});
        std::fs::write(path, data.to_string()).unwrap();
        let g2 = load_graph(path).unwrap();
        assert!(g2.nodes.contains_key("gamma"));
    }

    #[test]
    fn test_load_graph_cache_key_changes_with_content() {
        let f = write_graph_json(&["a"]);
        let path = f.path();
        let s1 = std::fs::metadata(path).unwrap();
        let key1 = (s1.modified().unwrap(), s1.len());
        std::thread::sleep(std::time::Duration::from_millis(10));
        let nodes_json = vec![
            serde_json::json!({"id": "a", "label": "a", "community": 0}),
            serde_json::json!({"id": "b", "label": "b", "community": 0}),
        ];
        let data = serde_json::json!({"directed": false, "nodes": nodes_json, "links": []});
        std::fs::write(path, data.to_string()).unwrap();
        let s2 = std::fs::metadata(path).unwrap();
        let key2 = (s2.modified().unwrap(), s2.len());
        assert_ne!(key1, key2, "stat key must change when file content changes");
    }

    // --- IDF ---

    fn make_noisy_graph() -> QueryGraph {
        let mut g = QueryGraph::new();
        for i in 0..20usize {
            let id = format!("err{}", i);
            g.add_node(
                id.clone(),
                NodeData {
                    label: format!("error_handler_{}", i),
                    source_file: format!("err{}.py", i),
                    source_location: "L1".into(),
                    community: Some(0),
                },
            );
            if i > 0 {
                let prev = format!("err{}", i - 1);
                g.add_edge(
                    prev,
                    id,
                    EdgeData {
                        relation: "calls".into(),
                        confidence: "EXTRACTED".into(),
                        context: Some("call".into()),
                    },
                );
            }
        }
        g.add_node(
            "fbs",
            NodeData {
                label: "FooBarService".into(),
                source_file: "service.py".into(),
                source_location: "L1".into(),
                community: Some(1),
            },
        );
        g.add_node(
            "fbs_dep",
            NodeData {
                label: "ServiceClient".into(),
                source_file: "client.py".into(),
                source_location: "L1".into(),
                community: Some(1),
            },
        );
        g.add_edge(
            "fbs",
            "fbs_dep",
            EdgeData {
                relation: "uses".into(),
                confidence: "EXTRACTED".into(),
                context: None,
            },
        );
        g
    }

    #[test]
    fn test_idf_downweights_common_terms() {
        let mut g = make_noisy_graph();
        let scored = score_nodes(&mut g, &["foobarservice".to_string(), "error".to_string()]);
        assert!(!scored.is_empty());
        assert_eq!(scored[0].1, "fbs", "FooBarService should rank first");
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
        assert!(g1.idf_cache.contains_key("extract"));
        assert!(!g2.idf_cache.contains_key("extract"));
    }

    #[test]
    fn test_idf_rare_term_gets_high_weight() {
        let mut g = make_graph();
        let idf = compute_idf(&mut g, &["extract".to_string()]);
        assert!(idf["extract"] > 1.0, "rare term should get IDF > 1");
    }

    #[test]
    fn test_idf_common_term_gets_low_weight() {
        let mut g = QueryGraph::new();
        for i in 0..20usize {
            g.add_node(
                format!("n{}", i),
                NodeData {
                    label: format!("handle_{}", i),
                    source_file: format!("f{}.py", i),
                    source_location: "L1".into(),
                    community: None,
                },
            );
        }
        let mut g_ref = g;
        let idf = compute_idf(&mut g_ref, &["handle".to_string()]);
        assert!(idf["handle"] < 1.0, "common term should get IDF < 1");
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
        assert_eq!(seeds, vec!["fbs"]);
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
        assert!(pick_seeds(&[], 3, 0.2).is_empty());
    }

    #[test]
    fn test_pick_seeds_single() {
        let scored = vec![(5.0, "x".to_string())];
        assert_eq!(pick_seeds(&scored, 3, 0.2), vec!["x"]);
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

    // --- identifier + noise query ---

    #[test]
    fn test_query_seeds_from_identifier_not_noise() {
        let mut g = make_noisy_graph();
        let text = query_graph_text(&mut g, "FooBarService error handling", "bfs", 2, 2000, &[]);
        assert!(text.contains("FooBarService"));
        assert!(text.contains("ServiceClient"));
    }

    // --- parameter_type context filter ---

    #[test]
    fn test_query_graph_text_parameter_type_context_filter() {
        let mut g = QueryGraph::new();
        g.add_node(
            "process",
            NodeData {
                label: "process".into(),
                source_file: "sample.cs".into(),
                source_location: "L20".into(),
                community: None,
            },
        );
        g.add_node(
            "payload",
            NodeData {
                label: "Payload".into(),
                source_file: "sample.cs".into(),
                source_location: "L5".into(),
                community: None,
            },
        );
        g.add_node(
            "other",
            NodeData {
                label: "PayloadFactory".into(),
                source_file: "sample.cs".into(),
                source_location: "L40".into(),
                community: None,
            },
        );
        g.add_edge(
            "process",
            "payload",
            EdgeData {
                relation: "references".into(),
                confidence: "EXTRACTED".into(),
                context: Some("parameter_type".into()),
            },
        );
        g.add_edge(
            "process",
            "other",
            EdgeData {
                relation: "calls".into(),
                confidence: "EXTRACTED".into(),
                context: Some("call".into()),
            },
        );
        let text = query_graph_text(
            &mut g,
            "who accepts Payload",
            "bfs",
            2,
            2000,
            &["parameter_type".to_string()],
        );
        assert!(text.contains("parameter_type"));
        assert!(text.contains("Payload"));
        assert!(!text.contains("PayloadFactory"));
    }

    // --- context filter aliases ---

    #[test]
    fn test_query_graph_text_context_filter_aliases_resolve() {
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
        assert_eq!(
            normalize_context_filters(&["parameter_type".to_string()]),
            vec!["parameter_type"]
        );
        assert_eq!(
            normalize_context_filters(&["field".to_string()]),
            vec!["field"]
        );
    }

    // --- Chinese text ---

    #[test]
    fn test_query_terms_chinese_mixed() {
        let terms = query_terms("前端 router 路由配置");
        assert!(terms.iter().any(|t| t.contains("前端") || t == "前端"));
        assert!(terms.contains(&"router".to_string()));
        assert!(terms.iter().any(|t| t.contains("路由")));
        assert!(terms.iter().any(|t| t.contains("配置")));
    }

    #[test]
    fn test_query_terms_non_chinese_scripts_are_not_segmented() {
        assert!(!has_chinese("かなカナ한글"));
        let terms = query_terms("かなカナ한글");
        assert!(terms.contains(&"かなカナ한글".to_string()));
    }

    #[test]
    fn test_query_terms_chinese_no_jieba_fallback() {
        let terms = query_terms("页面路由");
        assert!(terms.iter().any(|t| t.contains("页面")));
        assert!(terms.iter().any(|t| t.contains("路由")));
        assert!(terms.contains(&"页面路由".to_string()));
        assert_eq!(terms.len(), 4); // bigrams: 页面, 面路, 路由 + original
    }

    #[test]
    fn test_score_nodes_chinese_substring_match() {
        let mut g = QueryGraph::new();
        g.add_node(
            "n1",
            NodeData {
                label: "路由桥接核对表".into(),
                source_file: "doc.md".into(),
                source_location: "L1".into(),
                community: Some(0),
            },
        );
        g.add_node(
            "n2",
            NodeData {
                label: "其他内容".into(),
                source_file: "doc.md".into(),
                source_location: "L1".into(),
                community: Some(0),
            },
        );
        let scored = score_nodes(&mut g, &["路由".to_string()]);
        let nids: Vec<&str> = scored.iter().map(|(_, n)| n.as_str()).collect();
        assert!(nids.contains(&"n1"));
        assert!(!nids.contains(&"n2"));
    }

    #[test]
    fn test_query_text_chinese_finds_routing_nodes() {
        let mut g = QueryGraph::new();
        g.add_node(
            "parent",
            NodeData {
                label: "页面路由规范".into(),
                source_file: "doc.md".into(),
                source_location: "L1".into(),
                community: Some(0),
            },
        );
        g.add_node(
            "child",
            NodeData {
                label: "路由桥接核对表".into(),
                source_file: "doc.md".into(),
                source_location: "L10".into(),
                community: Some(0),
            },
        );
        g.add_edge(
            "parent",
            "child",
            EdgeData {
                relation: "contains".into(),
                confidence: "EXTRACTED".into(),
                context: None,
            },
        );
        let text = query_graph_text(&mut g, "页面路由", "bfs", 2, 2000, &[]);
        assert!(!text.contains("No matching nodes found."));
        assert!(text.contains("路由"));
    }
}
