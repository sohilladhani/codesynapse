use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HyperEdgeEntry {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub nodes: Vec<String>,
    #[serde(default)]
    pub relation: Option<String>,
    #[serde(default)]
    pub confidence: Option<String>,
    #[serde(default)]
    pub confidence_score: Option<f64>,
    #[serde(default)]
    pub source_file: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HyperGraphData {
    pub nodes: Vec<Value>,
    pub edges: Vec<Value>,
    pub hyperedges: Vec<HyperEdgeEntry>,
}

pub fn build_from_json(data: &Value) -> HyperGraphData {
    let nodes = data["nodes"].as_array().cloned().unwrap_or_default();
    let edges = data["edges"].as_array().cloned().unwrap_or_default();
    let hyperedges = data
        .get("hyperedges")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| serde_json::from_value(v.clone()).ok())
                .collect()
        })
        .unwrap_or_default();
    HyperGraphData {
        nodes,
        edges,
        hyperedges,
    }
}

pub fn attach_hyperedges(existing: &mut Vec<HyperEdgeEntry>, new: &[Value]) {
    let existing_ids: HashSet<String> = existing.iter().map(|h| h.id.clone()).collect();
    let mut seen = existing_ids;
    for v in new {
        let id = match v.get("id").and_then(|i| i.as_str()) {
            Some(id) => id.to_string(),
            None => continue,
        };
        if seen.contains(&id) {
            continue;
        }
        if let Ok(entry) = serde_json::from_value::<HyperEdgeEntry>(v.clone()) {
            seen.insert(id);
            existing.push(entry);
        }
    }
}

pub fn to_json(data: &HyperGraphData) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(data)
}

