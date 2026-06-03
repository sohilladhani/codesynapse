use serde_json::Value;

const VALID_FILE_TYPES: &[&str] = &["code", "document", "paper", "image", "rationale", "concept"];
const VALID_CONFIDENCES: &[&str] = &["EXTRACTED", "INFERRED", "AMBIGUOUS"];
const REQUIRED_NODE_FIELDS: &[&str] = &["id", "label", "file_type", "source_file"];
const REQUIRED_EDGE_FIELDS: &[&str] =
    &["source", "target", "relation", "confidence", "source_file"];

pub fn validate_extraction(data: &Value) -> Vec<String> {
    let obj = match data.as_object() {
        Some(o) => o,
        None => return vec!["Extraction must be a JSON object".to_string()],
    };

    let mut errors: Vec<String> = Vec::new();

    let node_ids: std::collections::HashSet<String> = match obj.get("nodes") {
        None => {
            errors.push("Missing required key 'nodes'".to_string());
            std::collections::HashSet::new()
        }
        Some(v) => match v.as_array() {
            None => {
                errors.push("'nodes' must be a list".to_string());
                std::collections::HashSet::new()
            }
            Some(nodes) => {
                let mut ids = std::collections::HashSet::new();
                for (i, node) in nodes.iter().enumerate() {
                    match node.as_object() {
                        None => {
                            errors.push(format!("Node {i} must be an object"));
                            continue;
                        }
                        Some(n) => {
                            let node_id = n.get("id").and_then(|v| v.as_str()).unwrap_or("?");
                            for field in REQUIRED_NODE_FIELDS {
                                if !n.contains_key(*field) {
                                    errors.push(format!(
                                        "Node {i} (id={node_id:?}) missing required field '{field}'"
                                    ));
                                }
                            }
                            if let Some(ft) = n.get("file_type").and_then(|v| v.as_str()) {
                                if !VALID_FILE_TYPES.contains(&ft) {
                                    errors.push(format!(
                                        "Node {i} (id={node_id:?}) has invalid file_type '{ft}' - must be one of {:?}",
                                        {
                                            let mut v: Vec<&&str> = VALID_FILE_TYPES.iter().collect();
                                            v.sort();
                                            v
                                        }
                                    ));
                                }
                            }
                            if let Some(id) = n.get("id").and_then(|v| v.as_str()) {
                                ids.insert(id.to_string());
                            }
                        }
                    }
                }
                ids
            }
        },
    };

    let edge_list = obj.get("edges").or_else(|| obj.get("links"));

    match edge_list {
        None => errors.push("Missing required key 'edges'".to_string()),
        Some(v) => match v.as_array() {
            None => errors.push("'edges' must be a list".to_string()),
            Some(edges) => {
                for (i, edge) in edges.iter().enumerate() {
                    match edge.as_object() {
                        None => {
                            errors.push(format!("Edge {i} must be an object"));
                            continue;
                        }
                        Some(e) => {
                            for field in REQUIRED_EDGE_FIELDS {
                                if !e.contains_key(*field) {
                                    errors
                                        .push(format!("Edge {i} missing required field '{field}'"));
                                }
                            }
                            if let Some(conf) = e.get("confidence").and_then(|v| v.as_str()) {
                                if !VALID_CONFIDENCES.contains(&conf) {
                                    errors.push(format!(
                                        "Edge {i} has invalid confidence '{conf}' - must be one of {:?}",
                                        {
                                            let mut v: Vec<&&str> = VALID_CONFIDENCES.iter().collect();
                                            v.sort();
                                            v
                                        }
                                    ));
                                }
                            }
                            if !node_ids.is_empty() {
                                if let Some(src) = e.get("source").and_then(|v| v.as_str()) {
                                    if !node_ids.contains(src) {
                                        errors.push(format!(
                                            "Edge {i} source '{src}' does not match any node id"
                                        ));
                                    }
                                }
                                if let Some(tgt) = e.get("target").and_then(|v| v.as_str()) {
                                    if !node_ids.contains(tgt) {
                                        errors.push(format!(
                                            "Edge {i} target '{tgt}' does not match any node id"
                                        ));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        },
    }

    errors
}

pub fn assert_valid(data: &Value) -> Result<(), crate::error::CodeSynapseError> {
    let errors = validate_extraction(data);
    if errors.is_empty() {
        Ok(())
    } else {
        let msg = format!(
            "Extraction JSON has {} error(s):\n{}",
            errors.len(),
            errors
                .iter()
                .map(|e| format!("  • {e}"))
                .collect::<Vec<_>>()
                .join("\n")
        );
        Err(crate::error::CodeSynapseError::Validation(msg))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn valid() -> Value {
        json!({
            "nodes": [
                {"id": "n1", "label": "Foo", "file_type": "code", "source_file": "foo.py"},
                {"id": "n2", "label": "Bar", "file_type": "document", "source_file": "bar.md"},
            ],
            "edges": [
                {"source": "n1", "target": "n2", "relation": "references",
                 "confidence": "EXTRACTED", "source_file": "foo.py", "weight": 1.0},
            ],
        })
    }

    #[test]
    fn test_valid_passes() {
        assert_eq!(validate_extraction(&valid()), Vec::<String>::new());
    }

    #[test]
    fn test_missing_nodes_key() {
        let data = json!({"edges": []});
        let errors = validate_extraction(&data);
        assert!(errors.iter().any(|e| e.contains("nodes")));
    }

    #[test]
    fn test_missing_edges_key() {
        let data = json!({"nodes": []});
        let errors = validate_extraction(&data);
        assert!(errors.iter().any(|e| e.contains("edges")));
    }

    #[test]
    fn test_not_a_dict() {
        let errors = validate_extraction(&json!([]));
        assert_eq!(errors.len(), 1);
    }

    #[test]
    fn test_invalid_file_type() {
        let data = json!({
            "nodes": [{"id": "n1", "label": "X", "file_type": "video", "source_file": "x.mp4"}],
            "edges": [],
        });
        let errors = validate_extraction(&data);
        assert!(errors.iter().any(|e| e.contains("file_type")));
    }

    #[test]
    fn test_invalid_confidence() {
        let data = json!({
            "nodes": [
                {"id": "n1", "label": "A", "file_type": "code", "source_file": "a.py"},
                {"id": "n2", "label": "B", "file_type": "code", "source_file": "b.py"},
            ],
            "edges": [
                {"source": "n1", "target": "n2", "relation": "calls",
                 "confidence": "CERTAIN", "source_file": "a.py"},
            ],
        });
        let errors = validate_extraction(&data);
        assert!(errors.iter().any(|e| e.contains("confidence")));
    }

    #[test]
    fn test_dangling_edge_source() {
        let data = json!({
            "nodes": [{"id": "n1", "label": "A", "file_type": "code", "source_file": "a.py"}],
            "edges": [
                {"source": "missing_id", "target": "n1", "relation": "calls",
                 "confidence": "EXTRACTED", "source_file": "a.py"},
            ],
        });
        let errors = validate_extraction(&data);
        assert!(errors
            .iter()
            .any(|e| e.contains("source") && e.contains("missing_id")));
    }

    #[test]
    fn test_dangling_edge_target() {
        let data = json!({
            "nodes": [{"id": "n1", "label": "A", "file_type": "code", "source_file": "a.py"}],
            "edges": [
                {"source": "n1", "target": "ghost", "relation": "calls",
                 "confidence": "EXTRACTED", "source_file": "a.py"},
            ],
        });
        let errors = validate_extraction(&data);
        assert!(errors
            .iter()
            .any(|e| e.contains("target") && e.contains("ghost")));
    }

    #[test]
    fn test_missing_node_field() {
        let data = json!({
            "nodes": [{"id": "n1", "label": "A", "source_file": "a.py"}],
            "edges": [],
        });
        let errors = validate_extraction(&data);
        assert!(errors.iter().any(|e| e.contains("file_type")));
    }

    #[test]
    fn test_assert_valid_raises_on_errors() {
        let data = json!({"nodes": "bad", "edges": []});
        assert!(assert_valid(&data).is_err());
        let err = assert_valid(&data).unwrap_err().to_string();
        assert!(err.contains("error"));
    }

    #[test]
    fn test_assert_valid_passes_silently() {
        assert!(assert_valid(&valid()).is_ok());
    }

    #[test]
    fn test_links_fallback() {
        let data = json!({
            "nodes": [
                {"id": "n1", "label": "A", "file_type": "code", "source_file": "a.py"},
            ],
            "links": [
                {"source": "n1", "target": "n1", "relation": "self",
                 "confidence": "INFERRED", "source_file": "a.py"},
            ],
        });
        assert_eq!(validate_extraction(&data), Vec::<String>::new());
    }
}
