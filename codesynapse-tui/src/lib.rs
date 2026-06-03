use codesynapse_core::types::{Edge, GraphData};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tab {
    Overview,
    Nodes,
    Edges,
}

impl Tab {
    pub fn next(&self) -> Self {
        match self {
            Tab::Overview => Tab::Nodes,
            Tab::Nodes => Tab::Edges,
            Tab::Edges => Tab::Overview,
        }
    }

    pub fn prev(&self) -> Self {
        match self {
            Tab::Overview => Tab::Edges,
            Tab::Nodes => Tab::Overview,
            Tab::Edges => Tab::Nodes,
        }
    }

    pub fn title(&self) -> &'static str {
        match self {
            Tab::Overview => "Overview",
            Tab::Nodes => "Nodes",
            Tab::Edges => "Edges",
        }
    }

    pub fn all() -> [Tab; 3] {
        [Tab::Overview, Tab::Nodes, Tab::Edges]
    }
}

#[derive(Debug, Clone)]
pub struct GraphStats {
    pub total_nodes: usize,
    pub total_edges: usize,
    pub language_counts: BTreeMap<String, usize>,
    pub edge_type_counts: BTreeMap<String, usize>,
    pub top_nodes_by_degree: Vec<(String, usize)>,
}

impl GraphStats {
    pub fn from_graph_data(data: &GraphData) -> Self {
        let mut language_counts: BTreeMap<String, usize> = BTreeMap::new();
        for node in &data.nodes {
            *language_counts.entry(node.file_type.clone()).or_insert(0) += 1;
        }

        let mut edge_type_counts: BTreeMap<String, usize> = BTreeMap::new();
        for edge in &data.edges {
            *edge_type_counts.entry(edge.relation.clone()).or_insert(0) += 1;
        }

        let mut degree: BTreeMap<&str, usize> = BTreeMap::new();
        for node in &data.nodes {
            degree.entry(&node.id).or_insert(0);
        }
        for edge in &data.edges {
            *degree.entry(&edge.source).or_insert(0) += 1;
            *degree.entry(&edge.target).or_insert(0) += 1;
        }

        let node_labels: BTreeMap<&str, &str> = data
            .nodes
            .iter()
            .map(|n| (n.id.as_str(), n.label.as_str()))
            .collect();

        let mut top: Vec<(String, usize)> = degree
            .iter()
            .filter_map(|(id, &deg)| node_labels.get(id).map(|label| (label.to_string(), deg)))
            .collect();
        top.sort_by_key(|k| std::cmp::Reverse(k.1));
        top.truncate(10);

        Self {
            total_nodes: data.nodes.len(),
            total_edges: data.edges.len(),
            language_counts,
            edge_type_counts,
            top_nodes_by_degree: top,
        }
    }
}

#[derive(Debug, Clone)]
pub struct NodeSummary {
    pub id: String,
    pub label: String,
    pub file_type: String,
    pub source_file: String,
    pub in_degree: usize,
    pub out_degree: usize,
}

impl NodeSummary {
    pub fn degree(&self) -> usize {
        self.in_degree + self.out_degree
    }
}

pub struct TuiApp {
    pub stats: GraphStats,
    pub selected_tab: Tab,
    pub node_list: Vec<NodeSummary>,
    pub edge_list: Vec<Edge>,
    pub scroll_offset: usize,
    pub filter: String,
}

impl TuiApp {
    pub fn new(data: GraphData) -> Self {
        let stats = GraphStats::from_graph_data(&data);

        let mut in_deg: BTreeMap<&str, usize> = BTreeMap::new();
        let mut out_deg: BTreeMap<&str, usize> = BTreeMap::new();
        for edge in &data.edges {
            *out_deg.entry(&edge.source).or_insert(0) += 1;
            *in_deg.entry(&edge.target).or_insert(0) += 1;
        }

        let mut node_list: Vec<NodeSummary> = data
            .nodes
            .iter()
            .map(|n| NodeSummary {
                id: n.id.clone(),
                label: n.label.clone(),
                file_type: n.file_type.clone(),
                source_file: n.source_file.clone(),
                in_degree: *in_deg.get(n.id.as_str()).unwrap_or(&0),
                out_degree: *out_deg.get(n.id.as_str()).unwrap_or(&0),
            })
            .collect();
        node_list.sort_by_key(|n| std::cmp::Reverse(n.degree()));

        Self {
            stats,
            selected_tab: Tab::Overview,
            node_list,
            edge_list: data.edges,
            scroll_offset: 0,
            filter: String::new(),
        }
    }

    pub fn next_tab(&mut self) {
        self.selected_tab = self.selected_tab.next();
        self.scroll_offset = 0;
    }

    pub fn prev_tab(&mut self) {
        self.selected_tab = self.selected_tab.prev();
        self.scroll_offset = 0;
    }

