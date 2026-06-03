use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};

use crate::types::{Edge, ExtractionFragment, Node};

pub fn ingest_scip_json(doc: &Value, source_file: &str, language: &str) -> ExtractionFragment {
    let mut nodes: Vec<Node> = Vec::new();
    let mut edges: Vec<Edge> = Vec::new();
    let mut seen_node_ids: HashSet<String> = HashSet::new();
    let mut seen_edges: HashSet<(String, String, String, String)> = HashSet::new();

    let obj = match doc.as_object() {
        Some(o) => o,
        None => return ExtractionFragment { nodes, edges },
    };

    let documents = match obj.get("documents").and_then(|v| v.as_array()) {
        Some(d) => d,
        None => return ExtractionFragment { nodes, edges },
    };

    // Pass 1: build symbol → node_id indices
    let mut per_doc_index: HashMap<(String, String), String> = HashMap::new();
    let mut global_index: HashMap<String, Vec<String>> = HashMap::new();
    let mut symbol_records: Vec<SymbolRecord> = Vec::new();

    for document in documents {
        let document = match document.as_object() {
            Some(d) => d,
            None => continue,
        };
        let doc_path = coerce_str(document.get("relative_path"), source_file);
        let doc_language = coerce_str(document.get("language"), language);
        let symbols = match document.get("symbols").and_then(|v| v.as_array()) {
            Some(s) => s,
            None => continue,
        };
        for symbol in symbols {
            let symbol_obj = match symbol.as_object() {
                Some(s) => s,
                None => continue,
            };
            let symbol_id = coerce_str(symbol_obj.get("symbol"), "");
            if symbol_id.is_empty() {
                continue;
            }
            let node_id = make_scip_node_id(symbol_id, doc_path);
            per_doc_index
                .entry((symbol_id.to_string(), doc_path.to_string()))
                .or_insert_with(|| node_id.clone());
            let candidates = global_index.entry(symbol_id.to_string()).or_default();
            if !candidates.contains(&node_id) {
                candidates.push(node_id.clone());
            }
            symbol_records.push(SymbolRecord {
                node_id,
                symbol_id: symbol_id.to_string(),
                doc_path: doc_path.to_string(),
                language: doc_language.to_string(),
                raw: symbol.clone(),
            });
        }
    }

    // Pass 2: emit nodes + relationship edges
    for record in &symbol_records {
        emit_symbol_node(record, &mut nodes, &mut seen_node_ids);
        emit_relationships(
            record,
            &per_doc_index,
            &global_index,
            &mut nodes,
            &mut edges,
            &mut seen_node_ids,
            &mut seen_edges,
        );
    }

    ExtractionFragment { nodes, edges }
}

struct SymbolRecord {
    node_id: String,
    symbol_id: String,
    doc_path: String,
    #[allow(dead_code)]
    language: String,
    raw: Value,
}

fn emit_symbol_node(
    record: &SymbolRecord,
    nodes: &mut Vec<Node>,
    seen_node_ids: &mut HashSet<String>,
) {
    if seen_node_ids.contains(&record.node_id) {
        return;
    }
    let raw = &record.raw;
    let kind = coerce_str(raw.get("kind"), "unknown");
    let display_name = coerce_str(raw.get("display_name"), "");
    let description = raw
        .get("documentation")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let occurrences = raw.get("occurrences").unwrap_or(&Value::Null);
    let sourceline = first_occurrence_line(occurrences);
    let suffix = if record.symbol_id.contains('#') {
        record
            .symbol_id
            .rsplit('#')
            .next()
            .unwrap_or(&record.symbol_id)
    } else {
        &record.symbol_id
    };
    let label = if !display_name.is_empty() {
        display_name.to_string()
    } else if !suffix.is_empty() {
        suffix.to_string()
    } else {
        record.symbol_id.clone()
    };
    seen_node_ids.insert(record.node_id.clone());
    nodes.push(Node {
        id: record.node_id.clone(),
        label,
        file_type: scip_kind_to_file_type(kind).to_string(),
        source_file: record.doc_path.clone(),
        source_location: if sourceline > 0 {
            Some(format!("L{}", sourceline))
        } else {
            None
        },
        community: None,
        rationale: None,
        docstring: None,
        metadata: build_scip_metadata(&record.symbol_id, kind, &description),
    });
}

