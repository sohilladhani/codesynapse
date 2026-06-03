use std::collections::{HashMap, HashSet};

use codesynapse_core::embedding::StaticEmbedder;

use crate::graph_query::{query_top_nodes, ServeGraph};

pub struct ContextResult {
    pub entry_points: Vec<(String, String, String)>, // (id, label, source_file)
    pub callers: Vec<(String, String, String)>,
    pub callees: Vec<(String, String, String)>,
    pub fallback: bool,
}

/// Extract symbol-like candidates from a natural-language query for exact label matching.
pub fn extract_symbol_candidates(query: &str) -> Vec<String> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut out: Vec<String> = Vec::new();

    let mut push = |s: &str| {
        if !s.is_empty() && seen.insert(s.to_string()) {
            out.push(s.to_string());
        }
    };

    let trimmed = query.trim();

    // Dot notation (single token like "django.db.models"): split on dots
    if trimmed.contains('.') && !trimmed.contains(' ') {
        for part in trimmed.split('.') {
            let p = part.trim_matches(|c: char| !c.is_alphanumeric() && c != '_');
            push(p);
        }
        return out;
    }

    // Per-word extraction
    for word in trimmed.split_whitespace() {
        let w = word.trim_matches(|c: char| !c.is_alphanumeric() && c != '_');
        if w.len() < 2 {
            continue;
        }
        // Must be a valid identifier (alphanum + underscore only)
        if !w.chars().all(|c| c.is_alphanumeric() || c == '_') {
            continue;
        }
        // Include if it looks like code: has uppercase, has underscore, or is 4+ chars
        if w.chars().any(|c| c.is_uppercase() || c == '_') || w.len() >= 4 {
            push(w);
        }
    }

    // Single token: always include the full trimmed value
    if !trimmed.contains(' ') && !trimmed.is_empty() {
        let cleaned = trimmed.trim_matches(|c: char| !c.is_alphanumeric() && c != '_');
        push(cleaned);
    }

    out
}