    pub fn filtered_nodes(&self) -> Vec<&NodeSummary> {
        if self.filter.is_empty() {
            return self.node_list.iter().collect();
        }
        let q = self.filter.to_lowercase();
        self.node_list
            .iter()
            .filter(|n| {
                n.label.to_lowercase().contains(&q)
                    || n.file_type.to_lowercase().contains(&q)
                    || n.source_file.to_lowercase().contains(&q)
            })
            .collect()
    }

    pub fn set_filter(&mut self, f: String) {
        self.filter = f;
        self.scroll_offset = 0;
    }

    pub fn scroll_down(&mut self) {
        let max = self.visible_list_len().saturating_sub(1);
        if self.scroll_offset < max {
            self.scroll_offset += 1;
        }
    }

    pub fn scroll_up(&mut self) {
        self.scroll_offset = self.scroll_offset.saturating_sub(1);
    }

    fn visible_list_len(&self) -> usize {
        match self.selected_tab {
            Tab::Nodes => self.filtered_nodes().len(),
            Tab::Edges => self.edge_list.len(),
            Tab::Overview => self.stats.top_nodes_by_degree.len(),
        }
    }
}

pub fn load_graph_data(path: &std::path::Path) -> Result<GraphData, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    serde_json::from_str(&content).map_err(|e| format!("invalid JSON in {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use codesynapse_core::types::{Edge, GraphData, Node};
    use std::collections::HashMap;

    fn node(id: &str, label: &str, file_type: &str) -> Node {
        Node {
            id: id.to_string(),
            label: label.to_string(),
            file_type: file_type.to_string(),
            source_file: format!("{}.py", label.to_lowercase()),
            source_location: None,
            community: None,
            rationale: None,
            docstring: None,
            metadata: HashMap::new(),
        }
    }

    fn edge(src: &str, tgt: &str, rel: &str) -> Edge {
        Edge {
            source: src.to_string(),
            target: tgt.to_string(),
            relation: rel.to_string(),
            confidence: "HIGH".to_string(),
            source_file: None,
            weight: 1.0,
            context: None,
        }
    }

    fn empty_data() -> GraphData {
        GraphData {
            nodes: vec![],
            edges: vec![],
            hyperedges: None,
        }
    }

    #[test]
    fn tab_next_cycles() {
        assert_eq!(Tab::Overview.next(), Tab::Nodes);
        assert_eq!(Tab::Nodes.next(), Tab::Edges);
        assert_eq!(Tab::Edges.next(), Tab::Overview);
    }

    #[test]
    fn tab_prev_cycles() {
        assert_eq!(Tab::Overview.prev(), Tab::Edges);
        assert_eq!(Tab::Nodes.prev(), Tab::Overview);
        assert_eq!(Tab::Edges.prev(), Tab::Nodes);
    }

    #[test]
    fn tab_titles() {
        assert_eq!(Tab::Overview.title(), "Overview");
        assert_eq!(Tab::Nodes.title(), "Nodes");
        assert_eq!(Tab::Edges.title(), "Edges");
    }

    #[test]
    fn stats_empty_graph() {
        let stats = GraphStats::from_graph_data(&empty_data());
        assert_eq!(stats.total_nodes, 0);
        assert_eq!(stats.total_edges, 0);
        assert!(stats.language_counts.is_empty());
        assert!(stats.edge_type_counts.is_empty());
        assert!(stats.top_nodes_by_degree.is_empty());
    }

    #[test]
    fn stats_counts_nodes_and_edges() {
        let data = GraphData {
            nodes: vec![node("a", "A", "python"), node("b", "B", "rust")],
            edges: vec![edge("a", "b", "calls"), edge("b", "a", "calls")],
            hyperedges: None,
        };
        let stats = GraphStats::from_graph_data(&data);
        assert_eq!(stats.total_nodes, 2);
        assert_eq!(stats.total_edges, 2);
    }

    #[test]
    fn stats_language_breakdown() {
        let data = GraphData {
            nodes: vec![
                node("a", "A", "python"),
                node("b", "B", "python"),
                node("c", "C", "rust"),
            ],
            edges: vec![],
            hyperedges: None,
        };
        let stats = GraphStats::from_graph_data(&data);
        assert_eq!(stats.language_counts["python"], 2);
        assert_eq!(stats.language_counts["rust"], 1);
    }

    #[test]
    fn stats_edge_type_counts() {
        let data = GraphData {
            nodes: vec![node("a", "A", "python"), node("b", "B", "python")],
            edges: vec![
                edge("a", "b", "calls"),
                edge("a", "b", "calls"),
                edge("b", "a", "imports"),
            ],
            hyperedges: None,
        };
        let stats = GraphStats::from_graph_data(&data);
        assert_eq!(stats.edge_type_counts["calls"], 2);
        assert_eq!(stats.edge_type_counts["imports"], 1);
    }

    #[test]
    fn stats_top_nodes_sorted_by_degree() {
        let data = GraphData {
            nodes: vec![
                node("a", "A", "python"),
                node("b", "B", "python"),
                node("c", "C", "python"),
            ],
            edges: vec![
                edge("a", "b", "calls"),
                edge("a", "c", "calls"),
                edge("b", "c", "calls"),
            ],
            hyperedges: None,
        };
        let stats = GraphStats::from_graph_data(&data);
        assert!(!stats.top_nodes_by_degree.is_empty());
        for w in stats.top_nodes_by_degree.windows(2) {
            assert!(w[0].1 >= w[1].1, "top nodes not sorted desc");
        }
    }

    #[test]
    fn stats_top_nodes_capped_at_10() {
        let nodes: Vec<Node> = (0..20)
            .map(|i| node(&format!("n{i}"), &format!("N{i}"), "python"))
            .collect();
        let edges: Vec<Edge> = (0..20)
            .map(|i| edge(&format!("n{i}"), "n0", "calls"))
            .collect();
        let data = GraphData {
            nodes,
            edges,
            hyperedges: None,
        };
        let stats = GraphStats::from_graph_data(&data);
        assert!(stats.top_nodes_by_degree.len() <= 10);
    }

    #[test]
    fn app_new_initializes_on_overview() {
        let app = TuiApp::new(empty_data());
        assert_eq!(app.selected_tab, Tab::Overview);
    }

    #[test]
    fn app_next_prev_tab() {
        let mut app = TuiApp::new(empty_data());
        app.next_tab();
        assert_eq!(app.selected_tab, Tab::Nodes);
        app.next_tab();
        assert_eq!(app.selected_tab, Tab::Edges);
        app.next_tab();
        assert_eq!(app.selected_tab, Tab::Overview);
        app.prev_tab();
        assert_eq!(app.selected_tab, Tab::Edges);
    }

    #[test]
    fn app_tab_switch_resets_scroll() {
        let data = GraphData {
            nodes: (0..5)
                .map(|i| node(&format!("n{i}"), &format!("N{i}"), "python"))
                .collect(),
            edges: vec![],
            hyperedges: None,
        };
        let mut app = TuiApp::new(data);
        app.next_tab(); // Nodes
        app.scroll_down();
        app.scroll_down();
        assert!(app.scroll_offset > 0);
        app.next_tab(); // Edges
        assert_eq!(app.scroll_offset, 0);
    }

    #[test]
    fn app_scroll_down_bounded() {
        let data = GraphData {
            nodes: vec![node("a", "A", "python"), node("b", "B", "python")],
            edges: vec![],
            hyperedges: None,
        };
        let mut app = TuiApp::new(data);
        app.next_tab(); // Nodes tab, 2 items
        app.scroll_down();
        app.scroll_down();
        app.scroll_down();
        assert_eq!(app.scroll_offset, 1); // max = len - 1 = 1
    }

    #[test]
    fn app_scroll_up_bounded() {
        let mut app = TuiApp::new(empty_data());
        app.scroll_up();
        app.scroll_up();
        assert_eq!(app.scroll_offset, 0);
    }

    #[test]
    fn app_filter_nodes() {
        let data = GraphData {
            nodes: vec![
                node("a", "DatabaseManager", "python"),
                node("b", "UserService", "python"),
                node("c", "AuthHandler", "rust"),
            ],
            edges: vec![],
            hyperedges: None,
        };
        let mut app = TuiApp::new(data);
        app.set_filter("database".to_string());
        let filtered = app.filtered_nodes();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].label, "DatabaseManager");
    }

    #[test]
    fn app_filter_empty_returns_all() {
        let data = GraphData {
            nodes: vec![node("a", "A", "python"), node("b", "B", "rust")],
            edges: vec![],
            hyperedges: None,
        };
        let app = TuiApp::new(data);
        assert_eq!(app.filtered_nodes().len(), 2);
    }

    #[test]
    fn app_filter_resets_scroll() {
        let data = GraphData {
            nodes: vec![
                node("a", "A", "python"),
                node("b", "B", "python"),
                node("c", "C", "python"),
            ],
            edges: vec![],
            hyperedges: None,
        };
        let mut app = TuiApp::new(data);
        app.next_tab(); // Nodes
        app.scroll_down();
        app.scroll_down();
        app.set_filter("A".to_string());
        assert_eq!(app.scroll_offset, 0);
    }

    #[test]
    fn node_summary_degree() {
        let ns = NodeSummary {
            id: "x".to_string(),
            label: "X".to_string(),
            file_type: "python".to_string(),
            source_file: "x.py".to_string(),
            in_degree: 3,
            out_degree: 7,
        };
        assert_eq!(ns.degree(), 10);
    }

    #[test]
    fn load_graph_data_invalid_path() {
        let result = load_graph_data(std::path::Path::new("/nonexistent/path.json"));
        assert!(result.is_err());
    }
}
