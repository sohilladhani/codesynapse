use crate::types::{Edge, GraphData, Node};
use std::collections::{HashMap, HashSet, VecDeque};

pub const DEFAULT_AFFECTED_RELATIONS: &[&str] = &[
    "calls",
    "references",
    "imports",
    "imports_from",
    "re_exports",
    "inherits",
    "extends",
    "implements",
    "uses",
    "mixes_in",
    "embeds",
];

#[derive(Debug, Clone, PartialEq)]
pub struct AffectedHit {
    pub node_id: String,
    pub depth: usize,
    pub via_relation: String,
}

pub fn resolve_seed(graph: &GraphData, query: &str) -> Option<String> {
    let node_ids: HashSet<&str> = graph.nodes.iter().map(|n| n.id.as_str()).collect();
    if node_ids.contains(query) {
        return Some(query.to_string());
    }
    let query_lower = query.to_lowercase();
    let exact_label: Vec<&str> = graph
        .nodes
        .iter()
        .filter(|n| n.label.to_lowercase() == query_lower)
        .map(|n| n.id.as_str())
        .collect();
    if exact_label.len() == 1 {
        return Some(exact_label[0].to_string());
    }
    let exact_source: Vec<&str> = graph
        .nodes
        .iter()
        .filter(|n| n.source_file.to_lowercase() == query_lower)
        .map(|n| n.id.as_str())
        .collect();
    if exact_source.len() == 1 {
        return Some(exact_source[0].to_string());
    }
    let contains: Vec<&str> = graph
        .nodes
        .iter()
        .filter(|n| n.label.to_lowercase().contains(&query_lower))
        .map(|n| n.id.as_str())
        .collect();
    if contains.len() == 1 {
        return Some(contains[0].to_string());
    }
    None
}

pub fn affected_nodes(
    graph: &GraphData,
    seed: &str,
    relations: &[&str],
    depth: usize,
) -> Vec<AffectedHit> {
    let relation_set: HashSet<&str> = relations.iter().copied().collect();

    let mut reverse_adj: HashMap<&str, Vec<(&str, &str)>> = HashMap::new();
    for edge in &graph.edges {
        reverse_adj
            .entry(edge.target.as_str())
            .or_default()
            .push((edge.source.as_str(), edge.relation.as_str()));
    }

    let mut seen: HashSet<&str> = HashSet::new();
    seen.insert(seed);
    let mut queue: VecDeque<(&str, usize)> = VecDeque::new();
    queue.push_back((seed, 0));
    let mut hits: Vec<AffectedHit> = Vec::new();

    while let Some((current, current_depth)) = queue.pop_front() {
        if current_depth >= depth {
            continue;
        }
        if let Some(incoming) = reverse_adj.get(current) {
            for &(source, relation) in incoming {
                if !relation_set.contains(relation) {
                    continue;
                }
                if seen.contains(source) {
                    continue;
                }
                seen.insert(source);
                hits.push(AffectedHit {
                    node_id: source.to_string(),
                    depth: current_depth + 1,
                    via_relation: relation.to_string(),
                });
                queue.push_back((source, current_depth + 1));
            }
        }
    }
    hits
}

pub fn format_affected(graph: &GraphData, query: &str, relations: &[&str], depth: usize) -> String {
    let node_map: HashMap<&str, &Node> = graph.nodes.iter().map(|n| (n.id.as_str(), n)).collect();

    let seed = match resolve_seed(graph, query) {
        Some(s) => s,
        None => return format!("No unique node match for {}", query),
    };

    let hits = affected_nodes(graph, &seed, relations, depth);
    let seed_label = node_map
        .get(seed.as_str())
        .map(|n| n.label.as_str())
        .unwrap_or(&seed);

    let mut lines = vec![
        format!("Affected nodes for {}", seed_label),
        format!("Relations: {}", relations.join(", ")),
        format!("Depth: {}", depth),
    ];

    if hits.is_empty() {
        lines.push("No affected nodes found.".to_string());
        return lines.join("\n");
    }

    for hit in &hits {
        let node = node_map.get(hit.node_id.as_str());
        let label = node.map(|n| n.label.as_str()).unwrap_or(&hit.node_id);
        let location = node
            .map(|n| {
                if let Some(ref loc) = n.source_location {
                    format!("{}:{}", n.source_file, loc)
                } else {
                    n.source_file.clone()
                }
            })
            .unwrap_or_else(|| "-".to_string());
        lines.push(format!("- {} [{}] {}", label, hit.via_relation, location));
    }
    lines.join("\n")
}

