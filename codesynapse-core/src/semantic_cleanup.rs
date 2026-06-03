use regex::Regex;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::OnceLock;

const RATIONALE_MIN_CHARS: usize = 80;
const RATIONALE_MIN_WORDS: usize = 8;

const VALID_SEMANTIC_FILE_TYPES: &[&str] =
    &["code", "document", "paper", "image", "rationale", "concept"];

fn semantic_id_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[A-Za-z0-9._:-]+$").unwrap())
}

pub struct SemanticLimits {
    pub max_bytes: usize,
    pub max_nodes: usize,
    pub max_edges: usize,
    pub max_hyperedges: usize,
    pub max_hyperedge_nodes: usize,
    pub max_id_length: usize,
}

impl Default for SemanticLimits {
    fn default() -> Self {
        Self {
            max_bytes: 25 * 1024 * 1024,
            max_nodes: 10_000,
            max_edges: 100_000,
            max_hyperedges: 10_000,
            max_hyperedge_nodes: 256,
            max_id_length: 256,
        }
    }
}

pub fn validate_semantic_fragment(fragment: &Value) -> Vec<String> {
    validate_semantic_fragment_with_limits(fragment, &SemanticLimits::default())
}

pub fn validate_semantic_fragment_with_limits(
    fragment: &Value,
    limits: &SemanticLimits,
) -> Vec<String> {
    let obj = match fragment.as_object() {
        Some(o) => o,
        None => return vec!["fragment must be a JSON object".to_string()],
    };

    let mut errors: Vec<String> = Vec::new();

    let payload = serde_json::to_string(fragment).unwrap_or_default();
    let byte_len = payload.len();
    if byte_len > limits.max_bytes {
        errors.push(format!(
            "payload is {} bytes; max is {}",
            byte_len, limits.max_bytes
        ));
    }

    let empty_arr = vec![];
    let nodes = match obj.get("nodes") {
        Some(v) => match v.as_array() {
            Some(arr) => {
                if arr.len() > limits.max_nodes {
                    errors.push(format!(
                        "nodes has {} entries; max is {}",
                        arr.len(),
                        limits.max_nodes
                    ));
                }
                arr
            }
            None => {
                errors.push("nodes must be a list".to_string());
                &empty_arr
            }
        },
        None => &empty_arr,
    };

    let edges = match obj.get("edges") {
        Some(v) => match v.as_array() {
            Some(arr) => {
                if arr.len() > limits.max_edges {
                    errors.push(format!(
                        "edges has {} entries; max is {}",
                        arr.len(),
                        limits.max_edges
                    ));
                }
                arr
            }
            None => {
                errors.push("edges must be a list".to_string());
                &empty_arr
            }
        },
        None => &empty_arr,
    };

    for (i, node) in nodes.iter().enumerate() {
        match node.as_object() {
            None => {
                errors.push(format!("nodes[{i}] must be an object"));
                continue;
            }
            Some(n) => {
                validate_semantic_id(&mut errors, &format!("nodes[{i}].id"), n.get("id"), limits);
                if let Some(ft) = n.get("file_type").and_then(|v| v.as_str()) {
                    if !VALID_SEMANTIC_FILE_TYPES.contains(&ft) {
                        errors.push(format!("nodes[{i}].file_type {ft:?} is not one of {:?}", {
                            let mut v: Vec<&&str> = VALID_SEMANTIC_FILE_TYPES.iter().collect();
                            v.sort();
                            v
                        }));
                    }
                }
            }
        }
    }

    for (i, edge) in edges.iter().enumerate() {
        match edge.as_object() {
            None => {
                errors.push(format!("edges[{i}] must be an object"));
                continue;
            }
            Some(e) => {
                validate_semantic_id(
                    &mut errors,
                    &format!("edges[{i}].source"),
                    e.get("source"),
                    limits,
                );
                validate_semantic_id(
                    &mut errors,
                    &format!("edges[{i}].target"),
                    e.get("target"),
                    limits,
                );
            }
        }
    }

    let hyperedges = match obj.get("hyperedges") {
        None | Some(Value::Null) => &empty_arr,
        Some(v) => match v.as_array() {
            Some(arr) => {
                if arr.len() > limits.max_hyperedges {
                    errors.push(format!(
                        "hyperedges has {} entries; max is {}",
                        arr.len(),
                        limits.max_hyperedges
                    ));
                }
                arr
            }
            None => {
                errors.push("hyperedges must be a list".to_string());
                &empty_arr
            }
        },
    };

    for (i, he) in hyperedges.iter().enumerate() {
        match he.as_object() {
            None => {
                errors.push(format!("hyperedges[{i}] must be an object"));
                continue;
            }
            Some(h) => {
                validate_semantic_id(
                    &mut errors,
                    &format!("hyperedges[{i}].id"),
                    h.get("id"),
                    limits,
                );
                match h.get("nodes") {
                    None => {
                        errors.push(format!("hyperedges[{i}].nodes must be a list"));
                    }
                    Some(v) => match v.as_array() {
                        None => {
                            errors.push(format!("hyperedges[{i}].nodes must be a list"));
                        }
                        Some(he_nodes) => {
                            if he_nodes.len() > limits.max_hyperedge_nodes {
                                errors.push(format!(
                                    "hyperedges[{i}].nodes has {} entries; max is {}",
                                    he_nodes.len(),
                                    limits.max_hyperedge_nodes
                                ));
                            }
                            for (j, r) in he_nodes.iter().enumerate() {
                                validate_semantic_id(
                                    &mut errors,
                                    &format!("hyperedges[{i}].nodes[{j}]"),
                                    Some(r),
                                    limits,
                                );
                            }
                        }
                    },
                }
            }
        }
    }

    errors
}

