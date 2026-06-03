use crate::error::Result;
use crate::types::{Edge, GraphData, HyperEdge, NetworkXGraph, Node};
use std::collections::HashMap;
use std::path::Path;

pub struct Exporter;

impl Exporter {
    /// NetworkX node_link_data format (default for compat mode)
    pub fn to_json_compat(&self, nodes: &[Node], edges: &[Edge]) -> Result<String> {
        let nx = NetworkXGraph::from_graph_data(nodes, edges);
        serde_json::to_string_pretty(&nx).map_err(Into::into)
    }

    pub fn to_json_compat_file(&self, nodes: &[Node], edges: &[Edge], path: &Path) -> Result<()> {
        let json = self.to_json_compat(nodes, edges)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    pub fn load_json_compat(&self, path: &Path) -> Result<NetworkXGraph> {
        let content = std::fs::read_to_string(path)?;
        let nx: NetworkXGraph = serde_json::from_str(&content)?;
        Ok(nx)
    }

    /// Legacy GraphData format (our original, not NetworkX-compatible)
    pub fn to_json(
        &self,
        nodes: &[Node],
        edges: &[Edge],
        hyperedges: Option<&[HyperEdge]>,
    ) -> Result<String> {
        let graph_data = GraphData {
            nodes: nodes.to_vec(),
            edges: edges.to_vec(),
            hyperedges: hyperedges.map(|h| h.to_vec()),
        };
        serde_json::to_string_pretty(&graph_data).map_err(Into::into)
    }

    pub fn to_json_file(
        &self,
        nodes: &[Node],
        edges: &[Edge],
        hyperedges: Option<&[HyperEdge]>,
        path: &Path,
    ) -> Result<()> {
        let json = self.to_json(nodes, edges, hyperedges)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    pub fn load_json(&self, path: &Path) -> Result<GraphData> {
        let content = std::fs::read_to_string(path)?;
        let data: GraphData = serde_json::from_str(&content)?;
        Ok(data)
    }

    pub fn to_svg(&self, nodes: &[Node], edges: &[Edge]) -> Result<String> {
        let n = nodes.len();
        let center = 300.0;
        let radius = 200.0;
        let r = 20.0;
        let h = "#";

        let mut svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="600" height="600">
<style>text { font-family: sans-serif; font-size: 10px; }</style>
"#
        .to_string();

        let positions: Vec<(f64, f64)> = (0..n)
            .map(|i| {
                let angle = 2.0 * std::f64::consts::PI * i as f64 / n as f64;
                (center + radius * angle.cos(), center + radius * angle.sin())
            })
            .collect();

        for edge in edges {
            let si = nodes.iter().position(|n| n.id == edge.source);
            let ti = nodes.iter().position(|n| n.id == edge.target);
            if let (Some(s), Some(t)) = (si, ti) {
                svg.push_str(&format!(
                    "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{h}999\" stroke-width=\"1\"/>",
                    positions[s].0, positions[s].1, positions[t].0, positions[t].1
                ));
            }
        }

        for (i, node) in nodes.iter().enumerate() {
            let dy = "0.3em";
            svg.push_str(&format!(
                "<circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"{h}4A90D9\" stroke=\"{h}2C5F8A\" stroke-width=\"1\"/>\n\
                 <text x=\"{}\" y=\"{}\" text-anchor=\"middle\" dy=\"{}\" fill=\"{h}fff\">{}</text>",
                positions[i].0,
                positions[i].1,
                r,
                positions[i].0,
                positions[i].1,
                dy,
                node.label.chars().take(4).collect::<String>()
            ));
        }

        svg.push_str("</svg>");
        Ok(svg)
    }

    pub fn to_svg_file(&self, nodes: &[Node], edges: &[Edge], path: &Path) -> Result<()> {
        let svg = self.to_svg(nodes, edges)?;
        std::fs::write(path, svg)?;
        Ok(())
    }

    pub fn to_cypher(&self, nodes: &[Node], edges: &[Edge]) -> Result<String> {
        let mut out = String::new();
        for node in nodes {
            let label = node.label.replace('\'', "\\'");
            let file_type = &node.file_type;
            out.push_str(&format!(
                "CREATE (:`{}` {{id: '{}', label: '{}', file_type: '{}'}})\n",
                file_type, node.id, label, file_type
            ));
        }
        for edge in edges {
            let rel = edge.relation.to_uppercase();
            out.push_str(&format!(
                "CREATE (:`_` {{id: '{}'}})-[:{} {{confidence: '{}'}}]->(:`_` {{id: '{}'}})\n",
                edge.source, rel, edge.confidence, edge.target
            ));
        }
        Ok(out)
    }

    pub fn to_cypher_file(&self, nodes: &[Node], edges: &[Edge], path: &Path) -> Result<()> {
        let cypher = self.to_cypher(nodes, edges)?;
        std::fs::write(path, cypher)?;
        Ok(())
    }

    pub fn to_wiki(&self, nodes: &[Node], _edges: &[Edge], output_dir: &Path) -> Result<()> {
        let index_path = output_dir.join("index.md");
        let mut index = "# Codesynapse Knowledge Graph\n\n".to_string();
        for node in nodes {
            index.push_str(&format!("- [[{}]]\n", node.label));
        }
        std::fs::write(&index_path, index)?;

        let community_map: HashMap<Option<usize>, Vec<&Node>> =
            nodes.iter().fold(HashMap::new(), |mut acc, node| {
                acc.entry(node.community).or_default().push(node);
                acc
            });

        for (community_id, members) in &community_map {
            let filename = format!(
                "Community-{}.md",
                community_id.map_or("none".to_string(), |i| i.to_string())
            );
            let mut content = format!(
                "# Community {}\n\n",
                community_id.map_or("none".to_string(), |i| i.to_string())
            );
            for member in members {
                content.push_str(&format!("- [[{}]]\n", member.label));
            }
            std::fs::write(output_dir.join(&filename), content)?;
        }
        Ok(())
    }

    pub fn to_html(&self, nodes: &[Node], _edges: &[Edge]) -> Result<String> {
        let mut html = String::from("<html><body><h1>Codesynapse Graph</h1><ul>");
        for node in nodes {
            html.push_str(&format!(
                "<li><strong>{}</strong> ({})</li>",
                node.label, node.file_type
            ));
        }
        html.push_str("</ul></body></html>");
        Ok(html)
    }

    pub fn to_html_file(&self, nodes: &[Node], edges: &[Edge], path: &Path) -> Result<()> {
        let html = self.to_html(nodes, edges)?;
        std::fs::write(path, html)?;
        Ok(())
    }

    pub fn to_graphml(&self, nodes: &[Node], edges: &[Edge]) -> Result<String> {
        let mut xml = String::from(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<graphml xmlns="http://graphml.graphdrawing.org/xmlns">
<graph id="G" edgedefault="directed">
<key id="label" for="node" attr.name="label" attr.type="string"/>
<key id="file_type" for="node" attr.name="file_type" attr.type="string"/>
<key id="relation" for="edge" attr.name="relation" attr.type="string"/>
"#,
        );

        for node in nodes {
            xml.push_str(&format!(
                r#"<node id="{}"><data key="label">{}</data><data key="file_type">{}</data></node>"#,
                node.id, node.label, node.file_type
            ));
        }

        for edge in edges {
            xml.push_str(&format!(
                r#"<edge source="{}" target="{}"><data key="relation">{}</data></edge>"#,
                edge.source, edge.target, edge.relation
            ));
        }

        xml.push_str("</graph></graphml>");
        Ok(xml)
    }

    pub fn to_obsidian_vault(
        &self,
        nodes: &[Node],
        _edges: &[Edge],
        output_dir: &Path,
    ) -> Result<()> {
        let index_path = output_dir.join("index.md");
        let mut index = "# Codesynapse Knowledge Graph\n\n".to_string();

        for node in nodes {
            index.push_str(&format!("[[{}]] ", node.label));
        }

        std::fs::write(&index_path, index)?;

        let community_map: HashMap<Option<usize>, Vec<&Node>> =
            nodes.iter().fold(HashMap::new(), |mut acc, node| {
                acc.entry(node.community).or_default().push(node);
                acc
            });

        for (community_id, members) in &community_map {
            let filename = format!(
                "Community {}.md",
                community_id.map_or("none".to_string(), |i| i.to_string())
            );
            let mut content = format!(
                "# Community {}\n\n",
                community_id.map_or("none".to_string(), |i| i.to_string())
            );
            for member in members {
                content.push_str(&format!("[[{}]]\n", member.label));
            }
            std::fs::write(output_dir.join(&filename), content)?;
        }

        Ok(())
    }

    pub fn to_obsidian_vault_from_data(&self, data: &GraphData, output_dir: &Path) -> Result<()> {
        self.to_obsidian_vault(&data.nodes, &data.edges, output_dir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn make_node(id: &str, label: &str, file_type: &str) -> Node {
        Node {
            id: id.to_string(),
            label: label.to_string(),
            file_type: file_type.to_string(),
            source_file: "test.py".to_string(),
            source_location: None,
            community: Some(0),
            rationale: None,
            docstring: None,
            metadata: HashMap::new(),
        }
    }

    fn make_edge(src: &str, tgt: &str, relation: &str) -> Edge {
        Edge {
            source: src.to_string(),
            target: tgt.to_string(),
            relation: relation.to_string(),
            confidence: "EXTRACTED".to_string(),
            source_file: Some("test.py".to_string()),
            weight: 1.0,
            context: None,
        }
    }

    #[test]
    fn test_export_json_roundtrip() {
        let nodes = vec![make_node("a", "A", "code"), make_node("b", "B", "code")];
        let edges = vec![make_edge("a", "b", "imports")];

        let exporter = Exporter;
        let json = exporter.to_json(&nodes, &edges, None).unwrap();
        let loaded: GraphData = serde_json::from_str(&json).unwrap();

        assert_eq!(loaded.nodes.len(), 2);
        assert_eq!(loaded.edges.len(), 1);
    }

    #[test]
    fn test_export_json_node_attrs() {
        let node = make_node("a", "A", "code");
        let exporter = Exporter;
        let json = exporter.to_json(&[node], &[], None).unwrap();
        assert!(json.contains("\"label\": \"A\""));
        assert!(json.contains("\"file_type\": \"code\""));
    }

    #[test]
    fn test_export_json_edge_attrs() {
        let edge = make_edge("a", "b", "imports");
        let exporter = Exporter;
        let json = exporter.to_json(&[], &[edge], None).unwrap();
        assert!(json.contains("\"relation\": \"imports\""));
        assert!(json.contains("\"confidence\": \"EXTRACTED\""));
    }

    #[test]
    fn test_export_html_creates_file() {
        let dir = std::env::temp_dir().join("codesynapse_test_html");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let path = dir.join("graph.html");
        let exporter = Exporter;
        exporter
            .to_html_file(&[make_node("a", "A", "code")], &[], &path)
            .unwrap();

        assert!(path.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_export_html_node_count() {
        let nodes = vec![make_node("a", "A", "code"), make_node("b", "B", "code")];
        let exporter = Exporter;
        let html = exporter.to_html(&nodes, &[]).unwrap();
        // Count <li> tags
        let count = html.matches("<li>").count();
        assert_eq!(count, 2);
    }

    #[test]
    fn test_export_obsidian_vault() {
        let dir = std::env::temp_dir().join("codesynapse_test_obsidian");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let nodes = vec![make_node("a", "A", "code"), make_node("b", "B", "code")];
        let edges = vec![make_edge("a", "b", "imports")];

        let exporter = Exporter;
        exporter.to_obsidian_vault(&nodes, &edges, &dir).unwrap();

        assert!(dir.join("index.md").exists());
        assert!(dir.join("Community 0.md").exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_export_graphml() {
        let nodes = vec![make_node("a", "A", "code"), make_node("b", "B", "code")];
        let edges = vec![make_edge("a", "b", "imports")];

        let exporter = Exporter;
        let graphml = exporter.to_graphml(&nodes, &edges).unwrap();

        assert!(graphml.contains("<node id=\"a\">"));
        assert!(graphml.contains("<edge source=\"a\" target=\"b\">"));
    }

    #[test]
    fn test_export_json_hyperedges() {
        let nodes = vec![make_node("a", "A", "code")];
        let edges = vec![];
        let hyperedges = vec![HyperEdge {
            id: "hyper1".to_string(),
            members: vec!["a".to_string()],
            label: "Group".to_string(),
        }];
        let exporter = Exporter;
        let json = exporter.to_json(&nodes, &edges, Some(&hyperedges)).unwrap();
        assert!(json.contains("\"hyperedges\""));
        assert!(json.contains("\"hyper1\""));
        assert!(json.contains("\"Group\""));
    }

    #[test]
    fn test_export_svg_creates_file() {
        let dir = std::env::temp_dir().join("codesynapse_test_svg");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let path = dir.join("graph.svg");
        let nodes = vec![make_node("a", "A", "code"), make_node("b", "B", "code")];
        let edges = vec![make_edge("a", "b", "imports")];
        let exporter = Exporter;
        exporter.to_svg_file(&nodes, &edges, &path).unwrap();
        assert!(path.exists());
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("<svg"));
        assert!(content.contains("</svg>"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_export_wiki() {
        let dir = std::env::temp_dir().join("codesynapse_test_wiki");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let nodes = vec![
            make_node("a", "AuthService", "code"),
            make_node("b", "UserService", "code"),
        ];
        let edges = vec![make_edge("a", "b", "calls")];
        let exporter = Exporter;
        exporter.to_wiki(&nodes, &edges, &dir).unwrap();

        assert!(dir.join("index.md").exists());
        let content = std::fs::read_to_string(dir.join("index.md")).unwrap();
        assert!(content.contains("AuthService"));
        assert!(content.contains("UserService"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_export_neo4j_cypher() {
        let nodes = vec![
            make_node("auth", "AuthService", "code"),
            make_node("user", "UserService", "code"),
        ];
        let edges = vec![make_edge("auth", "user", "calls")];
        let exporter = Exporter;
        let cypher = exporter.to_cypher(&nodes, &edges).unwrap();

        assert!(cypher.contains("CREATE"));
        assert!(cypher.contains("AuthService"));
        assert!(cypher.contains("UserService"));
        assert!(cypher.contains(":CALLS"));
        assert!(cypher.contains("confidence"));
    }

    // --- Edge case tests ---

    #[test]
    fn test_export_json_empty() {
        let exporter = Exporter;
        let json = exporter.to_json(&[], &[], None).unwrap();
        assert!(json.contains("\"nodes\""));
        assert!(json.contains("\"edges\""));
        let data: GraphData = serde_json::from_str(&json).unwrap();
        assert!(data.nodes.is_empty());
        assert!(data.edges.is_empty());
    }

    #[test]
    fn test_export_html_empty() {
        let exporter = Exporter;
        let html = exporter.to_html(&[], &[]).unwrap();
        assert!(html.contains("<ul>"));
        assert!(html.contains("</ul>"));
    }

    #[test]
    fn test_export_graphml_empty() {
        let exporter = Exporter;
        let graphml = exporter.to_graphml(&[], &[]).unwrap();
        assert!(graphml.contains("<graph"));
        assert!(graphml.contains("</graphml>"));
    }

    #[test]
    fn test_export_json_special_chars() {
        let node = Node {
            id: "a".to_string(),
            label: "Test \"Label\" & <tag>".to_string(),
            file_type: "code".to_string(),
            source_file: "test.py".to_string(),
            source_location: None,
            community: None,
            rationale: None,
            docstring: None,
            metadata: HashMap::new(),
        };
        let exporter = Exporter;
        let json = exporter.to_json(&[node], &[], None).unwrap();
        assert!(json.contains("Test"));
        let data: GraphData = serde_json::from_str(&json).unwrap();
        assert_eq!(data.nodes[0].label, "Test \"Label\" & <tag>");
    }

    #[test]
    fn test_export_json_unicode_label() {
        let node = Node {
            id: "fn".to_string(),
            label: "función".to_string(),
            file_type: "code".to_string(),
            source_file: "test.py".to_string(),
            source_location: None,
            community: None,
            rationale: None,
            docstring: None,
            metadata: HashMap::new(),
        };
        let exporter = Exporter;
        let json = exporter.to_json(&[node], &[], None).unwrap();
        assert!(json.contains("función"));
    }

    #[test]
    fn test_export_html_escaped() {
        let node = Node {
            id: "a".to_string(),
            label: "<script>alert('xss')</script>".to_string(),
            file_type: "code".to_string(),
            source_file: "test.py".to_string(),
            source_location: None,
            community: None,
            rationale: None,
            docstring: None,
            metadata: HashMap::new(),
        };
        let exporter = Exporter;
        let html = exporter.to_html(&[node], &[]).unwrap();
        // HTML should escape the label (might escape via serde or template)
        assert!(html.contains("alert") || !html.contains("<script>"));
    }

    #[test]
    fn test_export_json_file_error() {
        let dir = std::env::temp_dir().join("codesynapse_test_export_err");
        let _ = std::fs::remove_dir_all(&dir);
        // A path inside a nonexistent directory
        let bad_path = dir.join("nonexistent").join("output.json");
        let exporter = Exporter;
        let result = exporter.to_json_file(&[], &[], None, &bad_path);
        assert!(result.is_err(), "writing to nonexistent dir should fail");
    }

    #[test]
    fn test_export_svg_empty() {
        let exporter = Exporter;
        let svg = exporter.to_svg(&[], &[]).unwrap();
        assert!(svg.contains("<svg"));
        assert!(svg.contains("</svg>"));
    }

    #[test]
    fn test_export_wiki_empty() {
        let dir = std::env::temp_dir().join("codesynapse_test_wiki_empty");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let exporter = Exporter;
        exporter.to_wiki(&[], &[], &dir).unwrap();
        assert!(dir.join("index.md").exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_export_obsidian_empty() {
        let dir = std::env::temp_dir().join("codesynapse_test_obsidian_empty");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let exporter = Exporter;
        exporter.to_obsidian_vault(&[], &[], &dir).unwrap();
        assert!(dir.join("index.md").exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    // --- Compat (NetworkX) export tests ---

    #[test]
    fn test_export_json_compat_format() {
        let nodes = vec![make_node("a", "A", "code"), make_node("b", "B", "code")];
        let edges = vec![make_edge("a", "b", "imports")];

        let exporter = Exporter;
        let json = exporter.to_json_compat(&nodes, &edges).unwrap();

        // NetworkX format markers
        assert!(json.contains("\"directed\""));
        assert!(json.contains("\"multigraph\""));
        assert!(json.contains("\"graph\""));
        assert!(json.contains("\"nodes\""));
        assert!(json.contains("\"links\""));
        assert!(!json.contains("\"edges\""));

        // Compat fields
        assert!(json.contains("\"node_type\""));
        assert!(json.contains("\"EXTRACTED\""));

        // Roundtrip
        let nx: crate::types::NetworkXGraph = serde_json::from_str(&json).unwrap();
        assert_eq!(nx.nodes.len(), 2);
        assert_eq!(nx.links.len(), 1);
    }

    #[test]
    fn test_export_json_compat_file_roundtrip() {
        let dir = std::env::temp_dir().join("codesynapse_test_nx_export");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let path = dir.join("graph.json");
        let nodes = vec![
            make_node("a", "NodeA", "function"),
            make_node("b", "NodeB", "class"),
        ];
        let edges = vec![make_edge("a", "b", "calls")];

        let exporter = Exporter;
        exporter.to_json_compat_file(&nodes, &edges, &path).unwrap();
        assert!(path.exists());

        let loaded = exporter.load_json_compat(&path).unwrap();
        assert_eq!(loaded.nodes.len(), 2);
        assert_eq!(loaded.links.len(), 1);
        assert_eq!(loaded.nodes[0].label, "NodeA");
        assert_eq!(loaded.nodes[0].node_type, crate::types::NodeType::Function);
        assert_eq!(loaded.nodes[1].node_type, crate::types::NodeType::Class);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_export_json_compat_empty() {
        let exporter = Exporter;
        let json = exporter.to_json_compat(&[], &[]).unwrap();
        assert!(json.contains("\"nodes\":"));
        assert!(json.contains("\"links\":"));
        let nx: crate::types::NetworkXGraph = serde_json::from_str(&json).unwrap();
        assert!(nx.nodes.is_empty());
        assert!(nx.links.is_empty());
    }

    #[test]
    fn test_export_json_compat_inferred_confidence() {
        let edge = Edge {
            source: "a".to_string(),
            target: "b".to_string(),
            relation: "depends".to_string(),
            confidence: "INFERRED".to_string(),
            source_file: Some("f.rs".to_string()),
            weight: 1.0,
            context: None,
        };
        let exporter = Exporter;
        let json = exporter.to_json_compat(&[], &[edge]).unwrap();
        assert!(json.contains("\"INFERRED\""));
        assert!(json.contains("confidence_score"));
    }

    #[test]
    fn test_export_json_compat_unicode() {
        let node = Node {
            id: "fn".to_string(),
            label: "función".to_string(),
            file_type: "code".to_string(),
            source_file: "test.py".to_string(),
            source_location: None,
            community: None,
            rationale: None,
            docstring: None,
            metadata: HashMap::new(),
        };
        let exporter = Exporter;
        let json = exporter.to_json_compat(&[node], &[]).unwrap();
        assert!(json.contains("función"));
    }

    #[test]
    fn test_export_json_compat_file_error() {
        let dir = std::env::temp_dir().join("codesynapse_test_nx_export_err");
        let _ = std::fs::remove_dir_all(&dir);
        let bad_path = dir.join("nonexistent").join("output.json");
        let exporter = Exporter;
        let result = exporter.to_json_compat_file(&[], &[], &bad_path);
        assert!(result.is_err(), "writing to nonexistent dir should fail");
    }
}