fn emit_relationships(
    record: &SymbolRecord,
    per_doc_index: &HashMap<(String, String), String>,
    global_index: &HashMap<String, Vec<String>>,
    nodes: &mut Vec<Node>,
    edges: &mut Vec<Edge>,
    seen_node_ids: &mut HashSet<String>,
    seen_edges: &mut HashSet<(String, String, String, String)>,
) {
    let raw = &record.raw;
    let occurrences = raw.get("occurrences").unwrap_or(&Value::Null);
    let sourceline = first_occurrence_line(occurrences);
    let relationships = match raw.get("relationships").and_then(|v| v.as_array()) {
        Some(r) => r,
        None => return,
    };
    for rel in relationships {
        let rel_obj = match rel.as_object() {
            Some(r) => r,
            None => continue,
        };
        let target_symbol = coerce_str(rel_obj.get("symbol"), "");
        if target_symbol.is_empty() {
            continue;
        }
        let target_node_id = match resolve_relationship_target(
            target_symbol,
            &record.doc_path,
            per_doc_index,
            global_index,
        ) {
            Some(id) => id,
            None => {
                let stub_id = make_scip_node_id(target_symbol, &record.doc_path);
                if !seen_node_ids.contains(&stub_id) {
                    seen_node_ids.insert(stub_id.clone());
                    let suffix = if target_symbol.contains('#') {
                        target_symbol.rsplit('#').next().unwrap_or(target_symbol)
                    } else {
                        target_symbol
                    };
                    nodes.push(Node {
                        id: stub_id.clone(),
                        label: if suffix.is_empty() {
                            target_symbol.to_string()
                        } else {
                            suffix.to_string()
                        },
                        file_type: "code".to_string(),
                        source_file: record.doc_path.clone(),
                        source_location: None,
                        community: None,
                        rationale: None,
                        docstring: None,
                        metadata: build_scip_metadata(target_symbol, "external", ""),
                    });
                }
                stub_id
            }
        };
        let relation = scip_relation_for(rel);
        let source_location = if sourceline > 0 {
            format!("L{}", sourceline)
        } else {
            String::new()
        };
        let key = (
            record.node_id.clone(),
            target_node_id.clone(),
            relation.to_string(),
            source_location.clone(),
        );
        if seen_edges.contains(&key) {
            continue;
        }
        seen_edges.insert(key);
        edges.push(Edge {
            source: record.node_id.clone(),
            target: target_node_id,
            relation: relation.to_string(),
            confidence: "EXTRACTED".to_string(),
            source_file: Some(record.doc_path.clone()),
            weight: 1.0,
            context: None,
        });
    }
}

fn resolve_relationship_target(
    target_symbol: &str,
    source_doc_path: &str,
    per_doc_index: &HashMap<(String, String), String>,
    global_index: &HashMap<String, Vec<String>>,
) -> Option<String> {
    if let Some(id) = per_doc_index.get(&(target_symbol.to_string(), source_doc_path.to_string())) {
        return Some(id.clone());
    }
    let candidates = global_index.get(target_symbol)?;
    if candidates.len() == 1 {
        return Some(candidates[0].clone());
    }
    None
}

fn is_true(value: Option<&Value>) -> bool {
    matches!(value, Some(Value::Bool(true)))
}

fn scip_relation_for(rel: &Value) -> &'static str {
    let obj = match rel.as_object() {
        Some(o) => o,
        None => return "scip_ref",
    };
    if is_true(obj.get("is_implementation")) {
        return "scip_impl";
    }
    if is_true(obj.get("is_type_definition")) {
        return "scip_typed";
    }
    if is_true(obj.get("is_definition")) {
        return "scip_def";
    }
    "scip_ref"
}

