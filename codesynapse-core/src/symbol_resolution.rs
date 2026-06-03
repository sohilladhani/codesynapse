use std::collections::{HashMap, HashSet};

use crate::types::{Edge, Node};

pub fn normalise_callable_label(label: &str) -> String {
    label
        .trim()
        .trim_matches(|c| c == '(' || c == ')')
        .trim_start_matches('.')
        .to_lowercase()
}

pub fn node_is_resolvable_symbol(node: &Node) -> bool {
    if node.file_type != "code" {
        return false;
    }
    let label = node.label.trim();
    if label.is_empty() {
        return false;
    }
    for ext in &[".py", ".js", ".ts", ".tsx", ".java", ".go", ".rs"] {
        if label.ends_with(ext) {
            return false;
        }
    }
    !normalise_callable_label(label).is_empty()
}

pub fn build_label_index(nodes: &[Node]) -> HashMap<String, Vec<String>> {
    let mut index: HashMap<String, Vec<String>> = HashMap::new();
    for node in nodes {
        if !node_is_resolvable_symbol(node) {
            continue;
        }
        let key = normalise_callable_label(&node.label);
        if key.is_empty() {
            continue;
        }
        index.entry(key).or_default().push(node.id.clone());
    }
    index
}

pub fn existing_edge_pairs(edges: &[Edge]) -> HashSet<(String, String, String)> {
    edges
        .iter()
        .map(|e| (e.source.clone(), e.target.clone(), e.relation.clone()))
        .collect()
}

pub fn resolve_cross_file_raw_calls(
    per_file: &[Option<RawCallsFragment>],
    all_nodes: &[Node],
    all_edges: &[Edge],
) -> Vec<Edge> {
    let label_index = build_label_index(all_nodes);
    let mut known_pairs = existing_edge_pairs(all_edges);
    let mut resolved: Vec<Edge> = Vec::new();

    for fragment_opt in per_file {
        let fragment = match fragment_opt {
            Some(f) => f,
            None => continue,
        };
        for raw_call in &fragment.raw_calls {
            let callee = raw_call.callee.trim();
            if callee.is_empty() {
                continue;
            }
            if raw_call.is_member_call {
                continue;
            }
            let candidates = label_index.get(&callee.to_lowercase());
            let candidates = match candidates {
                Some(c) if c.len() == 1 => c,
                _ => continue,
            };
            let target = &candidates[0];
            let caller = raw_call.caller_nid.trim();
            if caller.is_empty() || caller == target.as_str() {
                continue;
            }
            let pair = (caller.to_string(), target.clone(), "calls".to_string());
            if known_pairs.contains(&pair) {
                continue;
            }
            known_pairs.insert(pair);
            resolved.push(Edge {
                source: caller.to_string(),
                target: target.clone(),
                relation: "calls".to_string(),
                confidence: "INFERRED".to_string(),
                source_file: if raw_call.source_file.is_empty() {
                    None
                } else {
                    Some(raw_call.source_file.clone())
                },
                weight: 1.0,
                context: None,
            });
        }
    }
    resolved
}

#[derive(Debug, Clone, Default)]
pub struct RawCall {
    pub callee: String,
    pub caller_nid: String,
    pub is_member_call: bool,
    pub source_file: String,
    pub source_location: String,
}

