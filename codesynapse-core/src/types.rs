use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

pub type NodeId = String;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Node {
    pub id: NodeId,
    pub label: String,
    pub file_type: String,
    pub source_file: String,
    pub source_location: Option<String>,
    pub community: Option<usize>,
    pub rationale: Option<String>,
    #[serde(default)]
    pub docstring: Option<String>,
    #[serde(default)]
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Edge {
    pub source: NodeId,
    pub target: NodeId,
    pub relation: String,
    pub confidence: String,
    pub source_file: Option<String>,
    pub weight: f64,
    #[serde(default)]
    pub context: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionFragment {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphData {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub hyperedges: Option<Vec<HyperEdge>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HyperEdge {
    pub id: String,
    pub members: Vec<NodeId>,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Community {
    pub id: usize,
    pub nodes: Vec<NodeId>,
    pub cohesion: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisResult {
    pub god_nodes: Vec<Node>,
    pub surprising_connections: Vec<Edge>,
    pub suggested_questions: Vec<String>,
    pub community_cohesion: Vec<Community>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResult {
    pub seed_nodes: Vec<Node>,
    pub neighborhood: Vec<Node>,
    pub edges: Vec<Edge>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum FileType {
    Code,
    Document,
    Paper,
    Image,
    Video,
    Data,
    Config,
}

impl FileType {
    pub fn as_str(&self) -> &'static str {
        match self {
            FileType::Code => "code",
            FileType::Document => "document",
            FileType::Paper => "paper",
            FileType::Image => "image",
            FileType::Video => "video",
            FileType::Data => "data",
            FileType::Config => "config",
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "code" => Some(FileType::Code),
            "document" => Some(FileType::Document),
            "paper" => Some(FileType::Paper),
            "image" => Some(FileType::Image),
            "video" => Some(FileType::Video),
            "data" => Some(FileType::Data),
            "config" => Some(FileType::Config),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Compat types — match codesynapse-rs GraphNode/GraphEdge for NetworkX JSON format
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Confidence {
    Extracted,
    Inferred,
    Ambiguous,
}

impl Confidence {
    pub fn score(&self) -> f64 {
        match self {
            Confidence::Extracted => 1.0,
            Confidence::Inferred => 0.7,
            Confidence::Ambiguous => 0.4,
        }
    }
}

impl From<&str> for Confidence {
    fn from(s: &str) -> Self {
        match s.to_uppercase().as_str() {
            "EXTRACTED" => Confidence::Extracted,
            "INFERRED" => Confidence::Inferred,
            "AMBIGUOUS" => Confidence::Ambiguous,
            _ => Confidence::Extracted,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub enum NodeType {
    Class,
    Function,
    Module,
    Concept,
    Paper,
    Image,
    #[default]
    File,
    Method,
    Interface,
    Enum,
    Struct,
    Trait,
    Constant,
    Variable,
    Package,
    Namespace,
}

impl NodeType {
    pub fn from_file_type(s: &str) -> Self {
        match s {
            "code" => NodeType::File,
            "function" => NodeType::Function,
            "method" => NodeType::Method,
            "module" => NodeType::Module,
            "class" => NodeType::Class,
            "interface" => NodeType::Interface,
            "struct" => NodeType::Struct,
            "enum" => NodeType::Enum,
            "trait" => NodeType::Trait,
            "constant" => NodeType::Constant,
            "export" => NodeType::Module,
            "macro" => NodeType::Function,
            "paper" => NodeType::Paper,
            "image" => NodeType::Image,
            "config" | "data" | "document" => NodeType::File,
            "concept" => NodeType::Concept,
            "package" => NodeType::Package,
            "namespace" => NodeType::Namespace,
            _ => NodeType::Variable,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CompatNode {
    pub id: String,
    pub label: String,
    pub source_file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_location: Option<String>,
    pub node_type: NodeType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub community: Option<usize>,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

impl From<&Node> for CompatNode {
    fn from(n: &Node) -> Self {
        let mut extra: HashMap<String, Value> = HashMap::new();
        extra.insert("file_type".to_string(), Value::String(n.file_type.clone()));
        if let Some(ref r) = n.rationale {
            extra.insert("rationale".to_string(), Value::String(r.clone()));
        }
        if let Some(ref d) = n.docstring {
            extra.insert("docstring".to_string(), Value::String(d.clone()));
        }
        for (k, v) in &n.metadata {
            extra.insert(k.clone(), Value::String(v.clone()));
        }
        CompatNode {
            id: n.id.clone(),
            label: n.label.clone(),
            source_file: n.source_file.clone(),
            source_location: n.source_location.clone(),
            node_type: NodeType::from_file_type(&n.file_type),
            community: n.community,
            extra,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CompatEdge {
    pub source: String,
    pub target: String,
    pub relation: String,
    pub confidence: Confidence,
    pub confidence_score: f64,
    pub source_file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_location: Option<String>,
    #[serde(default = "default_weight")]
    pub weight: f64,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

fn default_weight() -> f64 {
    1.0
}

impl From<&Edge> for CompatEdge {
    fn from(e: &Edge) -> Self {
        let confidence = Confidence::from(e.confidence.as_str());
        let confidence_score = confidence.score();
        CompatEdge {
            source: e.source.clone(),
            target: e.target.clone(),
            relation: e.relation.clone(),
            confidence,
            confidence_score,
            source_file: e.source_file.clone().unwrap_or_default(),
            source_location: None,
            weight: e.weight,
            extra: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NetworkXGraph {
    pub directed: bool,
    pub multigraph: bool,
    pub graph: HashMap<String, Value>,
    pub nodes: Vec<CompatNode>,
    pub links: Vec<CompatEdge>,
}

impl NetworkXGraph {
    pub fn from_graph_data(nodes: &[Node], edges: &[Edge]) -> Self {
        NetworkXGraph {
            directed: true,
            multigraph: false,
            graph: HashMap::new(),
            nodes: nodes.iter().map(CompatNode::from).collect(),
            links: edges.iter().map(CompatEdge::from).collect(),
        }
    }
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_node() -> Node {
        Node {
            id: "test-id".into(),
            label: "TestLabel".into(),
            file_type: "code".into(),
            source_file: "main.py".into(),
            source_location: Some("10:5".into()),
            community: Some(0),
            rationale: Some("Key function".into()),
            docstring: None,
            metadata: HashMap::from([("key".into(), "value".into())]),
        }
    }

    fn sample_edge() -> Edge {
        Edge {
            source: "a".into(),
            target: "b".into(),
            relation: "calls".into(),
            confidence: "EXTRACTED".into(),
            source_file: Some("main.py".into()),
            weight: 1.0,
            context: None,
        }
    }

    #[test]
    fn test_node_serde_roundtrip() {
        let node = sample_node();
        let json = serde_json::to_string(&node).unwrap();
        let back: Node = serde_json::from_str(&json).unwrap();
        assert_eq!(node, back);
    }

    #[test]
    fn test_edge_serde_roundtrip() {
        let edge = sample_edge();
        let json = serde_json::to_string(&edge).unwrap();
        let back: Edge = serde_json::from_str(&json).unwrap();
        assert_eq!(edge, back);
    }

    #[test]
    fn test_node_partial_fields() {
        let node = Node {
            id: "min".into(),
            label: "Minimal".into(),
            file_type: "code".into(),
            source_file: "f.rs".into(),
            source_location: None,
            community: None,
            rationale: None,
            docstring: None,
            metadata: HashMap::new(),
        };
        assert_eq!(node.id, "min");
    }

    #[test]
    fn test_edge_zero_weight() {
        let edge = Edge {
            source: "a".into(),
            target: "b".into(),
            relation: "connects".into(),
            confidence: "INFERRED".into(),
            source_file: None,
            weight: 0.0,
            context: None,
        };
        let json = serde_json::to_string(&edge).unwrap();
        let back: Edge = serde_json::from_str(&json).unwrap();
        assert_eq!(back.weight, 0.0);
    }

    #[test]
    fn test_filetype_as_str_roundtrip() {
        for ft in &[
            FileType::Code,
            FileType::Document,
            FileType::Paper,
            FileType::Image,
            FileType::Data,
            FileType::Config,
        ] {
            let s = ft.as_str();
            let back = FileType::from_str(s).unwrap();
            assert_eq!(*ft, back);
        }
    }

    #[test]
    fn test_filetype_from_str_unknown() {
        assert_eq!(FileType::from_str("unknown"), None);
        assert_eq!(FileType::from_str(""), None);
    }

    #[test]
    fn test_extraction_fragment_serde() {
        let fragment = ExtractionFragment {
            nodes: vec![sample_node()],
            edges: vec![sample_edge()],
        };
        let json = serde_json::to_string(&fragment).unwrap();
        let back: ExtractionFragment = serde_json::from_str(&json).unwrap();
        assert_eq!(back.nodes.len(), 1);
        assert_eq!(back.edges.len(), 1);
    }

    #[test]
    fn test_graph_data_serde() {
        let data = GraphData {
            nodes: vec![sample_node()],
            edges: vec![sample_edge()],
            hyperedges: Some(vec![HyperEdge {
                id: "h1".into(),
                members: vec!["a".into()],
                label: "Group".into(),
            }]),
        };
        let json = serde_json::to_string(&data).unwrap();
        let back: GraphData = serde_json::from_str(&json).unwrap();
        assert_eq!(back.nodes.len(), 1);
        assert_eq!(back.hyperedges.unwrap().len(), 1);
    }

    #[test]
    fn test_community_serde() {
        let c = Community {
            id: 42,
            nodes: vec!["a".into(), "b".into()],
            cohesion: 0.75,
        };
        let json = serde_json::to_string(&c).unwrap();
        let back: Community = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, 42);
        assert_eq!(back.nodes.len(), 2);
        assert!((back.cohesion - 0.75).abs() < 0.01);
    }

    #[test]
    fn test_analysis_result_serde() {
        let result = AnalysisResult {
            god_nodes: vec![sample_node()],
            surprising_connections: vec![sample_edge()],
            suggested_questions: vec!["What does X do?".into()],
            community_cohesion: vec![Community {
                id: 0,
                nodes: vec!["a".into()],
                cohesion: 1.0,
            }],
        };
        let json = serde_json::to_string(&result).unwrap();
        let back: AnalysisResult = serde_json::from_str(&json).unwrap();
        assert_eq!(back.god_nodes.len(), 1);
        assert_eq!(back.suggested_questions.len(), 1);
    }

    #[test]
    fn test_query_result_serde() {
        let qr = QueryResult {
            seed_nodes: vec![sample_node()],
            neighborhood: vec![],
            edges: vec![],
            truncated: false,
        };
        let json = serde_json::to_string(&qr).unwrap();
        let back: QueryResult = serde_json::from_str(&json).unwrap();
        assert!(!back.truncated);
    }

    // --- Compat type tests ---

    #[test]
    fn test_confidence_enum_serde() {
        for (variant, expected) in &[
            (Confidence::Extracted, "EXTRACTED"),
            (Confidence::Inferred, "INFERRED"),
            (Confidence::Ambiguous, "AMBIGUOUS"),
        ] {
            let json = serde_json::to_string(variant).unwrap();
            assert_eq!(json, format!("\"{}\"", expected));
            let back: Confidence = serde_json::from_str(&json).unwrap();
            assert_eq!(*variant, back);
        }
    }

    #[test]
    fn test_confidence_scores() {
        assert!((Confidence::Extracted.score() - 1.0).abs() < 0.01);
        assert!((Confidence::Inferred.score() - 0.7).abs() < 0.01);
        assert!((Confidence::Ambiguous.score() - 0.4).abs() < 0.01);
    }

    #[test]
    fn test_confidence_from_str() {
        assert_eq!(Confidence::from("EXTRACTED"), Confidence::Extracted);
        assert_eq!(Confidence::from("extracted"), Confidence::Extracted);
        assert_eq!(Confidence::from("INFERRED"), Confidence::Inferred);
        assert_eq!(Confidence::from("AMBIGUOUS"), Confidence::Ambiguous);
        assert_eq!(Confidence::from("unknown"), Confidence::Extracted);
    }

    #[test]
    fn test_nodetype_from_file_type() {
        assert_eq!(NodeType::from_file_type("code"), NodeType::File);
        assert_eq!(NodeType::from_file_type("function"), NodeType::Function);
        assert_eq!(NodeType::from_file_type("method"), NodeType::Method);
        assert_eq!(NodeType::from_file_type("module"), NodeType::Module);
        assert_eq!(NodeType::from_file_type("class"), NodeType::Class);
        assert_eq!(NodeType::from_file_type("paper"), NodeType::Paper);
        assert_eq!(NodeType::from_file_type(""), NodeType::Variable);
    }

    #[test]
    fn test_compat_node_from_node() {
        let n = sample_node();
        let cn = CompatNode::from(&n);
        assert_eq!(cn.id, "test-id");
        assert_eq!(cn.label, "TestLabel");
        assert_eq!(cn.source_file, "main.py");
        assert_eq!(cn.source_location, Some("10:5".into()));
        assert_eq!(cn.node_type, NodeType::File);
        assert_eq!(cn.community, Some(0));
        assert!(cn.extra.contains_key("file_type"));
        assert!(cn.extra.contains_key("rationale"));
        assert_eq!(
            cn.extra.get("rationale").and_then(|v| v.as_str()),
            Some("Key function")
        );
        assert_eq!(cn.extra.get("key").and_then(|v| v.as_str()), Some("value"));
    }

    #[test]
    fn test_compat_node_serde_roundtrip() {
        let n = sample_node();
        let cn = CompatNode::from(&n);
        let json = serde_json::to_string_pretty(&cn).unwrap();
        let back: CompatNode = serde_json::from_str(&json).unwrap();
        assert_eq!(cn, back);
    }

    #[test]
    fn test_compat_node_networkx_fields() {
        let n = sample_node();
        let cn = CompatNode::from(&n);
        let json = serde_json::to_string(&cn).unwrap();
        // Must have node_type not file_type at top level
        assert!(json.contains("\"node_type\""));
        // file_type must be inside extra (flattened)
        assert!(json.contains("\"file_type\""));
    }

    #[test]
    fn test_compat_edge_from_edge() {
        let e = sample_edge();
        let ce = CompatEdge::from(&e);
        assert_eq!(ce.source, "a");
        assert_eq!(ce.target, "b");
        assert_eq!(ce.relation, "calls");
        assert_eq!(ce.confidence, Confidence::Extracted);
        assert!((ce.confidence_score - 1.0).abs() < 0.01);
        assert_eq!(ce.source_file, "main.py");
        assert!((ce.weight - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_compat_edge_serde_roundtrip() {
        let e = sample_edge();
        let ce = CompatEdge::from(&e);
        let json = serde_json::to_string_pretty(&ce).unwrap();
        let back: CompatEdge = serde_json::from_str(&json).unwrap();
        assert_eq!(ce, back);
    }

    #[test]
    fn test_compat_edge_confidence_fields() {
        let e = sample_edge();
        let ce = CompatEdge::from(&e);
        let json = serde_json::to_string(&ce).unwrap();
        assert!(json.contains("\"confidence\":\"EXTRACTED\""));
        assert!(json.contains("\"confidence_score\""));
    }

    #[test]
    fn test_compat_edge_missing_source_file() {
        let e = Edge {
            source: "a".into(),
            target: "b".into(),
            relation: "calls".into(),
            confidence: "inf erred".into(),
            source_file: None,
            weight: 0.5,
            context: None,
        };
        let ce = CompatEdge::from(&e);
        assert_eq!(ce.source_file, "");
        assert_eq!(ce.confidence, Confidence::Extracted); // fallback
        assert!((ce.weight - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_networkx_graph_serde() {
        let nodes = vec![sample_node()];
        let edges = vec![sample_edge()];
        let nx = NetworkXGraph::from_graph_data(&nodes, &edges);
        assert!(nx.directed);
        assert!(!nx.multigraph);
        assert_eq!(nx.nodes.len(), 1);
        assert_eq!(nx.links.len(), 1);
    }

    #[test]
    fn test_networkx_graph_json_format() {
        let nodes = vec![sample_node()];
        let edges = vec![sample_edge()];
        let nx = NetworkXGraph::from_graph_data(&nodes, &edges);
        let json = serde_json::to_string_pretty(&nx).unwrap();
        // Must use "links" not "edges"
        assert!(json.contains("\"links\""));
        assert!(!json.contains("\"edges\""));
        // Must have top-level format fields
        assert!(json.contains("\"directed\""));
        assert!(json.contains("\"multigraph\""));
        assert!(json.contains("\"graph\""));
        assert!(json.contains("\"nodes\""));
        // Must have compat fields inside nodes
        assert!(json.contains("\"node_type\""));
        assert!(json.contains("\"EXTRACTED\""));
    }

    #[test]
    fn test_networkx_graph_empty() {
        let nx = NetworkXGraph::from_graph_data(&[], &[]);
        let json = serde_json::to_string(&nx).unwrap();
        assert!(json.contains("\"nodes\":[]"));
        assert!(json.contains("\"links\":[]"));
    }

    #[test]
    fn test_networkx_graph_serde_roundtrip() {
        let nodes = vec![sample_node()];
        let edges = vec![sample_edge()];
        let nx = NetworkXGraph::from_graph_data(&nodes, &edges);
        let json = serde_json::to_string(&nx).unwrap();
        let back: NetworkXGraph = serde_json::from_str(&json).unwrap();
        assert_eq!(nx, back);
    }

    #[test]
    fn test_compat_node_omits_optional_fields() {
        let n = Node {
            id: "min".into(),
            label: "Min".into(),
            file_type: "code".into(),
            source_file: "f.rs".into(),
            source_location: None,
            community: None,
            rationale: None,
            docstring: None,
            metadata: HashMap::new(),
        };
        let cn = CompatNode::from(&n);
        let json = serde_json::to_string(&cn).unwrap();
        // source_location and community should be absent (skip_serializing_if)
        assert!(!json.contains("\"source_location\""));
        assert!(!json.contains("\"community\""));
    }

    #[test]
    fn test_node_docstring_serde_roundtrip() {
        let n = Node {
            id: "x".into(),
            label: "Foo".into(),
            file_type: "code".into(),
            source_file: "foo.py".into(),
            source_location: None,
            community: None,
            rationale: None,
            docstring: Some("Handles payment processing.".into()),
            metadata: HashMap::new(),
        };
        let json = serde_json::to_string(&n).unwrap();
        assert!(
            json.contains("\"docstring\""),
            "docstring must appear in JSON"
        );
        let back: Node = serde_json::from_str(&json).unwrap();
        assert_eq!(back.docstring, Some("Handles payment processing.".into()));
    }

    #[test]
    fn test_node_docstring_backward_compat() {
        // Old JSON without docstring field must deserialize to docstring: None
        let old_json = r#"{"id":"y","label":"Bar","file_type":"code","source_file":"bar.py"}"#;
        let n: Node = serde_json::from_str(old_json).unwrap();
        assert_eq!(n.docstring, None);
    }

    #[test]
    fn test_compat_edge_inferred_confidence() {
        let e = Edge {
            source: "a".into(),
            target: "b".into(),
            relation: "depends".into(),
            confidence: "INFERRED".into(),
            source_file: Some("f.rs".into()),
            weight: 1.0,
            context: None,
        };
        let ce = CompatEdge::from(&e);
        assert_eq!(ce.confidence, Confidence::Inferred);
        assert!((ce.confidence_score - 0.7).abs() < 0.01);
    }
}