/// Exact-symbol lookup + 1-hop call-graph expansion.
///
/// Returns matched entry points with their callers and callees (via `calls` edges only).
/// Falls back to semantic search if no exact label match is found.
pub fn context_query(
    g: &ServeGraph,
    query: &str,
    dense: Option<(&StaticEmbedder, &HashMap<String, Vec<f32>>)>,
) -> ContextResult {
    let candidates = extract_symbol_candidates(query);

    // Exact label match (case-insensitive); strip "()" suffix from function labels.
    // When query contains long identifiers (≥10 chars), use only those for matching —
    // drops generic noise like "task"/"state" that would otherwise flood results.
    let max_candidate_len = candidates.iter().map(|c| c.len()).max().unwrap_or(0);
    let effective_candidates: Vec<&String> = if max_candidate_len >= 10 {
        candidates.iter().filter(|c| c.len() >= 6).collect()
    } else {
        candidates.iter().collect()
    };

    let entry_ids: Vec<String> = g
        .nodes_iter()
        .filter(|(_, n)| {
            let lc_label = n.label.to_lowercase();
            let bare_label = lc_label.strip_suffix("()").unwrap_or(&lc_label);
            effective_candidates.iter().any(|c| {
                let lc_c = c.to_lowercase();
                lc_c == lc_label || lc_c == bare_label
            })
        })
        .map(|(id, _)| id.to_string())
        .collect();

    if entry_ids.is_empty() {
        let fallback_nodes = query_top_nodes(g, query, 5, dense);
        return ContextResult {
            entry_points: fallback_nodes,
            callers: Vec::new(),
            callees: Vec::new(),
            fallback: true,
        };
    }

    let entry_set: HashSet<String> = entry_ids.iter().cloned().collect();

    // Collect calls edges once, avoid holding borrow across node lookups
    let call_edges: Vec<(String, String)> = g
        .edges_iter()
        .filter(|e| e.relation == "calls")
        .map(|e| (e.source.clone(), e.target.clone()))
        .collect();

    let mut caller_ids: HashSet<String> = HashSet::new();
    let mut callee_ids: HashSet<String> = HashSet::new();

    for (src, tgt) in &call_edges {
        if entry_set.contains(tgt) && !entry_set.contains(src) {
            caller_ids.insert(src.clone());
        }
        if entry_set.contains(src) && !entry_set.contains(tgt) {
            callee_ids.insert(tgt.clone());
        }
    }

    let to_tuple = |id: &str| -> Option<(String, String, String)> {
        g.get_node(id)
            .map(|n| (id.to_string(), n.label.clone(), n.source_file.clone()))
    };

    let mut ep_seen: HashSet<(String, String)> = HashSet::new();
    ContextResult {
        entry_points: entry_ids
            .iter()
            .filter_map(|id| to_tuple(id))
            .filter(|(_, label, sf)| ep_seen.insert((label.clone(), sf.clone())))
            .collect(),
        callers: caller_ids.iter().filter_map(|id| to_tuple(id)).collect(),
        callees: callee_ids.iter().filter_map(|id| to_tuple(id)).collect(),
        fallback: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph_query::ServeGraph;

    fn make_graph() -> ServeGraph {
        let mut g = ServeGraph::new_directed();
        g.add_node("handler_id", "QueryHandler", "app/handlers.py", "", None);
        g.add_node("caller_id", "dispatch", "app/router.py", "", None);
        g.add_node("callee_id", "fetch_data", "app/db.py", "", None);
        g.add_node("unrelated_id", "SomeOther", "app/other.py", "", None);
        // calls edges
        g.add_edge("caller_id", "handler_id", "calls", "high", None);
        g.add_edge("handler_id", "callee_id", "calls", "high", None);
        // non-calls edge: should NOT appear in 1-hop expansion
        g.add_edge("unrelated_id", "handler_id", "method", "high", None);
        g
    }

    // --- extract_symbol_candidates ---

    #[test]
    fn test_candidates_camelcase() {
        let c = extract_symbol_candidates("where is QueryHandler");
        assert!(c.contains(&"QueryHandler".to_string()));
    }

    #[test]
    fn test_candidates_snake_case() {
        let c = extract_symbol_candidates("full_text_search");
        assert!(c.contains(&"full_text_search".to_string()));
    }

    #[test]
    fn test_candidates_single_token() {
        let c = extract_symbol_candidates("QueryHandler");
        assert!(c.contains(&"QueryHandler".to_string()));
    }

    #[test]
    fn test_candidates_dot_notation() {
        let c = extract_symbol_candidates("django.db.models");
        assert!(c.contains(&"django".to_string()));
        assert!(c.contains(&"db".to_string()));
        assert!(c.contains(&"models".to_string()));
    }

    #[test]
    fn test_candidates_all_caps() {
        let c = extract_symbol_candidates("what is BM25Index");
        assert!(c.contains(&"BM25Index".to_string()));
    }

    #[test]
    fn test_candidates_excludes_short_stop_words() {
        let c = extract_symbol_candidates("how is it done");
        // "it" (2 chars, no uppercase/underscore) should be excluded
        assert!(!c.contains(&"it".to_string()));
    }

    // --- context_query ---

    #[test]
    fn test_exact_match_function_without_parens() {
        let mut g = ServeGraph::new_directed();
        g.add_node(
            "fn_id",
            "notify_should_wakeup()",
            "runtime/idle.rs",
            "",
            None,
        );
        let r = context_query(&g, "notify_should_wakeup", None);
        assert!(
            !r.fallback,
            "function query without () should match label with ()"
        );
        assert_eq!(r.entry_points.len(), 1);
        assert_eq!(r.entry_points[0].1, "notify_should_wakeup()");
    }

    #[test]
    fn test_long_identifier_filters_out_short_noise() {
        let mut g = ServeGraph::new_directed();
        g.add_node(
            "fn_id",
            "transition_to_notified_by_val()",
            "state.rs",
            "",
            None,
        );
        g.add_node("task1", "task", "task1.rs", "", None);
        g.add_node("task2", "task", "task2.rs", "", None);
        // "task" (4 chars) should be filtered when "transition_to_notified_by_val" (33 chars) present
        let r = context_query(&g, "transition_to_notified_by_val task state", None);
        assert!(!r.fallback);
        assert_eq!(
            r.entry_points.len(),
            1,
            "only specific function, not generic 'task' nodes"
        );
        assert_eq!(r.entry_points[0].1, "transition_to_notified_by_val()");
    }

    #[test]
    fn test_exact_match_entry_points() {
        let g = make_graph();
        let r = context_query(&g, "QueryHandler", None);
        assert!(!r.fallback);
        assert_eq!(r.entry_points.len(), 1);
        assert_eq!(r.entry_points[0].1, "QueryHandler");
    }

    #[test]
    fn test_exact_match_callers() {
        let g = make_graph();
        let r = context_query(&g, "QueryHandler", None);
        assert_eq!(r.callers.len(), 1);
        assert_eq!(r.callers[0].1, "dispatch");
    }

    #[test]
    fn test_exact_match_callees() {
        let g = make_graph();
        let r = context_query(&g, "QueryHandler", None);
        assert_eq!(r.callees.len(), 1);
        assert_eq!(r.callees[0].1, "fetch_data");
    }

    #[test]
    fn test_calls_only_not_method_edges() {
        let g = make_graph();
        let r = context_query(&g, "QueryHandler", None);
        // "SomeOther" connects via "method" edge — must NOT appear in callers
        assert!(!r.callers.iter().any(|(_, label, _)| label == "SomeOther"));
    }

    #[test]
    fn test_case_insensitive_match() {
        let g = make_graph();
        let r = context_query(&g, "queryhandler", None);
        assert!(!r.fallback);
        assert_eq!(r.entry_points.len(), 1);
        assert_eq!(r.entry_points[0].1, "QueryHandler");
    }

    #[test]
    fn test_no_match_returns_fallback() {
        let g = make_graph();
        let r = context_query(&g, "nonexistent_xyz_symbol", None);
        assert!(r.fallback);
        assert!(r.callers.is_empty());
        assert!(r.callees.is_empty());
    }

    #[test]
    fn test_entry_point_not_in_its_own_callers_or_callees() {
        let g = make_graph();
        let r = context_query(&g, "QueryHandler", None);
        assert!(!r.callers.iter().any(|(_, l, _)| l == "QueryHandler"));
        assert!(!r.callees.iter().any(|(_, l, _)| l == "QueryHandler"));
    }

    // --- Fix 2: dedup entry_points by (label, source_file) ---

    #[test]
    fn test_dedup_same_label_same_file() {
        let mut g = ServeGraph::new_directed();
        g.add_node("auth1", "authenticate", "auth/backends.py", "", None);
        g.add_node("auth2", "authenticate", "auth/backends.py", "", None);
        g.add_node("auth3", "authenticate", "auth/backends.py", "", None);
        let r = context_query(&g, "authenticate", None);
        assert!(!r.fallback);
        assert_eq!(
            r.entry_points.len(),
            1,
            "duplicate (label, source_file) pairs should be deduped to one"
        );
    }

    #[test]
    fn test_dedup_same_label_different_file() {
        let mut g = ServeGraph::new_directed();
        g.add_node("auth1", "authenticate", "module_a/backends.py", "", None);
        g.add_node("auth2", "authenticate", "module_b/backends.py", "", None);
        let r = context_query(&g, "authenticate", None);
        assert!(!r.fallback);
        assert_eq!(
            r.entry_points.len(),
            2,
            "same label but different source files should both appear"
        );
    }

    // --- Fix 3: length filter >= 8 → >= 6 ---

    #[test]
    fn test_filter_keeps_six_char_module_name() {
        let mut g = ServeGraph::new_directed();
        g.add_node("mod", "django", "app.py", "", None);
        g.add_node("auth", "authentication", "auth.py", "", None);
        let r = context_query(&g, "django authentication", None);
        assert!(!r.fallback);
        let labels: Vec<&str> = r.entry_points.iter().map(|(_, l, _)| l.as_str()).collect();
        assert!(
            labels.contains(&"django"),
            "'django' (6 chars) should be kept by filter; got: {:?}",
            labels
        );
    }

    #[test]
    fn test_filter_drops_five_char_stopword() {
        let mut g = ServeGraph::new_directed();
        g.add_node("where_node", "where", "app.py", "", None);
        g.add_node("handler", "QueryHandler", "handlers.py", "", None);
        let r = context_query(&g, "where is QueryHandler", None);
        let labels: Vec<&str> = r.entry_points.iter().map(|(_, l, _)| l.as_str()).collect();
        assert!(
            !labels.contains(&"where"),
            "'where' (5 chars) should still be filtered out; got: {:?}",
            labels
        );
        assert!(
            labels.contains(&"QueryHandler"),
            "'QueryHandler' should be matched; got: {:?}",
            labels
        );
    }

    #[test]
    fn test_filter_not_applied_when_max_below_ten() {
        let mut g = ServeGraph::new_directed();
        g.add_node("u", "User", "models.py", "", None);
        g.add_node("l", "login", "auth.py", "", None);
        // "User" (4) + "login" (5) → max = 5 < 10, no filter applies
        let r = context_query(&g, "User login", None);
        assert!(!r.fallback);
        let labels: Vec<&str> = r.entry_points.iter().map(|(_, l, _)| l.as_str()).collect();
        assert!(
            labels.contains(&"User"),
            "'User' should be kept; got: {:?}",
            labels
        );
        assert!(
            labels.contains(&"login"),
            "'login' should be kept; got: {:?}",
            labels
        );
    }
}