#[derive(Debug, Clone, Default)]
pub struct RawCallsFragment {
    pub raw_calls: Vec<RawCall>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn make_code_node(id: &str, label: &str) -> Node {
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

    fn make_node_with_file(id: &str, label: &str, file: &str) -> Node {
        Node {
            id: id.to_string(),
            label: label.to_string(),
            file_type: "code".to_string(),
            source_file: file.to_string(),
            source_location: None,
            community: None,
            rationale: None,
            docstring: None,
            metadata: HashMap::new(),
        }
    }

    fn make_doc_node(id: &str, label: &str) -> Node {
        Node {
            id: id.to_string(),
            label: label.to_string(),
            file_type: "document".to_string(),
            source_file: "test.md".to_string(),
            source_location: None,
            community: None,
            rationale: None,
            docstring: None,
            metadata: HashMap::new(),
        }
    }

    fn make_edge(src: &str, tgt: &str, rel: &str) -> Edge {
        Edge {
            source: src.to_string(),
            target: tgt.to_string(),
            relation: rel.to_string(),
            confidence: "EXTRACTED".to_string(),
            source_file: None,
            weight: 1.0,
            context: None,
        }
    }

    fn raw_call(callee: &str, caller_nid: &str) -> RawCall {
        RawCall {
            callee: callee.to_string(),
            caller_nid: caller_nid.to_string(),
            is_member_call: false,
            source_file: "test.py".to_string(),
            source_location: String::new(),
        }
    }

    fn member_call(callee: &str, caller_nid: &str) -> RawCall {
        RawCall {
            callee: callee.to_string(),
            caller_nid: caller_nid.to_string(),
            is_member_call: true,
            source_file: "test.py".to_string(),
            source_location: String::new(),
        }
    }

    // --- normalise_callable_label ---

    #[test]
    fn test_normalise_strips_parens() {
        assert_eq!(normalise_callable_label("foo()"), "foo");
    }

    #[test]
    fn test_normalise_lowercase() {
        assert_eq!(normalise_callable_label("MyFunc"), "myfunc");
    }

    #[test]
    fn test_normalise_strips_leading_dot() {
        assert_eq!(normalise_callable_label(".foo"), "foo");
    }

    #[test]
    fn test_normalise_strips_whitespace() {
        assert_eq!(normalise_callable_label("  bar  "), "bar");
    }

    #[test]
    fn test_normalise_empty() {
        assert_eq!(normalise_callable_label(""), "");
    }

    // --- node_is_resolvable_symbol ---

    #[test]
    fn test_code_node_is_resolvable() {
        let node = make_code_node("n1", "my_fn");
        assert!(node_is_resolvable_symbol(&node));
    }

    #[test]
    fn test_document_node_not_resolvable() {
        let node = make_doc_node("n1", "my_fn");
        assert!(!node_is_resolvable_symbol(&node));
    }

    #[test]
    fn test_empty_label_not_resolvable() {
        let node = make_code_node("n1", "");
        assert!(!node_is_resolvable_symbol(&node));
    }

    #[test]
    fn test_py_extension_label_not_resolvable() {
        let node = make_code_node("n1", "module.py");
        assert!(!node_is_resolvable_symbol(&node));
    }

    #[test]
    fn test_js_extension_label_not_resolvable() {
        let node = make_code_node("n1", "module.js");
        assert!(!node_is_resolvable_symbol(&node));
    }

    #[test]
    fn test_ts_extension_label_not_resolvable() {
        let node = make_code_node("n1", "module.ts");
        assert!(!node_is_resolvable_symbol(&node));
    }

    #[test]
    fn test_tsx_extension_label_not_resolvable() {
        let node = make_code_node("n1", "component.tsx");
        assert!(!node_is_resolvable_symbol(&node));
    }

    #[test]
    fn test_java_extension_label_not_resolvable() {
        let node = make_code_node("n1", "MyClass.java");
        assert!(!node_is_resolvable_symbol(&node));
    }

    #[test]
    fn test_go_extension_label_not_resolvable() {
        let node = make_code_node("n1", "main.go");
        assert!(!node_is_resolvable_symbol(&node));
    }

    #[test]
    fn test_rs_extension_label_not_resolvable() {
        let node = make_code_node("n1", "lib.rs");
        assert!(!node_is_resolvable_symbol(&node));
    }

    // --- build_label_index ---

    #[test]
    fn test_label_index_basic() {
        let nodes = vec![make_code_node("n1", "my_fn")];
        let index = build_label_index(&nodes);
        assert!(index.contains_key("my_fn"));
        assert_eq!(index["my_fn"], vec!["n1"]);
    }

    #[test]
    fn test_label_index_case_insensitive() {
        let nodes = vec![make_code_node("n1", "MyFn")];
        let index = build_label_index(&nodes);
        assert!(index.contains_key("myfn"));
    }

    #[test]
    fn test_label_index_multiple_nodes_same_label() {
        let nodes = vec![
            make_node_with_file("n1", "fn_a", "a.py"),
            make_node_with_file("n2", "fn_a", "b.py"),
        ];
        let index = build_label_index(&nodes);
        assert_eq!(index["fn_a"].len(), 2);
    }

    #[test]
    fn test_label_index_excludes_doc_nodes() {
        let nodes = vec![make_doc_node("n1", "my_fn")];
        let index = build_label_index(&nodes);
        assert!(index.is_empty());
    }

    #[test]
    fn test_label_index_empty_nodes() {
        let index = build_label_index(&[]);
        assert!(index.is_empty());
    }

    // --- existing_edge_pairs ---

    #[test]
    fn test_existing_edge_pairs_basic() {
        let edges = vec![make_edge("a", "b", "calls")];
        let pairs = existing_edge_pairs(&edges);
        assert!(pairs.contains(&("a".to_string(), "b".to_string(), "calls".to_string())));
    }

    #[test]
    fn test_existing_edge_pairs_relation_distinguishes() {
        let edges = vec![
            make_edge("a", "b", "calls"),
            make_edge("a", "b", "contains"),
        ];
        let pairs = existing_edge_pairs(&edges);
        assert_eq!(pairs.len(), 2);
    }

    #[test]
    fn test_existing_edge_pairs_empty() {
        let pairs = existing_edge_pairs(&[]);
        assert!(pairs.is_empty());
    }

    // --- resolve_cross_file_raw_calls ---

    #[test]
    fn test_resolve_unique_callee() {
        let nodes = vec![make_code_node("target_id", "helper")];
        let per_file = vec![Some(RawCallsFragment {
            raw_calls: vec![raw_call("helper", "caller_id")],
        })];
        let resolved = resolve_cross_file_raw_calls(&per_file, &nodes, &[]);
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].source, "caller_id");
        assert_eq!(resolved[0].target, "target_id");
        assert_eq!(resolved[0].relation, "calls");
        assert_eq!(resolved[0].confidence, "INFERRED");
    }

    #[test]
    fn test_resolve_ambiguous_callee_skipped() {
        let nodes = vec![
            make_node_with_file("t1", "helper", "a.py"),
            make_node_with_file("t2", "helper", "b.py"),
        ];
        let per_file = vec![Some(RawCallsFragment {
            raw_calls: vec![raw_call("helper", "caller_id")],
        })];
        let resolved = resolve_cross_file_raw_calls(&per_file, &nodes, &[]);
        assert_eq!(resolved.len(), 0);
    }

    #[test]
    fn test_resolve_member_call_skipped() {
        let nodes = vec![make_code_node("target_id", "do_thing")];
        let per_file = vec![Some(RawCallsFragment {
            raw_calls: vec![member_call("do_thing", "caller_id")],
        })];
        let resolved = resolve_cross_file_raw_calls(&per_file, &nodes, &[]);
        assert_eq!(resolved.len(), 0);
    }

    #[test]
    fn test_resolve_empty_callee_skipped() {
        let nodes = vec![make_code_node("target_id", "fn")];
        let per_file = vec![Some(RawCallsFragment {
            raw_calls: vec![raw_call("", "caller_id")],
        })];
        let resolved = resolve_cross_file_raw_calls(&per_file, &nodes, &[]);
        assert_eq!(resolved.len(), 0);
    }

    #[test]
    fn test_resolve_self_call_skipped() {
        let nodes = vec![make_code_node("caller_id", "my_fn")];
        let per_file = vec![Some(RawCallsFragment {
            raw_calls: vec![raw_call("my_fn", "caller_id")],
        })];
        let resolved = resolve_cross_file_raw_calls(&per_file, &nodes, &[]);
        assert_eq!(resolved.len(), 0);
    }

    #[test]
    fn test_resolve_existing_edge_not_duplicated() {
        let nodes = vec![make_code_node("target_id", "helper")];
        let existing = vec![make_edge("caller_id", "target_id", "calls")];
        let per_file = vec![Some(RawCallsFragment {
            raw_calls: vec![raw_call("helper", "caller_id")],
        })];
        let resolved = resolve_cross_file_raw_calls(&per_file, &nodes, &existing);
        assert_eq!(resolved.len(), 0);
    }

    #[test]
    fn test_resolve_none_fragment_skipped() {
        let nodes = vec![make_code_node("target_id", "helper")];
        let per_file: Vec<Option<RawCallsFragment>> = vec![None];
        let resolved = resolve_cross_file_raw_calls(&per_file, &nodes, &[]);
        assert_eq!(resolved.len(), 0);
    }

    #[test]
    fn test_resolve_multiple_calls_deduped() {
        let nodes = vec![make_code_node("target_id", "helper")];
        let per_file = vec![Some(RawCallsFragment {
            raw_calls: vec![
                raw_call("helper", "caller_id"),
                raw_call("helper", "caller_id"),
            ],
        })];
        let resolved = resolve_cross_file_raw_calls(&per_file, &nodes, &[]);
        assert_eq!(resolved.len(), 1);
    }

    #[test]
    fn test_resolve_case_insensitive_callee() {
        let nodes = vec![make_code_node("target_id", "MyHelper")];
        let per_file = vec![Some(RawCallsFragment {
            raw_calls: vec![raw_call("myhelper", "caller_id")],
        })];
        let resolved = resolve_cross_file_raw_calls(&per_file, &nodes, &[]);
        assert_eq!(resolved.len(), 1);
    }

    #[test]
    fn test_resolve_distinct_relation_does_not_suppress() {
        // A "contains" edge between same endpoints should not suppress a "calls" edge
        let nodes = vec![make_code_node("target_id", "helper")];
        let existing = vec![make_edge("caller_id", "target_id", "contains")];
        let per_file = vec![Some(RawCallsFragment {
            raw_calls: vec![raw_call("helper", "caller_id")],
        })];
        let resolved = resolve_cross_file_raw_calls(&per_file, &nodes, &existing);
        assert_eq!(resolved.len(), 1);
    }

    #[test]
    fn test_resolve_no_nodes_no_edges() {
        let per_file = vec![Some(RawCallsFragment {
            raw_calls: vec![raw_call("helper", "caller_id")],
        })];
        let resolved = resolve_cross_file_raw_calls(&per_file, &[], &[]);
        assert_eq!(resolved.len(), 0);
    }

    #[test]
    fn test_resolve_empty_caller_nid_skipped() {
        let nodes = vec![make_code_node("target_id", "helper")];
        let per_file = vec![Some(RawCallsFragment {
            raw_calls: vec![raw_call("helper", "")],
        })];
        let resolved = resolve_cross_file_raw_calls(&per_file, &nodes, &[]);
        assert_eq!(resolved.len(), 0);
    }

    #[test]
    fn test_resolve_edge_weight_is_one() {
        let nodes = vec![make_code_node("target_id", "helper")];
        let per_file = vec![Some(RawCallsFragment {
            raw_calls: vec![raw_call("helper", "caller_id")],
        })];
        let resolved = resolve_cross_file_raw_calls(&per_file, &nodes, &[]);
        assert_eq!(resolved[0].weight, 1.0);
    }
}