fn validate_semantic_id(
    errors: &mut Vec<String>,
    field: &str,
    value: Option<&Value>,
    limits: &SemanticLimits,
) {
    match value {
        None | Some(Value::Null) => {
            errors.push(format!("{field} must be a string"));
        }
        Some(v) => match v.as_str() {
            None => {
                errors.push(format!("{field} must be a string"));
            }
            Some(s) => {
                if s.is_empty() {
                    errors.push(format!("{field} must not be empty"));
                    return;
                }
                if s.len() > limits.max_id_length {
                    errors.push(format!(
                        "{field} is {} chars; max is {}",
                        s.len(),
                        limits.max_id_length
                    ));
                }
                if s.contains('/') || s.contains('\\') || s.contains("..") {
                    errors.push(format!("{field} must not contain path separators or '..'"));
                }
                if !semantic_id_re().is_match(s) {
                    errors.push(format!("{field} contains unsupported characters"));
                }
            }
        },
    }
}

pub fn load_validated_semantic_fragment(path: &Path) -> (Option<Value>, Vec<String>) {
    let limits = SemanticLimits::default();
    load_validated_semantic_fragment_with_limits(path, &limits)
}

pub fn load_validated_semantic_fragment_with_limits(
    path: &Path,
    limits: &SemanticLimits,
) -> (Option<Value>, Vec<String>) {
    let size = match std::fs::metadata(path) {
        Err(e) => {
            return (
                None,
                vec![format!("could not stat {}: {}", path.display(), e)],
            )
        }
        Ok(m) => m.len() as usize,
    };
    if size > limits.max_bytes {
        return (
            None,
            vec![format!(
                "payload is {} bytes; max is {}",
                size, limits.max_bytes
            )],
        );
    }
    let text = match std::fs::read_to_string(path) {
        Err(e) => {
            return (
                None,
                vec![format!("could not read {}: {}", path.display(), e)],
            )
        }
        Ok(t) => t,
    };
    let fragment: Value = match serde_json::from_str(&text) {
        Err(e) => return (None, vec![format!("invalid JSON: {}", e)]),
        Ok(v) => v,
    };
    let errors = validate_semantic_fragment_with_limits(&fragment, limits);
    if errors.is_empty() {
        (Some(fragment), vec![])
    } else {
        (None, errors)
    }
}

