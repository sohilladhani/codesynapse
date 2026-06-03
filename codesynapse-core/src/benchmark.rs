use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Write;
use std::path::Path;

use serde_json::Value;

use crate::error::Result;
use crate::security::{check_file_size, MAX_GRAPH_FILE_BYTES};

const CHARS_PER_TOKEN: usize = 4;

pub const SAMPLE_QUESTIONS: &[&str] = &[
    "how does authentication work",
    "what is the main entry point",
    "how are errors handled",
    "what connects the data layer to the api",
    "what are the core abstractions",
];

// ---------------------------------------------------------------------------
// Unicode safety
// ---------------------------------------------------------------------------

pub fn safe_char<'a>(unicode: &'a str, ascii: &'a str, supports_unicode: bool) -> &'a str {
    if supports_unicode {
        unicode
    } else {
        ascii
    }
}

pub fn hr(width: usize, supports_unicode: bool) -> String {
    safe_char("─", "-", supports_unicode).repeat(width)
}

// ---------------------------------------------------------------------------
// Lightweight graph for benchmarking (loads node-link JSON)
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
struct BenchGraph {
    nodes: Vec<BenchNode>,
    node_index: HashMap<String, usize>,
    adj: HashMap<String, Vec<String>>,
    edge_count: usize,
}

#[derive(Debug, Clone)]
struct BenchNode {
    id: String,
    label: String,
    source_file: String,
    source_location: String,
}

fn value_to_id(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

impl BenchGraph {
    fn from_json(data: &Value) -> Option<Self> {
        let mut g = BenchGraph::default();

        let nodes = data.get("nodes")?.as_array()?;
        for n in nodes {
            let id = n.get("id").and_then(value_to_id).unwrap_or_default();
            if id.is_empty() {
                continue;
            }
            let label = n
                .get("label")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let source_file = n
                .get("source_file")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let source_location = n
                .get("source_location")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let idx = g.nodes.len();
            g.node_index.insert(id.clone(), idx);
            g.adj.entry(id.clone()).or_default();
            g.nodes.push(BenchNode {
                id,
                label,
                source_file,
                source_location,
            });
        }

        // Accept "links" or "edges" key
        let links_key = if data.get("links").is_some() {
            "links"
        } else {
            "edges"
        };
        if let Some(links) = data.get(links_key).and_then(Value::as_array) {
            for e in links {
                let src = e.get("source").and_then(value_to_id).unwrap_or_default();
                let tgt = e.get("target").and_then(value_to_id).unwrap_or_default();
                if src.is_empty() || tgt.is_empty() {
                    continue;
                }
                g.adj.entry(src.clone()).or_default().push(tgt.clone());
                g.adj.entry(tgt.clone()).or_default().push(src.clone());
                g.edge_count += 1;
            }
        }

        Some(g)
    }

    fn node_count(&self) -> usize {
        self.nodes.len()
    }
    fn edge_count(&self) -> usize {
        self.edge_count
    }
}

// ---------------------------------------------------------------------------
// Token estimation
// ---------------------------------------------------------------------------

fn estimate_tokens(text: &str) -> u64 {
    (text.len() / CHARS_PER_TOKEN).max(1) as u64
}

// ---------------------------------------------------------------------------
// Query terms (port of Python _query_terms from serve.py)
// ---------------------------------------------------------------------------

fn has_non_ascii(s: &str) -> bool {
    !s.is_ascii()
}

fn is_searchable(term: &str) -> bool {
    if term
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
    {
        term.len() > 2
    } else {
        true
    }
}

fn bench_query_terms(question: &str) -> Vec<String> {
    let mut terms = Vec::new();
    for word in question.split_whitespace() {
        if has_non_ascii(word) {
            let s = word.to_string();
            if is_searchable(&s) {
                terms.push(s);
            }
        } else {
            let lower = word.to_lowercase();
            let mut start: Option<usize> = None;
            let chars: Vec<(usize, char)> = lower.char_indices().collect();
            for (i, c) in &chars {
                if c.is_alphanumeric() || *c == '_' {
                    if start.is_none() {
                        start = Some(*i);
                    }
                } else if let Some(s) = start {
                    let tok = &lower[s..*i];
                    if is_searchable(tok) {
                        terms.push(tok.to_string());
                    }
                    start = None;
                }
            }
            if let Some(s) = start {
                let tok = &lower[s..];
                if is_searchable(tok) {
                    terms.push(tok.to_string());
                }
            }
        }
    }
    terms
}

// ---------------------------------------------------------------------------
// Core: BFS subgraph token estimation
// ---------------------------------------------------------------------------

pub fn query_subgraph_tokens(g_data: &Value, question: &str, depth: u32) -> u64 {
    let g = match BenchGraph::from_json(g_data) {
        Some(g) => g,
        None => return 0,
    };
    query_subgraph_tokens_inner(&g, question, depth)
}

fn query_subgraph_tokens_inner(g: &BenchGraph, question: &str, depth: u32) -> u64 {
    let terms = bench_query_terms(question);
    if terms.is_empty() {
        return 0;
    }

    // Score nodes by term matches in label
    let mut scored: Vec<(usize, &str)> = g
        .nodes
        .iter()
        .map(|n| {
            let label = n.label.to_lowercase();
            let score = terms.iter().filter(|t| label.contains(t.as_str())).count();
            (score, n.id.as_str())
        })
        .filter(|(s, _)| *s > 0)
        .collect();
    scored.sort_by_key(|k| std::cmp::Reverse(k.0));

    let start_ids: Vec<&str> = scored.iter().take(3).map(|(_, id)| *id).collect();
    if start_ids.is_empty() {
        return 0;
    }

    // BFS
    let mut visited: HashSet<&str> = start_ids.iter().copied().collect();
    let mut frontier: VecDeque<&str> = start_ids.iter().copied().collect();
    let mut edges_seen: Vec<(&str, &str)> = Vec::new();

    for _ in 0..depth {
        let current: Vec<&str> = frontier.drain(..).collect();
        for n in &current {
            if let Some(neighbors) = g.adj.get(*n) {
                for nb in neighbors {
                    let nb_s = nb.as_str();
                    if !visited.contains(nb_s) {
                        visited.insert(nb_s);
                        frontier.push_back(nb_s);
                        edges_seen.push((n, nb_s));
                    }
                }
            }
        }
    }

    // Build text like Python does
    let mut lines = Vec::new();
    for nid in &visited {
        if let Some(&idx) = g.node_index.get(*nid) {
            let n = &g.nodes[idx];
            lines.push(format!(
                "NODE {} src={} loc={}",
                n.label, n.source_file, n.source_location
            ));
        }
    }
    for (u, v) in &edges_seen {
        if visited.contains(u) && visited.contains(v) {
            lines.push(format!("EDGE {} --> {}", u, v));
        }
    }

    if lines.is_empty() {
        return 0;
    }
    estimate_tokens(&lines.join("\n"))
}

// ---------------------------------------------------------------------------
// Public result types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct QuestionResult {
    pub question: String,
    pub query_tokens: u64,
    pub reduction: f64,
}