fn first_occurrence_line(occurrences: &Value) -> u32 {
    let arr = match occurrences.as_array() {
        Some(a) if !a.is_empty() => a,
        _ => return 0,
    };
    let first = match arr[0].as_object() {
        Some(o) => o,
        None => return 0,
    };
    let range = match first.get("range").and_then(|v| v.as_array()) {
        Some(r) if !r.is_empty() => r,
        _ => return 0,
    };
    match range[0].as_u64() {
        Some(line) => line as u32,
        None => 0,
    }
}

fn coerce_str<'a>(value: Option<&'a Value>, default: &'a str) -> &'a str {
    match value {
        Some(Value::String(s)) => s.as_str(),
        _ => default,
    }
}

fn make_scip_node_id(symbol: &str, source_file: &str) -> String {
    let raw = format!("{}:{}", source_file, symbol);
    let mut hasher = Sha256::new();
    hasher.update(raw.as_bytes());
    let hash = hasher.finalize();
    let hex = format!("{:x}", hash);
    let h = &hex[..12];
    let suffix: String = symbol
        .rsplit('#')
        .next()
        .unwrap_or(symbol)
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches('_')
        .to_lowercase()
        .chars()
        .collect();
    if suffix.is_empty() {
        format!("scip_{}", h)
    } else {
        format!("scip_{}_{}", suffix, h)
    }
}

fn scip_kind_to_file_type(_kind: &str) -> &'static str {
    "code"
}