pub fn report_hyperedges_section(hyperedges: &[HyperEdgeEntry]) -> String {
    if hyperedges.is_empty() {
        return String::new();
    }
    let mut lines = vec!["## Hyperedges (group relationships)".to_string()];
    for h in hyperedges {
        let confidence_part = match (&h.confidence, h.confidence_score) {
            (Some(c), Some(s)) => format!(" {} {}", c, s),
            (Some(c), None) => format!(" {}", c),
            _ => String::new(),
        };
        let nodes_part = h.nodes.join(", ");
        lines.push(format!(
            "- **{}**{} — {}",
            h.label, confidence_part, nodes_part
        ));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn sample_extraction() -> Value {
        json!({
            "nodes": [
                {"id": "BasicAuth", "label": "BasicAuth", "file_type": "code", "source_file": "auth.py"},
                {"id": "DigestAuth", "label": "DigestAuth", "file_type": "code", "source_file": "auth.py"},
                {"id": "Request", "label": "Request", "file_type": "code", "source_file": "http.py"},
            ],
            "edges": [
                {"source": "BasicAuth", "target": "Request", "relation": "uses", "confidence": "EXTRACTED", "confidence_score": 1.0, "source_file": "auth.py"}
            ],
            "hyperedges": [
                {
                    "id": "auth_flow",
                    "label": "Auth Flow",
                    "nodes": ["BasicAuth", "DigestAuth", "Request"],
                    "relation": "participate_in",
                    "confidence": "INFERRED",
                    "confidence_score": 0.75,
                    "source_file": "auth.py"
                }
            ]
        })
    }

    #[test]
    fn test_build_from_json_stores_hyperedges() {
        let g = build_from_json(&sample_extraction());
        assert_eq!(g.hyperedges.len(), 1);
        assert_eq!(g.hyperedges[0].id, "auth_flow");
    }

    #[test]
    fn test_build_from_json_no_hyperedges() {
        let mut data = sample_extraction();
        data["hyperedges"] = json!([]);
        let g = build_from_json(&data);
        assert_eq!(g.hyperedges.len(), 0);
    }

    #[test]
    fn test_build_from_json_missing_hyperedges_key() {
        let data = json!({
            "nodes": [],
            "edges": []
        });
        let g = build_from_json(&data);
        assert_eq!(g.hyperedges.len(), 0);
    }

    #[test]
    fn test_attach_hyperedges_adds_new() {
        let mut existing: Vec<HyperEdgeEntry> = vec![];
        let new = vec![json!({"id": "auth_flow", "label": "Auth Flow", "nodes": ["A", "B", "C"]})];
        attach_hyperedges(&mut existing, &new);
        assert_eq!(existing.len(), 1);
    }

    #[test]
    fn test_attach_hyperedges_deduplicates() {
        let mut existing: Vec<HyperEdgeEntry> = vec![];
        let h = json!({"id": "auth_flow", "label": "Auth Flow", "nodes": ["A", "B", "C"]});
        attach_hyperedges(&mut existing, std::slice::from_ref(&h));
        attach_hyperedges(&mut existing, &[h]);
        assert_eq!(existing.len(), 1);
    }

    #[test]
    fn test_attach_hyperedges_multiple_different_ids() {
        let mut existing: Vec<HyperEdgeEntry> = vec![];
        let new = vec![
            json!({"id": "flow_a", "label": "Flow A", "nodes": ["A", "B"]}),
            json!({"id": "flow_b", "label": "Flow B", "nodes": ["C", "D"]}),
        ];
        attach_hyperedges(&mut existing, &new);
        assert_eq!(existing.len(), 2);
    }

    #[test]
    fn test_attach_hyperedges_skips_entry_without_id() {
        let mut existing: Vec<HyperEdgeEntry> = vec![];
        let new = vec![json!({"label": "No ID", "nodes": ["A", "B"]})];
        attach_hyperedges(&mut existing, &new);
        assert_eq!(existing.len(), 0);
    }

    #[test]
    fn test_to_json_includes_hyperedges() {
        let g = build_from_json(&sample_extraction());
        let json_str = to_json(&g).unwrap();
        let parsed: Value = serde_json::from_str(&json_str).unwrap();
        assert!(parsed.get("hyperedges").is_some());
        let hyperedges = parsed["hyperedges"].as_array().unwrap();
        assert_eq!(hyperedges.len(), 1);
        assert_eq!(hyperedges[0]["id"], "auth_flow");
    }

    #[test]
    fn test_to_json_hyperedges_empty_when_none() {
        let mut data = sample_extraction();
        data["hyperedges"] = json!([]);
        let g = build_from_json(&data);
        let json_str = to_json(&g).unwrap();
        let parsed: Value = serde_json::from_str(&json_str).unwrap();
        assert!(parsed.get("hyperedges").is_some());
        assert_eq!(parsed["hyperedges"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn test_hyperedges_roundtrip_via_json_file() {
        let g = build_from_json(&sample_extraction());
        let json_str = to_json(&g).unwrap();

        let mut tmp = NamedTempFile::new().unwrap();
        tmp.write_all(json_str.as_bytes()).unwrap();
        let path = tmp.path();

        let loaded: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let g2 = build_from_json(&loaded);
        assert!(!g2.hyperedges.is_empty());
        assert_eq!(g2.hyperedges[0].id, "auth_flow");
    }

    #[test]
    fn test_report_includes_hyperedges_section() {
        let g = build_from_json(&sample_extraction());
        let report = report_hyperedges_section(&g.hyperedges);
        assert!(report.contains("## Hyperedges (group relationships)"));
        assert!(report.contains("Auth Flow"));
        assert!(report.contains("INFERRED"));
        assert!(report.contains("0.75"));
    }

    #[test]
    fn test_report_includes_hyperedge_node_list() {
        let g = build_from_json(&sample_extraction());
        let report = report_hyperedges_section(&g.hyperedges);
        assert!(report.contains("BasicAuth"));
        assert!(report.contains("DigestAuth"));
    }

    #[test]
    fn test_report_skips_hyperedges_section_when_empty() {
        let report = report_hyperedges_section(&[]);
        assert!(!report.contains("## Hyperedges"));
    }

    #[test]
    fn test_report_skips_hyperedges_section_when_key_missing() {
        let data = json!({"nodes": [], "edges": []});
        let g = build_from_json(&data);
        let report = report_hyperedges_section(&g.hyperedges);
        assert!(!report.contains("## Hyperedges"));
    }
}