#[derive(Debug, Clone, Default)]
pub struct BenchmarkResult {
    pub corpus_tokens: u64,
    pub corpus_words: u64,
    pub nodes: usize,
    pub edges: usize,
    pub avg_query_tokens: u64,
    pub reduction_ratio: f64,
    pub per_question: Vec<QuestionResult>,
    pub error: Option<String>,
}

// ---------------------------------------------------------------------------
// run_benchmark
// ---------------------------------------------------------------------------

pub fn run_benchmark(
    graph_path: &Path,
    corpus_words: Option<u64>,
    questions: Option<&[&str]>,
    max_bytes: Option<u64>,
) -> Result<BenchmarkResult> {
    let cap = max_bytes.unwrap_or(MAX_GRAPH_FILE_BYTES);
    check_file_size(graph_path, cap)?;

    let text = std::fs::read_to_string(graph_path)?;
    let data: Value = serde_json::from_str(&text)?;

    let g = match BenchGraph::from_json(&data) {
        Some(g) => g,
        None => {
            return Ok(BenchmarkResult {
                error: Some("Failed to parse graph JSON".to_string()),
                ..Default::default()
            })
        }
    };

    let node_count = g.node_count();
    let edge_count = g.edge_count();
    let corpus_words = corpus_words.unwrap_or(node_count as u64 * 50);
    let corpus_tokens = corpus_words * 100 / 75;

    let qs = questions.unwrap_or(SAMPLE_QUESTIONS);
    let mut per_question: Vec<QuestionResult> = Vec::new();

    for q in qs {
        let qt = query_subgraph_tokens_inner(&g, q, 3);
        if qt > 0 {
            let reduction = if qt > 0 {
                (corpus_tokens as f64 / qt as f64 * 10.0).round() / 10.0
            } else {
                0.0
            };
            per_question.push(QuestionResult {
                question: q.to_string(),
                query_tokens: qt,
                reduction,
            });
        }
    }

    if per_question.is_empty() {
        return Ok(BenchmarkResult {
            error: Some(
                "No matching nodes found for sample questions. Build the graph first.".to_string(),
            ),
            nodes: node_count,
            edges: edge_count,
            corpus_tokens,
            corpus_words,
            ..Default::default()
        });
    }

    let avg_query_tokens =
        per_question.iter().map(|p| p.query_tokens).sum::<u64>() / per_question.len() as u64;
    let reduction_ratio = if avg_query_tokens > 0 {
        (corpus_tokens as f64 / avg_query_tokens as f64 * 10.0).round() / 10.0
    } else {
        0.0
    };

    Ok(BenchmarkResult {
        corpus_tokens,
        corpus_words,
        nodes: node_count,
        edges: edge_count,
        avg_query_tokens,
        reduction_ratio,
        per_question,
        error: None,
    })
}