pub fn sanitize_semantic_fragment(fragment: &mut Value) -> &mut Value {
    let obj = match fragment.as_object_mut() {
        Some(o) => o,
        None => return fragment,
    };

    let invalid_ft: HashSet<&str> = ["rationale", "concept"].iter().copied().collect();

    let nodes: Vec<Value> = obj
        .get("nodes")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let edges: Vec<Value> = obj
        .get("edges")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let hyperedges: Vec<Value> = obj
        .get("hyperedges")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut node_by_id: HashMap<String, Value> = HashMap::new();
    for n in &nodes {
        if let Some(id) = n.get("id").and_then(|v| v.as_str()) {
            if !id.is_empty() {
                node_by_id.insert(id.to_string(), n.clone());
            }
        }
    }

    let rationale_for_sources: HashSet<String> = edges
        .iter()
        .filter(|e| e.get("relation").and_then(|v| v.as_str()) == Some("rationale_for"))
        .filter_map(|e| {
            e.get("source")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        })
        .collect();

    let mut rationale_candidates: Vec<Value> = Vec::new();
    let mut remove_ids: HashSet<String> = HashSet::new();
    let mut keep_nodes: Vec<Value> = Vec::new();

    for n in nodes {
        let nid = match n.get("id").and_then(|v| v.as_str()) {
            Some(id) if !id.is_empty() => id.to_string(),
            _ => continue,
        };
        let ft = n.get("file_type").and_then(|v| v.as_str()).unwrap_or("");
        let label = n.get("label").and_then(|v| v.as_str()).unwrap_or("");
        if invalid_ft.contains(ft) {
            if is_sentence_like_rationale_label(label) {
                rationale_candidates.push(n.clone());
            }
            remove_ids.insert(nid);
            continue;
        }
        if rationale_for_sources.contains(&nid) && is_sentence_like_rationale_label(label) {
            rationale_candidates.push(n.clone());
            remove_ids.insert(nid);
            continue;
        }
        keep_nodes.push(n);
    }

    let mut rationale_attrs: HashMap<String, Vec<String>> = HashMap::new();
    for rn in &rationale_candidates {
        let rn_id = match rn.get("id").and_then(|v| v.as_str()) {
            Some(id) => id,
            None => continue,
        };
        let text = rn
            .get("label")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        for e in &edges {
            if e.get("relation").and_then(|v| v.as_str()) != Some("rationale_for") {
                continue;
            }
            if e.get("source").and_then(|v| v.as_str()) != Some(rn_id) {
                continue;
            }
            let target_id = match e.get("target").and_then(|v| v.as_str()) {
                Some(t) => t.to_string(),
                None => continue,
            };
            if !node_by_id.contains_key(&target_id) || remove_ids.contains(&target_id) {
                continue;
            }
            rationale_attrs
                .entry(target_id)
                .or_default()
                .push(text.clone());
        }
    }

    for n in &mut keep_nodes {
        let nid = n
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if let Some(texts) = rationale_attrs.get(&nid) {
            append_rationale_attr(n, texts);
        }
    }

    let keep_edges: Vec<Value> = edges
        .into_iter()
        .filter(|e| {
            let src = e.get("source").and_then(|v| v.as_str()).unwrap_or("");
            let tgt = e.get("target").and_then(|v| v.as_str()).unwrap_or("");
            !remove_ids.contains(src) && !remove_ids.contains(tgt)
        })
        .collect();

    let surviving_ids: HashSet<String> = keep_nodes
        .iter()
        .filter_map(|n| n.get("id").and_then(|v| v.as_str()).map(|s| s.to_string()))
        .filter(|s| !s.is_empty())
        .collect();

    let keep_hyperedges: Vec<Value> = hyperedges
        .into_iter()
        .filter_map(|he| {
            let he_obj = he.as_object()?;
            let he_nodes = he_obj.get("nodes")?.as_array()?;
            let filtered: Vec<Value> = he_nodes
                .iter()
                .filter(|r| {
                    r.as_str()
                        .map(|s| surviving_ids.contains(s))
                        .unwrap_or(false)
                })
                .cloned()
                .collect();
            if filtered.len() < 2 {
                return None;
            }
            let mut new_he = he_obj.clone();
            new_he.insert("nodes".to_string(), Value::Array(filtered));
            Some(Value::Object(new_he))
        })
        .collect();

    let obj = fragment.as_object_mut().unwrap();
    obj.insert("nodes".to_string(), Value::Array(keep_nodes));
    obj.insert("edges".to_string(), Value::Array(keep_edges));
    obj.insert("hyperedges".to_string(), Value::Array(keep_hyperedges));
    fragment
}