fn build_scip_metadata(symbol_id: &str, kind: &str, description: &str) -> HashMap<String, String> {
    let mut meta = HashMap::new();
    meta.insert("scip_symbol".to_string(), symbol_id.to_string());
    meta.insert("scip_kind".to_string(), kind.to_string());
    if !description.is_empty() {
        meta.insert("scip_description".to_string(), description.to_string());
    }
    meta
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // --- ingest_scip_json: invalid inputs ---

    #[test]
    fn test_non_dict_input_returns_empty() {
        let result = ingest_scip_json(&json!(null), "", "python");
        assert_eq!(result.nodes.len(), 0);
        assert_eq!(result.edges.len(), 0);
    }

    #[test]
    fn test_array_input_returns_empty() {
        let result = ingest_scip_json(&json!([1, 2, 3]), "", "python");
        assert_eq!(result.nodes.len(), 0);
    }

    #[test]
    fn test_string_input_returns_empty() {
        let result = ingest_scip_json(&json!("hello"), "", "python");
        assert_eq!(result.nodes.len(), 0);
    }

    #[test]
    fn test_no_documents_key_returns_empty() {
        let result = ingest_scip_json(&json!({"other": "data"}), "", "python");
        assert_eq!(result.nodes.len(), 0);
    }

    #[test]
    fn test_documents_not_array_returns_empty() {
        let result = ingest_scip_json(&json!({"documents": "bad"}), "", "python");
        assert_eq!(result.nodes.len(), 0);
    }

    #[test]
    fn test_empty_documents_array() {
        let result = ingest_scip_json(&json!({"documents": []}), "", "python");
        assert_eq!(result.nodes.len(), 0);
        assert_eq!(result.edges.len(), 0);
    }

    #[test]
    fn test_non_dict_document_skipped() {
        let result = ingest_scip_json(&json!({"documents": ["bad", 42, null]}), "", "python");
        assert_eq!(result.nodes.len(), 0);
    }

    // --- basic node emission ---

    #[test]
    fn test_single_symbol_produces_one_node() {
        let doc = json!({
            "documents": [{
                "relative_path": "foo.py",
                "language": "python",
                "symbols": [{
                    "symbol": "foo.py#MyClass",
                    "kind": "class",
                    "display_name": "MyClass",
                    "documentation": ["A class doc"],
                    "relationships": [],
                    "occurrences": [{"range": [10, 0, 10, 7], "symbol": "foo.py#MyClass", "symbol_roles": 1}]
                }]
            }]
        });
        let result = ingest_scip_json(&doc, "foo.py", "python");
        assert_eq!(result.nodes.len(), 1);
        assert_eq!(result.nodes[0].label, "MyClass");
        assert_eq!(result.nodes[0].file_type, "code");
        assert_eq!(result.nodes[0].source_file, "foo.py");
        assert_eq!(result.nodes[0].source_location, Some("L10".to_string()));
        assert_eq!(
            result.nodes[0]
                .metadata
                .get("scip_kind")
                .map(|s| s.as_str()),
            Some("class")
        );
        assert_eq!(
            result.nodes[0]
                .metadata
                .get("scip_description")
                .map(|s| s.as_str()),
            Some("A class doc")
        );
    }

    #[test]
    fn test_multiple_symbols_multiple_nodes() {
        let doc = json!({
            "documents": [{
                "relative_path": "bar.py",
                "language": "python",
                "symbols": [
                    {"symbol": "bar.py#Foo", "kind": "class", "display_name": "Foo", "relationships": []},
                    {"symbol": "bar.py#bar_fn", "kind": "function", "display_name": "bar_fn", "relationships": []}
                ]
            }]
        });
        let result = ingest_scip_json(&doc, "", "python");
        assert_eq!(result.nodes.len(), 2);
        let labels: Vec<&str> = result.nodes.iter().map(|n| n.label.as_str()).collect();
        assert!(labels.contains(&"Foo"));
        assert!(labels.contains(&"bar_fn"));
    }

    #[test]
    fn test_symbol_without_display_name_uses_suffix() {
        let doc = json!({
            "documents": [{
                "relative_path": "a.py",
                "language": "python",
                "symbols": [{
                    "symbol": "pkg/mod#MyFunc",
                    "kind": "function",
                    "relationships": []
                }]
            }]
        });
        let result = ingest_scip_json(&doc, "", "python");
        assert_eq!(result.nodes.len(), 1);
        assert_eq!(result.nodes[0].label, "MyFunc");
    }

    #[test]
    fn test_symbol_with_empty_symbol_id_skipped() {
        let doc = json!({
            "documents": [{
                "relative_path": "x.py",
                "language": "python",
                "symbols": [
                    {"symbol": "", "kind": "function", "display_name": "empty", "relationships": []},
                    {"symbol": "x.py#Valid", "kind": "class", "display_name": "Valid", "relationships": []}
                ]
            }]
        });
        let result = ingest_scip_json(&doc, "", "python");
        assert_eq!(result.nodes.len(), 1);
        assert_eq!(result.nodes[0].label, "Valid");
    }

    #[test]
    fn test_node_id_is_stable_and_unique() {
        let doc = json!({
            "documents": [{
                "relative_path": "stable.py",
                "language": "python",
                "symbols": [
                    {"symbol": "stable.py#A", "kind": "class", "display_name": "A", "relationships": []},
                    {"symbol": "stable.py#B", "kind": "class", "display_name": "B", "relationships": []}
                ]
            }]
        });
        let result = ingest_scip_json(&doc, "", "python");
        assert_eq!(result.nodes.len(), 2);
        let id_a = result.nodes[0].id.clone();
        let id_b = result.nodes[1].id.clone();
        assert_ne!(id_a, id_b);
        // Re-ingest should produce same IDs
        let result2 = ingest_scip_json(&doc, "", "python");
        assert_eq!(result2.nodes[0].id, id_a);
        assert_eq!(result2.nodes[1].id, id_b);
    }

    #[test]
    fn test_node_id_starts_with_scip_prefix() {
        let doc = json!({
            "documents": [{
                "relative_path": "x.py",
                "language": "python",
                "symbols": [{"symbol": "x.py#Foo", "kind": "class", "display_name": "Foo", "relationships": []}]
            }]
        });
        let result = ingest_scip_json(&doc, "", "python");
        assert!(result.nodes[0].id.starts_with("scip_"));
    }

    // --- occurrence / source_location ---

    #[test]
    fn test_no_occurrences_no_source_location() {
        let doc = json!({
            "documents": [{
                "relative_path": "x.py",
                "language": "python",
                "symbols": [{
                    "symbol": "x.py#NoLoc",
                    "kind": "function",
                    "display_name": "NoLoc",
                    "relationships": [],
                    "occurrences": []
                }]
            }]
        });
        let result = ingest_scip_json(&doc, "", "python");
        assert_eq!(result.nodes[0].source_location, None);
    }

    #[test]
    fn test_occurrence_line_zero_no_source_location() {
        let doc = json!({
            "documents": [{
                "relative_path": "x.py",
                "language": "python",
                "symbols": [{
                    "symbol": "x.py#ZeroLine",
                    "kind": "function",
                    "display_name": "ZeroLine",
                    "relationships": [],
                    "occurrences": [{"range": [0, 0, 0, 5]}]
                }]
            }]
        });
        let result = ingest_scip_json(&doc, "", "python");
        assert_eq!(result.nodes[0].source_location, None);
    }

    #[test]
    fn test_occurrence_line_positive_sets_source_location() {
        let doc = json!({
            "documents": [{
                "relative_path": "x.py",
                "language": "python",
                "symbols": [{
                    "symbol": "x.py#HasLine",
                    "kind": "function",
                    "display_name": "HasLine",
                    "relationships": [],
                    "occurrences": [{"range": [42, 0, 42, 8]}]
                }]
            }]
        });
        let result = ingest_scip_json(&doc, "", "python");
        assert_eq!(result.nodes[0].source_location, Some("L42".to_string()));
    }

    // --- documentation ---

    #[test]
    fn test_first_doc_string_becomes_description() {
        let doc = json!({
            "documents": [{
                "relative_path": "d.py",
                "language": "python",
                "symbols": [{
                    "symbol": "d.py#Fn",
                    "kind": "function",
                    "display_name": "Fn",
                    "documentation": ["First doc", "Second doc"],
                    "relationships": []
                }]
            }]
        });
        let result = ingest_scip_json(&doc, "", "python");
        assert_eq!(
            result.nodes[0]
                .metadata
                .get("scip_description")
                .map(|s| s.as_str()),
            Some("First doc")
        );
    }

    #[test]
    fn test_empty_docs_no_description_key() {
        let doc = json!({
            "documents": [{
                "relative_path": "d.py",
                "language": "python",
                "symbols": [{
                    "symbol": "d.py#Fn",
                    "kind": "function",
                    "display_name": "Fn",
                    "documentation": [],
                    "relationships": []
                }]
            }]
        });
        let result = ingest_scip_json(&doc, "", "python");
        assert!(!result.nodes[0].metadata.contains_key("scip_description"));
    }

    // --- relationships ---

    #[test]
    fn test_reference_relationship_produces_edge() {
        let doc = json!({
            "documents": [{
                "relative_path": "r.py",
                "language": "python",
                "symbols": [
                    {
                        "symbol": "r.py#Caller",
                        "kind": "function",
                        "display_name": "Caller",
                        "relationships": [{"symbol": "r.py#Callee", "is_reference": true}]
                    },
                    {
                        "symbol": "r.py#Callee",
                        "kind": "function",
                        "display_name": "Callee",
                        "relationships": []
                    }
                ]
            }]
        });
        let result = ingest_scip_json(&doc, "", "python");
        assert_eq!(result.nodes.len(), 2);
        assert_eq!(result.edges.len(), 1);
        assert_eq!(result.edges[0].relation, "scip_ref");
    }

    #[test]
    fn test_is_implementation_relation() {
        let doc = json!({
            "documents": [{
                "relative_path": "impl.py",
                "language": "python",
                "symbols": [
                    {
                        "symbol": "impl.py#Concrete",
                        "kind": "class",
                        "display_name": "Concrete",
                        "relationships": [{"symbol": "impl.py#Interface", "is_implementation": true}]
                    },
                    {
                        "symbol": "impl.py#Interface",
                        "kind": "interface",
                        "display_name": "Interface",
                        "relationships": []
                    }
                ]
            }]
        });
        let result = ingest_scip_json(&doc, "", "python");
        let impl_edges: Vec<&Edge> = result
            .edges
            .iter()
            .filter(|e| e.relation == "scip_impl")
            .collect();
        assert_eq!(impl_edges.len(), 1);
    }

    #[test]
    fn test_is_type_definition_relation() {
        let doc = json!({
            "documents": [{
                "relative_path": "td.py",
                "language": "python",
                "symbols": [
                    {
                        "symbol": "td.py#Alias",
                        "kind": "type",
                        "display_name": "Alias",
                        "relationships": [{"symbol": "td.py#Base", "is_type_definition": true}]
                    },
                    {"symbol": "td.py#Base", "kind": "class", "display_name": "Base", "relationships": []}
                ]
            }]
        });
        let result = ingest_scip_json(&doc, "", "python");
        let typed_edges: Vec<&Edge> = result
            .edges
            .iter()
            .filter(|e| e.relation == "scip_typed")
            .collect();
        assert_eq!(typed_edges.len(), 1);
    }

    #[test]
    fn test_is_definition_relation() {
        let doc = json!({
            "documents": [{
                "relative_path": "def.py",
                "language": "python",
                "symbols": [
                    {
                        "symbol": "def.py#A",
                        "kind": "function",
                        "display_name": "A",
                        "relationships": [{"symbol": "def.py#B", "is_definition": true}]
                    },
                    {"symbol": "def.py#B", "kind": "function", "display_name": "B", "relationships": []}
                ]
            }]
        });
        let result = ingest_scip_json(&doc, "", "python");
        let def_edges: Vec<&Edge> = result
            .edges
            .iter()
            .filter(|e| e.relation == "scip_def")
            .collect();
        assert_eq!(def_edges.len(), 1);
    }

    #[test]
    fn test_string_true_is_not_is_true() {
        // Guard against "false" string or other truthy values being accepted
        let doc = json!({
            "documents": [{
                "relative_path": "x.py",
                "language": "python",
                "symbols": [
                    {
                        "symbol": "x.py#A",
                        "kind": "function",
                        "display_name": "A",
                        "relationships": [{"symbol": "x.py#B", "is_implementation": "true"}]
                    },
                    {"symbol": "x.py#B", "kind": "function", "display_name": "B", "relationships": []}
                ]
            }]
        });
        let result = ingest_scip_json(&doc, "", "python");
        // "true" string should not be treated as is_implementation
        let impl_edges: Vec<&Edge> = result
            .edges
            .iter()
            .filter(|e| e.relation == "scip_impl")
            .collect();
        assert_eq!(impl_edges.len(), 0);
        // But edge should still be emitted as scip_ref
        assert_eq!(result.edges.len(), 1);
        assert_eq!(result.edges[0].relation, "scip_ref");
    }

    // --- cross-document resolution ---

    #[test]
    fn test_cross_document_unique_resolution() {
        let doc = json!({
            "documents": [
                {
                    "relative_path": "a.py",
                    "language": "python",
                    "symbols": [{
                        "symbol": "a.py#Fn",
                        "kind": "function",
                        "display_name": "Fn",
                        "relationships": [{"symbol": "b.py#Helper", "is_reference": true}]
                    }]
                },
                {
                    "relative_path": "b.py",
                    "language": "python",
                    "symbols": [{
                        "symbol": "b.py#Helper",
                        "kind": "function",
                        "display_name": "Helper",
                        "relationships": []
                    }]
                }
            ]
        });
        let result = ingest_scip_json(&doc, "", "python");
        assert_eq!(result.nodes.len(), 2);
        assert_eq!(result.edges.len(), 1);
        // Target should be the real Helper node, not a stub
        let target = &result.edges[0].target;
        let helper_node = result.nodes.iter().find(|n| n.label == "Helper").unwrap();
        assert_eq!(target, &helper_node.id);
    }

    #[test]
    fn test_ambiguous_cross_document_creates_stub() {
        // Same symbol defined in two documents → ambiguous → stub
        let doc = json!({
            "documents": [
                {
                    "relative_path": "a.py",
                    "language": "python",
                    "symbols": [{
                        "symbol": "a.py#Caller",
                        "kind": "function",
                        "display_name": "Caller",
                        "relationships": [{"symbol": "shared#Fn", "is_reference": true}]
                    }]
                },
                {
                    "relative_path": "b.py",
                    "language": "python",
                    "symbols": [{"symbol": "shared#Fn", "kind": "function", "display_name": "FnB", "relationships": []}]
                },
                {
                    "relative_path": "c.py",
                    "language": "python",
                    "symbols": [{"symbol": "shared#Fn", "kind": "function", "display_name": "FnC", "relationships": []}]
                }
            ]
        });
        let result = ingest_scip_json(&doc, "", "python");
        // Should have 3 real nodes + 1 stub
        assert_eq!(result.nodes.len(), 4);
        let stubs: Vec<&Node> = result
            .nodes
            .iter()
            .filter(|n| n.metadata.get("scip_kind").map(|s| s.as_str()) == Some("external"))
            .collect();
        assert_eq!(stubs.len(), 1);
    }

    #[test]
    fn test_external_symbol_creates_stub_node() {
        let doc = json!({
            "documents": [{
                "relative_path": "main.py",
                "language": "python",
                "symbols": [{
                    "symbol": "main.py#MyFn",
                    "kind": "function",
                    "display_name": "MyFn",
                    "relationships": [{"symbol": "external.lib#ExternalFn", "is_reference": true}]
                }]
            }]
        });
        let result = ingest_scip_json(&doc, "", "python");
        assert_eq!(result.nodes.len(), 2);
        let stub = result
            .nodes
            .iter()
            .find(|n| n.metadata.get("scip_kind").map(|s| s.as_str()) == Some("external"));
        assert!(stub.is_some());
    }

    // --- deduplication ---

    #[test]
    fn test_duplicate_symbol_in_same_doc_deduped() {
        let doc = json!({
            "documents": [{
                "relative_path": "dup.py",
                "language": "python",
                "symbols": [
                    {"symbol": "dup.py#Foo", "kind": "class", "display_name": "Foo", "relationships": []},
                    {"symbol": "dup.py#Foo", "kind": "class", "display_name": "Foo", "relationships": []}
                ]
            }]
        });
        let result = ingest_scip_json(&doc, "", "python");
        assert_eq!(result.nodes.len(), 1);
    }

    #[test]
    fn test_duplicate_edges_deduped() {
        let doc = json!({
            "documents": [{
                "relative_path": "dup.py",
                "language": "python",
                "symbols": [
                    {
                        "symbol": "dup.py#A",
                        "kind": "function",
                        "display_name": "A",
                        "relationships": [
                            {"symbol": "dup.py#B", "is_reference": true},
                            {"symbol": "dup.py#B", "is_reference": true}
                        ]
                    },
                    {"symbol": "dup.py#B", "kind": "function", "display_name": "B", "relationships": []}
                ]
            }]
        });
        let result = ingest_scip_json(&doc, "", "python");
        assert_eq!(result.edges.len(), 1);
    }

    // --- fallback values ---

    #[test]
    fn test_relative_path_from_doc_overrides_default() {
        let doc = json!({
            "documents": [{
                "relative_path": "override/path.py",
                "language": "python",
                "symbols": [{
                    "symbol": "override/path.py#Fn",
                    "kind": "function",
                    "display_name": "Fn",
                    "relationships": []
                }]
            }]
        });
        let result = ingest_scip_json(&doc, "default.py", "python");
        assert_eq!(result.nodes[0].source_file, "override/path.py");
    }

    #[test]
    fn test_default_source_file_used_when_no_relative_path() {
        let doc = json!({
            "documents": [{
                "language": "python",
                "symbols": [{
                    "symbol": "mod#Fn",
                    "kind": "function",
                    "display_name": "Fn",
                    "relationships": []
                }]
            }]
        });
        let result = ingest_scip_json(&doc, "fallback.py", "python");
        assert_eq!(result.nodes[0].source_file, "fallback.py");
    }

    #[test]
    fn test_relationship_missing_symbol_skipped() {
        let doc = json!({
            "documents": [{
                "relative_path": "x.py",
                "language": "python",
                "symbols": [{
                    "symbol": "x.py#A",
                    "kind": "function",
                    "display_name": "A",
                    "relationships": [{"is_reference": true}]
                }]
            }]
        });
        let result = ingest_scip_json(&doc, "", "python");
        assert_eq!(result.nodes.len(), 1);
        assert_eq!(result.edges.len(), 0);
    }

    #[test]
    fn test_non_dict_relationship_skipped() {
        let doc = json!({
            "documents": [{
                "relative_path": "x.py",
                "language": "python",
                "symbols": [{
                    "symbol": "x.py#A",
                    "kind": "function",
                    "display_name": "A",
                    "relationships": ["bad", 42, null]
                }]
            }]
        });
        let result = ingest_scip_json(&doc, "", "python");
        assert_eq!(result.nodes.len(), 1);
        assert_eq!(result.edges.len(), 0);
    }

    // --- multi-document ---

    #[test]
    fn test_multiple_documents_all_processed() {
        let doc = json!({
            "documents": [
                {
                    "relative_path": "mod1.py",
                    "language": "python",
                    "symbols": [{"symbol": "mod1.py#A", "kind": "class", "display_name": "A", "relationships": []}]
                },
                {
                    "relative_path": "mod2.py",
                    "language": "python",
                    "symbols": [{"symbol": "mod2.py#B", "kind": "class", "display_name": "B", "relationships": []}]
                }
            ]
        });
        let result = ingest_scip_json(&doc, "", "python");
        assert_eq!(result.nodes.len(), 2);
        let files: Vec<&str> = result
            .nodes
            .iter()
            .map(|n| n.source_file.as_str())
            .collect();
        assert!(files.contains(&"mod1.py"));
        assert!(files.contains(&"mod2.py"));
    }

    #[test]
    fn test_edge_confidence_is_extracted() {
        let doc = json!({
            "documents": [{
                "relative_path": "e.py",
                "language": "python",
                "symbols": [
                    {
                        "symbol": "e.py#A",
                        "kind": "function",
                        "display_name": "A",
                        "relationships": [{"symbol": "e.py#B", "is_reference": true}]
                    },
                    {"symbol": "e.py#B", "kind": "function", "display_name": "B", "relationships": []}
                ]
            }]
        });
        let result = ingest_scip_json(&doc, "", "python");
        assert_eq!(result.edges[0].confidence, "EXTRACTED");
        assert_eq!(result.edges[0].weight, 1.0);
    }

    #[test]
    fn test_scip_symbol_in_metadata() {
        let doc = json!({
            "documents": [{
                "relative_path": "m.py",
                "language": "python",
                "symbols": [{
                    "symbol": "pkg/mod#MyClass",
                    "kind": "class",
                    "display_name": "MyClass",
                    "relationships": []
                }]
            }]
        });
        let result = ingest_scip_json(&doc, "", "python");
        assert_eq!(
            result.nodes[0]
                .metadata
                .get("scip_symbol")
                .map(|s| s.as_str()),
            Some("pkg/mod#MyClass")
        );
    }

    // --- make_scip_node_id ---

    #[test]
    fn test_make_scip_node_id_deterministic() {
        let id1 = make_scip_node_id("pkg#Fn", "file.py");
        let id2 = make_scip_node_id("pkg#Fn", "file.py");
        assert_eq!(id1, id2);
    }

    #[test]
    fn test_make_scip_node_id_different_files_differ() {
        let id1 = make_scip_node_id("pkg#Fn", "a.py");
        let id2 = make_scip_node_id("pkg#Fn", "b.py");
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_make_scip_node_id_starts_with_scip() {
        let id = make_scip_node_id("pkg#Fn", "x.py");
        assert!(id.starts_with("scip_"));
    }

    #[test]
    fn test_first_occurrence_line_non_list_returns_zero() {
        assert_eq!(first_occurrence_line(&json!(null)), 0);
        assert_eq!(first_occurrence_line(&json!("bad")), 0);
        assert_eq!(first_occurrence_line(&json!(42)), 0);
    }

    #[test]
    fn test_first_occurrence_line_empty_list() {
        assert_eq!(first_occurrence_line(&json!([])), 0);
    }

    #[test]
    fn test_first_occurrence_line_valid() {
        let occ = json!([{"range": [5, 0, 5, 3]}]);
        assert_eq!(first_occurrence_line(&occ), 5);
    }

    #[test]
    fn test_is_true_exact_bool() {
        assert!(is_true(Some(&json!(true))));
        assert!(!is_true(Some(&json!(false))));
        assert!(!is_true(Some(&json!("true"))));
        assert!(!is_true(Some(&json!(1))));
        assert!(!is_true(None));
    }
}