// ---------------------------------------------------------------------------
// print_benchmark
// ---------------------------------------------------------------------------

pub fn print_benchmark(result: &BenchmarkResult) {
    print_benchmark_to(result, &mut std::io::stdout(), true);
}

pub fn print_benchmark_to<W: Write>(result: &BenchmarkResult, out: &mut W, supports_unicode: bool) {
    if let Some(ref err) = result.error {
        let _ = writeln!(out, "Benchmark error: {err}");
        return;
    }

    let arrow = safe_char("\u{2192}", "->", supports_unicode);
    let _ = writeln!(out, "\ncodesynapse token reduction benchmark");
    let _ = writeln!(out, "{}", hr(50, supports_unicode));
    let _ = writeln!(
        out,
        "  Corpus:          {} words {} ~{} tokens (naive)",
        result.corpus_words, arrow, result.corpus_tokens
    );
    let _ = writeln!(
        out,
        "  Graph:           {} nodes, {} edges",
        result.nodes, result.edges
    );
    let _ = writeln!(
        out,
        "  Avg query cost:  ~{} tokens",
        result.avg_query_tokens
    );
    let _ = writeln!(
        out,
        "  Reduction:       {}x fewer tokens per query",
        result.reduction_ratio
    );
    let _ = writeln!(out, "\n  Per question:");
    for p in &result.per_question {
        let q = if p.question.len() > 55 {
            &p.question[..55]
        } else {
            &p.question
        };
        let _ = writeln!(out, "    [{}x] {}", p.reduction, q);
    }
    let _ = writeln!(out);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn make_graph_value() -> Value {
        json!({
            "nodes": [
                {"id": "n1", "label": "authentication", "source_file": "auth.py", "source_location": "L1", "community": 0},
                {"id": "n2", "label": "api_handler",    "source_file": "api.py",  "source_location": "L5", "community": 0},
                {"id": "n3", "label": "main_entry",     "source_file": "main.py", "source_location": "L1", "community": 1},
                {"id": "n4", "label": "error_handler",  "source_file": "errors.py","source_location": "L1", "community": 1},
                {"id": "n5", "label": "database_layer", "source_file": "db.py",   "source_location": "L1", "community": 2}
            ],
            "links": [
                {"source": "n1", "target": "n2", "relation": "calls",   "confidence": "INFERRED"},
                {"source": "n2", "target": "n3", "relation": "imports", "confidence": "EXTRACTED"},
                {"source": "n3", "target": "n4", "relation": "uses",    "confidence": "EXTRACTED"},
                {"source": "n5", "target": "n2", "relation": "provides","confidence": "EXTRACTED"}
            ]
        })
    }

    fn write_graph_file(data: &Value) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        write!(f, "{}", serde_json::to_string(data).unwrap()).unwrap();
        f
    }

    // --- query_subgraph_tokens ---

    #[test]
    fn test_query_returns_positive_for_matching_question() {
        let g = make_graph_value();
        let tokens = query_subgraph_tokens(&g, "how does authentication work", 3);
        assert!(tokens > 0, "expected >0 tokens, got {tokens}");
    }

    #[test]
    fn test_query_returns_zero_for_no_match() {
        let g = make_graph_value();
        let tokens = query_subgraph_tokens(&g, "xyzzy plugh zorkmid", 3);
        assert_eq!(tokens, 0);
    }

    #[test]
    fn test_query_bfs_expands_neighbors() {
        let g = make_graph_value();
        let tokens_deep = query_subgraph_tokens(&g, "authentication", 3);
        let tokens_shallow = query_subgraph_tokens(&g, "authentication", 1);
        assert!(
            tokens_deep >= tokens_shallow,
            "deep={tokens_deep} shallow={tokens_shallow}"
        );
    }

    #[test]
    fn test_query_keeps_short_non_english_terms() {
        let g = json!({
            "nodes": [
                {"id": "frontend", "label": "\u{524d}\u{7aef}", "source_file": "docs/frontend.md", "source_location": "L1", "community": 0}
            ],
            "links": []
        });
        let tokens = query_subgraph_tokens(&g, "\u{524d}\u{7aef}", 1);
        assert!(tokens > 0, "expected >0 for Chinese query term");
    }

    // --- run_benchmark ---

    #[test]
    fn test_run_benchmark_returns_reduction() {
        let data = make_graph_value();
        let f = write_graph_file(&data);
        let result = run_benchmark(f.path(), Some(10_000), None, None).unwrap();
        assert!(result.error.is_none());
        assert!(
            result.reduction_ratio > 1.0,
            "reduction_ratio={}",
            result.reduction_ratio
        );
    }

    #[test]
    fn test_run_benchmark_corpus_tokens_proportional() {
        let data = make_graph_value();
        let f = write_graph_file(&data);
        let r1 = run_benchmark(f.path(), Some(1_000), None, None).unwrap();
        let r2 = run_benchmark(f.path(), Some(10_000), None, None).unwrap();
        let diff = (r2.corpus_tokens as i64 - r1.corpus_tokens as i64 * 10).unsigned_abs();
        assert!(
            diff <= r1.corpus_tokens,
            "corpus_tokens not proportional: r1={} r2={} diff={}",
            r1.corpus_tokens,
            r2.corpus_tokens,
            diff
        );
    }

    #[test]
    fn test_run_benchmark_per_question_list() {
        let data = make_graph_value();
        let f = write_graph_file(&data);
        let qs = &["how does authentication work", "what is the main entry"];
        let result = run_benchmark(f.path(), Some(5_000), Some(qs), None).unwrap();
        assert!(!result.per_question.is_empty());
        for p in &result.per_question {
            assert!(!p.question.is_empty());
            assert!(p.query_tokens > 0);
            assert!(p.reduction > 0.0);
        }
    }

    #[test]
    fn test_run_benchmark_estimates_corpus_if_no_words() {
        let data = make_graph_value();
        let f = write_graph_file(&data);
        let result = run_benchmark(f.path(), None, None, None).unwrap();
        assert!(result.corpus_words > 0, "corpus_words should be estimated");
    }

    #[test]
    fn test_run_benchmark_error_on_empty_graph() {
        let data = json!({"nodes": [], "links": []});
        let f = write_graph_file(&data);
        let result = run_benchmark(f.path(), Some(1_000), None, None).unwrap();
        assert!(result.error.is_some(), "expected error for empty graph");
    }

    #[test]
    fn test_run_benchmark_includes_node_edge_counts() {
        let data = make_graph_value();
        let f = write_graph_file(&data);
        let result = run_benchmark(f.path(), Some(5_000), None, None).unwrap();
        assert_eq!(result.nodes, 5, "nodes={}", result.nodes);
        assert_eq!(result.edges, 4, "edges={}", result.edges);
    }

    // --- print_benchmark ---

    #[test]
    fn test_print_benchmark_no_crash() {
        let data = make_graph_value();
        let f = write_graph_file(&data);
        let result = run_benchmark(f.path(), Some(5_000), None, None).unwrap();
        let mut buf = Vec::new();
        print_benchmark_to(&result, &mut buf, true);
        let out = String::from_utf8(buf).unwrap();
        assert!(out.to_lowercase().contains("reduction"), "out={out}");
        assert!(out.contains('x'), "out={out}");
    }

    #[test]
    fn test_print_benchmark_error_message() {
        let result = BenchmarkResult {
            error: Some("test error message".to_string()),
            ..Default::default()
        };
        let mut buf = Vec::new();
        print_benchmark_to(&result, &mut buf, true);
        let out = String::from_utf8(buf).unwrap();
        assert!(out.contains("test error message"), "out={out}");
    }

    // --- safe_char / hr ---

    #[test]
    fn test_safe_char_unicode_mode() {
        assert_eq!(safe_char("\u{2192}", "->", true), "\u{2192}");
        assert_eq!(hr(5, true), "\u{2500}".repeat(5));
    }

    #[test]
    fn test_safe_char_ascii_mode() {
        assert_eq!(safe_char("\u{2192}", "->", false), "->");
        assert_eq!(hr(5, false), "-----");
    }

    #[test]
    fn test_print_benchmark_ascii_only_output() {
        let data = make_graph_value();
        let f = write_graph_file(&data);
        let result = run_benchmark(f.path(), Some(5_000), None, None).unwrap();
        let mut buf = Vec::new();
        print_benchmark_to(&result, &mut buf, false);
        let out = String::from_utf8(buf).unwrap();
        assert!(out.to_lowercase().contains("reduction"), "out={out}");
        assert!(!out.contains('\u{2500}'), "should not contain ─");
        assert!(!out.contains('\u{2192}'), "should not contain →");
    }

    #[test]
    fn test_run_benchmark_rejects_oversized_graph() {
        let data = make_graph_value();
        let f = write_graph_file(&data);
        let result = run_benchmark(f.path(), Some(5_000), None, Some(8));
        assert!(result.is_err(), "expected Err for oversized graph");
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("exceeds"), "msg={msg}");
    }
}