fn is_sentence_like_rationale_label(label: &str) -> bool {
    if label.is_empty() {
        return false;
    }
    let label = label.trim();
    if label.len() < RATIONALE_MIN_CHARS {
        let word_count = label.split_whitespace().count();
        if word_count < RATIONALE_MIN_WORDS {
            return false;
        }
    }
    label.contains('.') || label.contains('!') || label.contains('?') || label.contains(':')
}

fn append_rationale_attr(node: &mut Value, texts: &[String]) {
    let new_text = texts.join("\n\n").trim().to_string();
    if let Some(obj) = node.as_object_mut() {
        let existing = obj
            .get("rationale")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let combined = if existing.is_empty() {
            new_text
        } else {
            format!("{}\n\n{}", existing, new_text)
        };
        obj.insert("rationale".to_string(), json!(combined));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn valid_fragment() -> Value {
        json!({
            "nodes": [{"id": "module_func", "label": "func", "file_type": "code"}],
            "edges": [{"source": "module_func", "target": "other_node"}],
            "hyperedges": [],
        })
    }

    #[test]
    fn test_validate_semantic_fragment_accepts_valid() {
        assert_eq!(
            validate_semantic_fragment(&valid_fragment()),
            Vec::<String>::new()
        );
    }

    #[test]
    fn test_validate_semantic_fragment_rejects_non_object() {
        let errors = validate_semantic_fragment(&json!(["not", "an", "object"]));
        assert!(errors.iter().any(|e| e.to_lowercase().contains("object")));
    }

    #[test]
    fn test_validate_semantic_fragment_rejects_oversize_payload() {
        let limits = SemanticLimits {
            max_bytes: 64,
            ..SemanticLimits::default()
        };
        let mut fragment = valid_fragment();
        fragment["nodes"][0]["label"] = json!("x".repeat(128));
        let errors = validate_semantic_fragment_with_limits(&fragment, &limits);
        assert!(errors.iter().any(|e| e.to_lowercase().contains("payload")));
    }

    #[test]
    fn test_validate_semantic_fragment_rejects_too_many_nodes() {
        let limits = SemanticLimits {
            max_nodes: 1,
            ..SemanticLimits::default()
        };
        let mut fragment = valid_fragment();
        fragment["nodes"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id": "extra", "label": "extra", "file_type": "code"}));
        let errors = validate_semantic_fragment_with_limits(&fragment, &limits);
        assert!(errors.iter().any(|e| e.to_lowercase().contains("nodes")));
    }

    #[test]
    fn test_validate_semantic_fragment_rejects_too_many_edges() {
        let limits = SemanticLimits {
            max_edges: 0,
            ..SemanticLimits::default()
        };
        let errors = validate_semantic_fragment_with_limits(&valid_fragment(), &limits);
        assert!(errors.iter().any(|e| e.to_lowercase().contains("edges")));
    }

    #[test]
    fn test_validate_semantic_fragment_rejects_path_separator_in_id() {
        let mut fragment = valid_fragment();
        fragment["nodes"][0]["id"] = json!("../etc/passwd");
        let errors = validate_semantic_fragment(&fragment);
        assert!(errors.iter().any(|e| e.contains("nodes[0].id")));
    }

    #[test]
    fn test_validate_semantic_fragment_rejects_invalid_file_type() {
        let mut fragment = valid_fragment();
        fragment["nodes"][0]["file_type"] = json!("executable");
        let errors = validate_semantic_fragment(&fragment);
        assert!(errors.iter().any(|e| e.contains("file_type")));
    }

    #[test]
    fn test_validate_semantic_fragment_accepts_rationale_file_type() {
        let mut fragment = valid_fragment();
        fragment["nodes"][0]["file_type"] = json!("rationale");
        let errors = validate_semantic_fragment(&fragment);
        assert!(
            !errors.iter().any(|e| e.contains("file_type")),
            "'rationale' must be accepted; errors: {errors:?}"
        );
    }

    #[test]
    fn test_validate_semantic_fragment_accepts_concept_file_type() {
        let mut fragment = valid_fragment();
        fragment["nodes"][0]["file_type"] = json!("concept");
        let errors = validate_semantic_fragment(&fragment);
        assert!(
            !errors.iter().any(|e| e.contains("file_type")),
            "'concept' must be accepted; errors: {errors:?}"
        );
    }

    #[test]
    fn test_load_validated_semantic_fragment_accepts_valid() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("chunk.json");
        std::fs::write(&path, serde_json::to_string(&valid_fragment()).unwrap()).unwrap();
        let (fragment, errors) = load_validated_semantic_fragment(&path);
        assert_eq!(errors, Vec::<String>::new());
        assert!(fragment.is_some());
    }

    #[test]
    fn test_load_validated_semantic_fragment_rejects_oversize_before_parse() {
        let limits = SemanticLimits {
            max_bytes: 64,
            ..SemanticLimits::default()
        };
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("chunk.json");
        let content: String = std::iter::repeat_n('"', 128).collect::<String>();
        std::fs::write(&path, format!("[{}]", content)).unwrap();
        let (fragment, errors) = load_validated_semantic_fragment_with_limits(&path, &limits);
        assert!(fragment.is_none());
        assert!(errors.iter().any(|e| e.to_lowercase().contains("payload")));
    }

    #[test]
    fn test_load_validated_semantic_fragment_rejects_invalid_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.json");
        std::fs::write(&path, "{not valid json").unwrap();
        let (fragment, errors) = load_validated_semantic_fragment(&path);
        assert!(fragment.is_none());
        assert!(errors
            .iter()
            .any(|e| e.to_lowercase().contains("invalid json")));
    }

    #[test]
    fn test_validate_hyperedge_rejects_bad_id() {
        let mut fragment = valid_fragment();
        fragment["hyperedges"] = json!([
            {"id": "../escape", "label": "x", "nodes": ["module_func", "module_func"]}
        ]);
        let errors = validate_semantic_fragment(&fragment);
        assert!(errors.iter().any(|e| e.contains("hyperedges[0].id")));
    }

    #[test]
    fn test_validate_hyperedge_rejects_bad_node_ref() {
        let mut fragment = valid_fragment();
        fragment["hyperedges"] = json!([
            {"id": "valid_he", "label": "x", "nodes": ["module_func", "../bad_ref"]}
        ]);
        let errors = validate_semantic_fragment(&fragment);
        assert!(errors.iter().any(|e| e.contains("hyperedges[0].nodes[1]")));
    }

    #[test]
    fn test_validate_hyperedge_requires_list() {
        let mut fragment = valid_fragment();
        fragment["hyperedges"] = json!([{"id": "valid_he", "label": "x", "nodes": "not a list"}]);
        let errors = validate_semantic_fragment(&fragment);
        assert!(errors.iter().any(|e| e.contains("hyperedges[0].nodes")));
    }

    #[test]
    fn test_validate_hyperedge_caps_count() {
        let limits = SemanticLimits {
            max_hyperedges: 1,
            ..SemanticLimits::default()
        };
        let mut fragment = valid_fragment();
        fragment["hyperedges"] = json!([
            {"id": "he_0", "label": "x", "nodes": ["module_func", "module_func"]},
            {"id": "he_1", "label": "x", "nodes": ["module_func", "module_func"]},
            {"id": "he_2", "label": "x", "nodes": ["module_func", "module_func"]},
        ]);
        let errors = validate_semantic_fragment_with_limits(&fragment, &limits);
        assert!(errors.iter().any(|e| e.contains("hyperedges has 3")));
    }

    #[test]
    fn test_sanitize_drops_rationale_filetype_node() {
        let mut fragment = json!({
            "nodes": [
                {"id": "real_node", "label": "Real", "file_type": "code"},
                {"id": "garbage", "label": "junk", "file_type": "rationale"},
            ],
            "edges": [],
            "hyperedges": [],
        });
        sanitize_semantic_fragment(&mut fragment);
        let ids: HashSet<&str> = fragment["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|n| n["id"].as_str())
            .collect();
        assert!(ids.contains("real_node"));
        assert!(!ids.contains("garbage"));
    }

    #[test]
    fn test_sanitize_converts_sentence_rationale_node_to_attribute() {
        let mut fragment = json!({
            "nodes": [
                {"id": "real_node", "label": "Real", "file_type": "code"},
                {
                    "id": "why_node",
                    "label": "We chose tree-sitter because the deterministic parser is faster than regex-based extraction.",
                    "file_type": "rationale",
                },
            ],
            "edges": [{"source": "why_node", "target": "real_node", "relation": "rationale_for"}],
            "hyperedges": [],
        });
        sanitize_semantic_fragment(&mut fragment);
        let ids: HashSet<&str> = fragment["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|n| n["id"].as_str())
            .collect();
        assert!(!ids.contains("why_node"));
        let target = fragment["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["id"].as_str() == Some("real_node"))
            .unwrap();
        assert!(target["rationale"]
            .as_str()
            .unwrap_or("")
            .contains("tree-sitter"));
    }

    #[test]
    fn test_sanitize_converts_allowed_filetype_sentence_via_rationale_for_edge() {
        let long_label = "Decision: this node has sentence-like rationale text but uses an \
            allowed file_type, so it should not survive as a standalone graph node.";
        let mut fragment = json!({
            "nodes": [
                {"id": "real_node", "label": "Real", "file_type": "code"},
                {"id": "sentence_node", "label": long_label, "file_type": "document"},
            ],
            "edges": [{"source": "sentence_node", "target": "real_node", "relation": "rationale_for"}],
            "hyperedges": [],
        });
        sanitize_semantic_fragment(&mut fragment);
        let ids: HashSet<&str> = fragment["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|n| n["id"].as_str())
            .collect();
        assert!(!ids.contains("sentence_node"));
        let target = fragment["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["id"].as_str() == Some("real_node"))
            .unwrap();
        assert!(target["rationale"]
            .as_str()
            .unwrap_or("")
            .contains("Decision"));
    }

    #[test]
    fn test_sanitize_keeps_short_concept_named_node_with_punctuation() {
        let mut fragment = json!({
            "nodes": [
                {"id": "a_b", "label": "a.b.c", "file_type": "document"},
                {"id": "anchor", "label": "Anchor", "file_type": "code"},
            ],
            "edges": [{"source": "a_b", "target": "anchor", "relation": "rationale_for"}],
            "hyperedges": [],
        });
        sanitize_semantic_fragment(&mut fragment);
        let ids: HashSet<&str> = fragment["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|n| n["id"].as_str())
            .collect();
        assert!(ids.contains("a_b"));
        assert!(ids.contains("anchor"));
    }

    #[test]
    fn test_sanitize_filters_hyperedges_after_node_removal() {
        let mut fragment = json!({
            "nodes": [
                {"id": "real_node", "label": "Real", "file_type": "code"},
                {"id": "other", "label": "Other", "file_type": "code"},
                {"id": "garbage", "label": "junk", "file_type": "rationale"},
            ],
            "edges": [],
            "hyperedges": [
                {"id": "group_a", "label": "Group A", "nodes": ["garbage", "real_node", "other"], "relation": "participate_in"},
                {"id": "group_b", "label": "Group B (only one survivor)", "nodes": ["garbage", "real_node"], "relation": "participate_in"},
            ],
        });
        sanitize_semantic_fragment(&mut fragment);
        let he_ids: HashSet<&str> = fragment["hyperedges"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|he| he["id"].as_str())
            .collect();
        assert!(he_ids.contains("group_a"));
        assert!(!he_ids.contains("group_b"));
        let group_a = fragment["hyperedges"]
            .as_array()
            .unwrap()
            .iter()
            .find(|he| he["id"].as_str() == Some("group_a"))
            .unwrap();
        let nodes: HashSet<&str> = group_a["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert!(!nodes.contains("garbage"));
        assert_eq!(nodes, ["real_node", "other"].iter().copied().collect());
    }

    #[test]
    fn test_sanitize_drops_hyperedge_with_only_unknown_refs() {
        let mut fragment = json!({
            "nodes": [{"id": "real_node", "label": "Real", "file_type": "code"}],
            "edges": [],
            "hyperedges": [{"id": "phantom", "label": "Phantom", "nodes": ["ghost1", "ghost2"]}],
        });
        sanitize_semantic_fragment(&mut fragment);
        assert_eq!(
            fragment["hyperedges"].as_array().unwrap(),
            &Vec::<Value>::new()
        );
    }

    #[test]
    fn test_sanitize_boundary_sentence_threshold() {
        // 8 words + colon → sentence-like, gets removed and becomes rationale
        let long_label = "Note: alpha beta gamma delta epsilon zeta eta";
        let mut fragment = json!({
            "nodes": [
                {"id": "anchor", "label": "Anchor", "file_type": "code"},
                {"id": "n1", "label": long_label, "file_type": "rationale"},
            ],
            "edges": [{"source": "n1", "target": "anchor", "relation": "rationale_for"}],
            "hyperedges": [],
        });
        sanitize_semantic_fragment(&mut fragment);
        let ids: HashSet<&str> = fragment["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|n| n["id"].as_str())
            .collect();
        assert_eq!(ids, ["anchor"].iter().copied().collect());
        let anchor = fragment["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["id"].as_str() == Some("anchor"))
            .unwrap();
        assert!(anchor["rationale"].as_str().unwrap_or("").contains("alpha"));

        // 7 words, no terminal punctuation → not sentence-like, but still removed (rationale ft) without attribute
        let short_label = "alpha beta gamma delta epsilon zeta eta";
        let mut fragment2 = json!({
            "nodes": [
                {"id": "anchor", "label": "Anchor", "file_type": "code"},
                {"id": "n2", "label": short_label, "file_type": "rationale"},
            ],
            "edges": [],
            "hyperedges": [],
        });
        sanitize_semantic_fragment(&mut fragment2);
        let ids2: HashSet<&str> = fragment2["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|n| n["id"].as_str())
            .collect();
        assert_eq!(ids2, ["anchor"].iter().copied().collect());
        let anchor2 = fragment2["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["id"].as_str() == Some("anchor"))
            .unwrap();
        assert!(anchor2.get("rationale").is_none());
    }

    #[test]
    fn test_sanitize_rationale_only_propagates_through_rationale_for_edges() {
        let mut fragment = json!({
            "nodes": [
                {"id": "rationale_target", "label": "Rationale Target", "file_type": "code"},
                {"id": "unrelated_target", "label": "Unrelated Target", "file_type": "code"},
                {
                    "id": "why_node",
                    "label": "Decision: we chose tree-sitter because the deterministic parser is faster than regex-based extraction.",
                    "file_type": "rationale",
                },
            ],
            "edges": [
                {"source": "why_node", "target": "rationale_target", "relation": "rationale_for"},
                {"source": "why_node", "target": "unrelated_target", "relation": "references"},
            ],
            "hyperedges": [],
        });
        sanitize_semantic_fragment(&mut fragment);
        let id_map: HashMap<&str, &Value> = fragment["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|n| n["id"].as_str().map(|id| (id, n)))
            .collect();
        assert!(!id_map.contains_key("why_node"));
        assert!(id_map["rationale_target"]["rationale"]
            .as_str()
            .unwrap_or("")
            .contains("tree-sitter"));
        assert!(id_map["unrelated_target"].get("rationale").is_none());
    }
}