pub fn load_graph_json(path: &std::path::Path) -> crate::error::Result<GraphData> {
    let text = std::fs::read_to_string(path)?;
    let v: serde_json::Value =
        serde_json::from_str(&text).map_err(crate::error::CodeSynapseError::Serialization)?;

    let nodes: Vec<Node> = v
        .get("nodes")
        .and_then(|n| n.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| serde_json::from_value(item.clone()).ok())
                .collect()
        })
        .unwrap_or_default();

    let edge_arr = v
        .get("edges")
        .or_else(|| v.get("links"))
        .and_then(|e| e.as_array());

    let edges: Vec<Edge> = edge_arr
        .map(|arr| {
            arr.iter()
                .filter_map(|item| {
                    let obj = item.as_object()?;
                    let source = obj.get("source")?.as_str()?.to_string();
                    let target = obj.get("target")?.as_str()?.to_string();
                    let relation = obj
                        .get("relation")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let confidence = obj
                        .get("confidence")
                        .and_then(|v| v.as_str())
                        .unwrap_or("EXTRACTED")
                        .to_string();
                    let source_file = obj
                        .get("source_file")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    let weight = obj.get("weight").and_then(|v| v.as_f64()).unwrap_or(1.0);
                    Some(Edge {
                        source,
                        target,
                        relation,
                        confidence,
                        source_file,
                        weight,
                        context: None,
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(GraphData {
        nodes,
        edges,
        hyperedges: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Edge, Node};
    use std::collections::HashMap;

    fn make_node(id: &str, label: &str, source_file: &str) -> Node {
        Node {
            id: id.to_string(),
            label: label.to_string(),
            file_type: "code".to_string(),
            source_file: source_file.to_string(),
            source_location: None,
            community: None,
            rationale: None,
            docstring: None,
            metadata: HashMap::new(),
        }
    }

    fn make_edge(source: &str, target: &str, relation: &str) -> Edge {
        Edge {
            source: source.to_string(),
            target: target.to_string(),
            relation: relation.to_string(),
            confidence: "EXTRACTED".to_string(),
            source_file: None,
            weight: 1.0,
            context: None,
        }
    }

    fn test_graph() -> GraphData {
        GraphData {
            nodes: vec![
                make_node("target", "Foo", "pkg/foo.py"),
                make_node("caller", "X()", "app.py"),
                make_node("barrel", "__init__.py", "pkg/__init__.py"),
                make_node("consumer", "app.py", "app.py"),
            ],
            edges: vec![
                make_edge("caller", "target", "calls"),
                make_edge("barrel", "target", "re_exports"),
                make_edge("consumer", "target", "imports"),
            ],
            hyperedges: None,
        }
    }

    #[test]
    fn test_resolve_seed_by_id() {
        let g = test_graph();
        assert_eq!(resolve_seed(&g, "target"), Some("target".to_string()));
    }

    #[test]
    fn test_resolve_seed_by_label() {
        let g = test_graph();
        assert_eq!(resolve_seed(&g, "Foo"), Some("target".to_string()));
    }

    #[test]
    fn test_resolve_seed_case_insensitive() {
        let g = test_graph();
        assert_eq!(resolve_seed(&g, "foo"), Some("target".to_string()));
    }

    #[test]
    fn test_resolve_seed_not_found() {
        let g = test_graph();
        assert_eq!(resolve_seed(&g, "nonexistent"), None);
    }

    #[test]
    fn test_resolve_seed_ambiguous_returns_none() {
        let g = GraphData {
            nodes: vec![
                make_node("n1", "Foo", "a.py"),
                make_node("n2", "Foo", "b.py"),
            ],
            edges: vec![],
            hyperedges: None,
        };
        assert_eq!(resolve_seed(&g, "Foo"), None);
    }

    #[test]
    fn test_affected_nodes_all_relations() {
        let g = test_graph();
        let hits = affected_nodes(&g, "target", DEFAULT_AFFECTED_RELATIONS, 2);
        let ids: HashSet<&str> = hits.iter().map(|h| h.node_id.as_str()).collect();
        assert!(ids.contains("caller"));
        assert!(ids.contains("barrel"));
        assert!(ids.contains("consumer"));
    }

    #[test]
    fn test_affected_nodes_relation_filter() {
        let g = test_graph();
        let hits = affected_nodes(&g, "target", &["calls"], 2);
        let ids: HashSet<&str> = hits.iter().map(|h| h.node_id.as_str()).collect();
        assert!(ids.contains("caller"));
        assert!(!ids.contains("barrel"));
        assert!(!ids.contains("consumer"));
    }

    #[test]
    fn test_affected_nodes_depth_zero_returns_empty() {
        let g = test_graph();
        let hits = affected_nodes(&g, "target", DEFAULT_AFFECTED_RELATIONS, 0);
        assert!(hits.is_empty());
    }

    #[test]
    fn test_affected_nodes_depth_propagation() {
        let g = GraphData {
            nodes: vec![
                make_node("a", "A", "a.py"),
                make_node("b", "B", "b.py"),
                make_node("c", "C", "c.py"),
            ],
            edges: vec![make_edge("b", "a", "calls"), make_edge("c", "b", "calls")],
            hyperedges: None,
        };
        let hits_d1 = affected_nodes(&g, "a", &["calls"], 1);
        let hits_d2 = affected_nodes(&g, "a", &["calls"], 2);
        assert_eq!(hits_d1.len(), 1);
        assert_eq!(hits_d2.len(), 2);
    }

    #[test]
    fn test_format_affected_contains_expected_output() {
        let g = test_graph();
        let out = format_affected(&g, "Foo", DEFAULT_AFFECTED_RELATIONS, 2);
        assert!(
            out.contains("Affected nodes for Foo"),
            "missing header: {out}"
        );
        assert!(out.contains("X()"), "missing X(): {out}");
        assert!(out.contains("calls"), "missing calls: {out}");
        assert!(out.contains("__init__.py"), "missing __init__: {out}");
        assert!(out.contains("re_exports"), "missing re_exports: {out}");
        assert!(out.contains("imports"), "missing imports: {out}");
    }

    #[test]
    fn test_format_affected_relation_filter() {
        let g = test_graph();
        let out = format_affected(&g, "Foo", &["calls"], 2);
        assert!(
            out.contains("Relations: calls"),
            "missing relations header: {out}"
        );
        assert!(out.contains("X()"), "missing X(): {out}");
        assert!(!out.contains("__init__.py"), "unexpected __init__: {out}");
    }

    #[test]
    fn test_format_affected_not_found() {
        let g = test_graph();
        let out = format_affected(&g, "nonexistent", DEFAULT_AFFECTED_RELATIONS, 2);
        assert!(out.contains("No unique node match"));
    }

    #[test]
    fn test_format_affected_no_hits() {
        let g = GraphData {
            nodes: vec![make_node("isolated", "Isolated", "x.py")],
            edges: vec![],
            hyperedges: None,
        };
        let out = format_affected(&g, "isolated", DEFAULT_AFFECTED_RELATIONS, 2);
        assert!(out.contains("No affected nodes found."));
    }
}
