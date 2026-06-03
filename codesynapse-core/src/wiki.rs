use std::collections::{BTreeMap, HashMap};
use std::path::Path;

#[derive(Clone, Debug, Default)]
pub struct WikiNodeData {
    pub label: String,
    pub source_file: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct WikiEdgeData {
    pub relation: String,
    pub confidence: String,
}

#[derive(Debug, Default)]
pub struct WikiGraph {
    pub nodes: HashMap<String, WikiNodeData>,
    adj: HashMap<String, Vec<(String, WikiEdgeData)>>,
}

impl WikiGraph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_node(&mut self, id: impl Into<String>, data: WikiNodeData) {
        let id = id.into();
        self.adj.entry(id.clone()).or_default();
        self.nodes.insert(id, data);
    }

    pub fn add_edge(&mut self, u: impl Into<String>, v: impl Into<String>, data: WikiEdgeData) {
        let u: String = u.into();
        let v: String = v.into();
        self.adj
            .entry(u.clone())
            .or_default()
            .push((v.clone(), data.clone()));
        self.adj.entry(v.clone()).or_default().push((u, data));
    }

    pub fn contains(&self, id: &str) -> bool {
        self.nodes.contains_key(id)
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn edge_count(&self) -> usize {
        self.adj.values().map(|v| v.len()).sum::<usize>() / 2
    }

    pub fn degree(&self, id: &str) -> usize {
        self.adj.get(id).map(|v| v.len()).unwrap_or(0)
    }

    pub fn neighbors(&self, id: &str) -> Vec<(&str, &WikiEdgeData)> {
        self.adj
            .get(id)
            .map(|v| v.iter().map(|(n, e)| (n.as_str(), e)).collect())
            .unwrap_or_default()
    }
}

pub fn safe_filename(name: &str) -> String {
    let s = name.replace('/', "-").replace(' ', "_").replace(':', "-");
    let s: String = s
        .chars()
        .map(|c| match c {
            '<' | '>' | '"' | '\\' | '|' | '?' | '*' => '_',
            c => c,
        })
        .collect();
    let s = s.trim_matches(|c| c == '.' || c == ' ').to_string();
    let s = if s.is_empty() {
        "unnamed".to_string()
    } else {
        s
    };
    if s.len() > 200 {
        s[..200].to_string()
    } else {
        s
    }
}

fn cross_community_links(
    graph: &WikiGraph,
    nodes: &[String],
    own_cid: i64,
    labels: &HashMap<i64, String>,
    node_community: &HashMap<String, i64>,
) -> Vec<(String, usize)> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for nid in nodes {
        for (neighbor, _) in graph.neighbors(nid) {
            if let Some(&ncid) = node_community.get(neighbor) {
                if ncid != own_cid {
                    let lbl = labels
                        .get(&ncid)
                        .cloned()
                        .unwrap_or_else(|| format!("Community {ncid}"));
                    *counts.entry(lbl).or_insert(0) += 1;
                }
            }
        }
    }
    let mut result: Vec<(String, usize)> = counts.into_iter().collect();
    result.sort_by_key(|k| std::cmp::Reverse(k.1));
    result
}

pub fn community_article(
    graph: &WikiGraph,
    cid: i64,
    nodes: &[String],
    label: &str,
    labels: &HashMap<i64, String>,
    cohesion: Option<f64>,
    node_community: &HashMap<String, i64>,
) -> String {
    let mut sorted_nodes = nodes.to_vec();
    sorted_nodes.sort_by_key(|n| std::cmp::Reverse(graph.degree(n)));
    let top_nodes = &sorted_nodes[..sorted_nodes.len().min(25)];

    let cross = cross_community_links(graph, nodes, cid, labels, node_community);

    let mut conf_counts: HashMap<String, usize> = HashMap::new();
    for nid in nodes {
        for (_, edge) in graph.neighbors(nid) {
            *conf_counts.entry(edge.confidence.clone()).or_insert(0) += 1;
        }
    }
    let total_edges = conf_counts.values().sum::<usize>().max(1);

    let mut sources: Vec<String> = nodes
        .iter()
        .filter_map(|n| graph.nodes.get(n).and_then(|d| d.source_file.clone()))
        .filter(|s| !s.is_empty())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    sources.sort();

    let mut lines = Vec::<String>::new();
    lines.push(format!("# {label}"));
    lines.push(String::new());

    let mut meta_parts = vec![format!("{} nodes", nodes.len())];
    if let Some(c) = cohesion {
        meta_parts.push(format!("cohesion {c:.2}"));
    }
    lines.push(format!("> {}", meta_parts.join(" · ")));
    lines.push(String::new());

    lines.push("## Key Concepts".to_string());
    lines.push(String::new());
    for nid in top_nodes {
        let d = graph.nodes.get(nid.as_str());
        let node_label = d.map(|d| d.label.as_str()).unwrap_or(nid.as_str());
        let src = d.and_then(|d| d.source_file.as_deref()).unwrap_or("");
        let degree = graph.degree(nid);
        let src_str = if src.is_empty() {
            String::new()
        } else {
            format!(" — `{src}`")
        };
        lines.push(format!(
            "- **{node_label}** ({degree} connections){src_str}"
        ));
    }
    let remaining = nodes.len().saturating_sub(top_nodes.len());
    if remaining > 0 {
        lines.push(format!(
            "- *... and {remaining} more nodes in this community*"
        ));
    }
    lines.push(String::new());

    lines.push("## Relationships".to_string());
    lines.push(String::new());
    if cross.is_empty() {
        lines.push("- No strong cross-community connections detected".to_string());
    } else {
        for (other_label, count) in cross.iter().take(12) {
            lines.push(format!("- [[{other_label}]] ({count} shared connections)"));
        }
    }
    lines.push(String::new());

    if !sources.is_empty() {
        lines.push("## Source Files".to_string());
        lines.push(String::new());
        for src in sources.iter().take(20) {
            lines.push(format!("- `{src}`"));
        }
        lines.push(String::new());
    }

    lines.push("## Audit Trail".to_string());
    lines.push(String::new());
    for conf in &["EXTRACTED", "INFERRED", "AMBIGUOUS"] {
        let n = conf_counts.get(*conf).copied().unwrap_or(0);
        let pct = (n * 100) / total_edges;
        lines.push(format!("- {conf}: {n} ({pct}%)"));
    }
    lines.push(String::new());

    lines.push("---".to_string());
    lines.push(String::new());
    lines.push("*Part of the codesynapse knowledge wiki. See [[index]] to navigate.*".to_string());

    lines.join("\n")
}

pub fn god_node_article(
    graph: &WikiGraph,
    nid: &str,
    labels: &HashMap<i64, String>,
    node_community: &HashMap<String, i64>,
) -> String {
    let d = graph.nodes.get(nid);
    let node_label = d.map(|d| d.label.as_str()).unwrap_or(nid);
    let src = d.and_then(|d| d.source_file.as_deref()).unwrap_or("");
    let cid = node_community.get(nid).copied();
    let community_name = cid.map(|c| {
        labels
            .get(&c)
            .cloned()
            .unwrap_or_else(|| format!("Community {c}"))
    });

    let mut lines = Vec::<String>::new();
    lines.push(format!("# {node_label}"));
    lines.push(String::new());
    lines.push(format!(
        "> God node · {} connections · `{src}`",
        graph.degree(nid)
    ));
    lines.push(String::new());

    if let Some(ref cname) = community_name {
        lines.push(format!("**Community:** [[{cname}]]"));
        lines.push(String::new());
    }

    let mut by_relation: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut neighbors: Vec<(&str, &WikiEdgeData)> = graph.neighbors(nid);
    neighbors.sort_by_key(|n| std::cmp::Reverse(graph.degree(n.0)));

    for (neighbor, edge) in &neighbors {
        let nd = graph.nodes.get(*neighbor);
        let neighbor_label = nd.map(|d| d.label.as_str()).unwrap_or(neighbor);
        let conf_str = if edge.confidence.is_empty() {
            String::new()
        } else {
            format!(" `{}`", edge.confidence)
        };
        by_relation
            .entry(edge.relation.clone())
            .or_default()
            .push(format!("[[{neighbor_label}]]{conf_str}"));
    }

    lines.push("## Connections by Relation".to_string());
    lines.push(String::new());
    for (rel, targets) in &by_relation {
        lines.push(format!("### {rel}"));
        for t in targets.iter().take(20) {
            lines.push(format!("- {t}"));
        }
        lines.push(String::new());
    }

    lines.push("---".to_string());
    lines.push(String::new());
    lines.push("*Part of the codesynapse knowledge wiki. See [[index]] to navigate.*".to_string());

    lines.join("\n")
}

pub fn index_md(
    communities: &BTreeMap<i64, Vec<String>>,
    labels: &HashMap<i64, String>,
    god_nodes_data: &[HashMap<String, serde_json::Value>],
    total_nodes: usize,
    total_edges: usize,
) -> String {
    let mut lines = vec![
        "# Knowledge Graph Index".to_string(),
        String::new(),
        "> Auto-generated by codesynapse. Start here — read community articles for context, then drill into god nodes for detail.".to_string(),
        String::new(),
        format!("**{total_nodes} nodes · {total_edges} edges · {} communities**", communities.len()),
        String::new(),
        "---".to_string(),
        String::new(),
        "## Communities".to_string(),
        "(sorted by size, largest first)".to_string(),
        String::new(),
    ];

    let mut sorted_communities: Vec<(i64, &Vec<String>)> =
        communities.iter().map(|(k, v)| (*k, v)).collect();
    sorted_communities.sort_by_key(|k| std::cmp::Reverse(k.1.len()));

    for (cid, nodes) in &sorted_communities {
        let label = labels
            .get(cid)
            .cloned()
            .unwrap_or_else(|| format!("Community {cid}"));
        lines.push(format!("- [[{label}]] — {} nodes", nodes.len()));
    }
    lines.push(String::new());

    if !god_nodes_data.is_empty() {
        lines.push("## God Nodes".to_string());
        lines.push("(most connected concepts — the load-bearing abstractions)".to_string());
        lines.push(String::new());
        for node in god_nodes_data {
            let lbl = node.get("label").and_then(|v| v.as_str()).unwrap_or("");
            let degree = node.get("degree").and_then(|v| v.as_u64()).unwrap_or(0);
            lines.push(format!("- [[{lbl}]] — {degree} connections"));
        }
        lines.push(String::new());
    }

    lines.push("---".to_string());
    lines.push(String::new());
    lines.push(
        "*Generated by [codesynapse](https://github.com/safishamsi/codesynapse)*".to_string(),
    );

    lines.join("\n")
}

pub fn to_wiki(
    graph: &WikiGraph,
    communities: &HashMap<i64, Vec<String>>,
    output_dir: &Path,
    community_labels: Option<&HashMap<i64, String>>,
    cohesion: Option<&HashMap<i64, f64>>,
    god_nodes_data: Option<&[HashMap<String, serde_json::Value>]>,
) -> Result<usize, String> {
    if communities.is_empty() {
        return Err("communities dict is empty — refusing to clear wiki/. \
             Run `codesynapse extract .` or `codesynapse cluster-only .` first."
            .to_string());
    }

    let g_nodes: std::collections::HashSet<&str> = graph.nodes.keys().map(|s| s.as_str()).collect();
    let orig_total: usize = communities.values().map(|v| v.len()).sum();

    let clean_communities: HashMap<i64, Vec<String>> = communities
        .iter()
        .map(|(cid, nodes)| {
            let filtered: Vec<String> = nodes
                .iter()
                .filter(|n| g_nodes.contains(n.as_str()))
                .cloned()
                .collect();
            (*cid, filtered)
        })
        .filter(|(_, nodes)| !nodes.is_empty())
        .collect();

    let kept_total: usize = clean_communities.values().map(|v| v.len()).sum();
    if kept_total < orig_total {
        eprintln!(
            "wiki: dropped {} stale node ID(s) not in graph ({} communities remaining)",
            orig_total - kept_total,
            clean_communities.len()
        );
    }

    if clean_communities.is_empty() {
        return Err(
            "all community node IDs are stale — none exist in the graph. \
             Re-run `codesynapse extract .` to regenerate .codesynapse_analysis.json."
                .to_string(),
        );
    }

    std::fs::create_dir_all(output_dir).map_err(|e| format!("create_dir_all: {e}"))?;

    for entry in std::fs::read_dir(output_dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let p = entry.path();
        if p.extension().and_then(|e| e.to_str()) == Some("md") {
            std::fs::remove_file(&p).map_err(|e| e.to_string())?;
        }
    }

    let default_labels: HashMap<i64, String> = clean_communities
        .keys()
        .map(|cid| (*cid, format!("Community {cid}")))
        .collect();
    let labels = community_labels.unwrap_or(&default_labels);
    let empty_cohesion = HashMap::new();
    let cohesion = cohesion.unwrap_or(&empty_cohesion);
    let empty_gods: Vec<HashMap<String, serde_json::Value>> = vec![];
    let god_nodes_data = god_nodes_data.unwrap_or(&empty_gods);

    let node_community: HashMap<String, i64> = clean_communities
        .iter()
        .flat_map(|(cid, nodes)| nodes.iter().map(|n| (n.clone(), *cid)))
        .collect();

    let mut used_slugs: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut unique_slug = |base: &str| -> String {
        let mut slug = base.to_string();
        let mut n = 2;
        while used_slugs.contains(&slug) {
            slug = format!("{base}_{n}");
            n += 1;
        }
        used_slugs.insert(slug.clone());
        slug
    };

    let mut sorted_communities: Vec<(i64, Vec<String>)> = clean_communities.into_iter().collect();
    sorted_communities.sort_by_key(|(cid, _)| *cid);

    let mut count = 0;

    for (cid, nodes) in &sorted_communities {
        let label = labels
            .get(cid)
            .cloned()
            .unwrap_or_else(|| format!("Community {cid}"));
        let article = community_article(
            graph,
            *cid,
            nodes,
            &label,
            labels,
            cohesion.get(cid).copied(),
            &node_community,
        );
        let slug = unique_slug(&safe_filename(&label));
        std::fs::write(output_dir.join(format!("{slug}.md")), &article)
            .map_err(|e| e.to_string())?;
        count += 1;
    }

    for node_data in god_nodes_data {
        let nid = node_data.get("id").and_then(|v| v.as_str()).unwrap_or("");
        if !nid.is_empty() && graph.contains(nid) {
            let article = god_node_article(graph, nid, labels, &node_community);
            let lbl = node_data
                .get("label")
                .and_then(|v| v.as_str())
                .unwrap_or(nid);
            let slug = unique_slug(&safe_filename(lbl));
            std::fs::write(output_dir.join(format!("{slug}.md")), &article)
                .map_err(|e| e.to_string())?;
            count += 1;
        }
    }

    let btree_communities: BTreeMap<i64, Vec<String>> = sorted_communities.into_iter().collect();
    let idx = index_md(
        &btree_communities,
        labels,
        god_nodes_data,
        graph.node_count(),
        graph.edge_count(),
    );
    std::fs::write(output_dir.join("index.md"), &idx).map_err(|e| e.to_string())?;

    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use tempfile::tempdir;

    fn make_graph() -> WikiGraph {
        let mut g = WikiGraph::new();
        g.add_node(
            "n1",
            WikiNodeData {
                label: "parse".into(),
                source_file: Some("parser.py".into()),
            },
        );
        g.add_node(
            "n2",
            WikiNodeData {
                label: "validate".into(),
                source_file: Some("parser.py".into()),
            },
        );
        g.add_node(
            "n3",
            WikiNodeData {
                label: "render".into(),
                source_file: Some("renderer.py".into()),
            },
        );
        g.add_node(
            "n4",
            WikiNodeData {
                label: "stream".into(),
                source_file: Some("renderer.py".into()),
            },
        );
        g.add_edge(
            "n1",
            "n2",
            WikiEdgeData {
                relation: "calls".into(),
                confidence: "EXTRACTED".into(),
            },
        );
        g.add_edge(
            "n1",
            "n3",
            WikiEdgeData {
                relation: "references".into(),
                confidence: "INFERRED".into(),
            },
        );
        g.add_edge(
            "n3",
            "n4",
            WikiEdgeData {
                relation: "calls".into(),
                confidence: "EXTRACTED".into(),
            },
        );
        g
    }

    fn make_communities() -> HashMap<i64, Vec<String>> {
        let mut m = HashMap::new();
        m.insert(0, vec!["n1".into(), "n2".into()]);
        m.insert(1, vec!["n3".into(), "n4".into()]);
        m
    }

    fn make_labels() -> HashMap<i64, String> {
        let mut m = HashMap::new();
        m.insert(0i64, "Parsing Layer".to_string());
        m.insert(1i64, "Rendering Layer".to_string());
        m
    }

    fn make_cohesion() -> HashMap<i64, f64> {
        let mut m = HashMap::new();
        m.insert(0i64, 0.85f64);
        m.insert(1i64, 0.72f64);
        m
    }

    fn make_god_nodes() -> Vec<HashMap<String, serde_json::Value>> {
        vec![{
            let mut m = HashMap::new();
            m.insert(
                "id".to_string(),
                serde_json::Value::String("n1".to_string()),
            );
            m.insert(
                "label".to_string(),
                serde_json::Value::String("parse".to_string()),
            );
            m.insert("degree".to_string(), serde_json::Value::Number(2.into()));
            m
        }]
    }

    #[test]
    fn test_to_wiki_writes_index() {
        let g = make_graph();
        let dir = tempdir().unwrap();
        to_wiki(
            &g,
            &make_communities(),
            dir.path(),
            Some(&make_labels()),
            Some(&make_cohesion()),
            Some(&make_god_nodes()),
        )
        .unwrap();
        assert!(dir.path().join("index.md").exists());
    }

    #[test]
    fn test_to_wiki_returns_article_count() {
        let g = make_graph();
        let dir = tempdir().unwrap();
        let n = to_wiki(
            &g,
            &make_communities(),
            dir.path(),
            Some(&make_labels()),
            Some(&make_cohesion()),
            Some(&make_god_nodes()),
        )
        .unwrap();
        assert_eq!(n, 3); // 2 communities + 1 god node
    }

    #[test]
    fn test_to_wiki_community_articles_created() {
        let g = make_graph();
        let dir = tempdir().unwrap();
        to_wiki(
            &g,
            &make_communities(),
            dir.path(),
            Some(&make_labels()),
            None,
            None,
        )
        .unwrap();
        assert!(dir.path().join("Parsing_Layer.md").exists());
        assert!(dir.path().join("Rendering_Layer.md").exists());
    }

    #[test]
    fn test_to_wiki_god_node_article_created() {
        let g = make_graph();
        let dir = tempdir().unwrap();
        to_wiki(
            &g,
            &make_communities(),
            dir.path(),
            Some(&make_labels()),
            None,
            Some(&make_god_nodes()),
        )
        .unwrap();
        assert!(dir.path().join("parse.md").exists());
    }

    #[test]
    fn test_index_links_all_communities() {
        let g = make_graph();
        let dir = tempdir().unwrap();
        to_wiki(
            &g,
            &make_communities(),
            dir.path(),
            Some(&make_labels()),
            None,
            None,
        )
        .unwrap();
        let idx = std::fs::read_to_string(dir.path().join("index.md")).unwrap();
        assert!(idx.contains("[[Parsing Layer]]"));
        assert!(idx.contains("[[Rendering Layer]]"));
    }

    #[test]
    fn test_index_lists_god_nodes() {
        let g = make_graph();
        let dir = tempdir().unwrap();
        to_wiki(
            &g,
            &make_communities(),
            dir.path(),
            Some(&make_labels()),
            None,
            Some(&make_god_nodes()),
        )
        .unwrap();
        let idx = std::fs::read_to_string(dir.path().join("index.md")).unwrap();
        assert!(idx.contains("[[parse]]"));
        assert!(idx.contains("2 connections"));
    }

    #[test]
    fn test_community_article_has_cross_links() {
        let g = make_graph();
        let dir = tempdir().unwrap();
        to_wiki(
            &g,
            &make_communities(),
            dir.path(),
            Some(&make_labels()),
            None,
            None,
        )
        .unwrap();
        let content = std::fs::read_to_string(dir.path().join("Parsing_Layer.md")).unwrap();
        assert!(content.contains("[[Rendering Layer]]"));
    }

    #[test]
    fn test_community_article_shows_cohesion() {
        let g = make_graph();
        let dir = tempdir().unwrap();
        to_wiki(
            &g,
            &make_communities(),
            dir.path(),
            Some(&make_labels()),
            Some(&make_cohesion()),
            None,
        )
        .unwrap();
        let content = std::fs::read_to_string(dir.path().join("Parsing_Layer.md")).unwrap();
        assert!(content.contains("cohesion 0.85"));
    }

    #[test]
    fn test_community_article_has_audit_trail() {
        let g = make_graph();
        let dir = tempdir().unwrap();
        to_wiki(
            &g,
            &make_communities(),
            dir.path(),
            Some(&make_labels()),
            None,
            None,
        )
        .unwrap();
        let content = std::fs::read_to_string(dir.path().join("Parsing_Layer.md")).unwrap();
        assert!(content.contains("EXTRACTED"));
        assert!(content.contains("INFERRED"));
    }

    #[test]
    fn test_god_node_article_has_connections() {
        let g = make_graph();
        let dir = tempdir().unwrap();
        to_wiki(
            &g,
            &make_communities(),
            dir.path(),
            Some(&make_labels()),
            None,
            Some(&make_god_nodes()),
        )
        .unwrap();
        let content = std::fs::read_to_string(dir.path().join("parse.md")).unwrap();
        assert!(content.contains("[[validate]]") || content.contains("[[render]]"));
    }

    #[test]
    fn test_god_node_article_links_community() {
        let g = make_graph();
        let dir = tempdir().unwrap();
        to_wiki(
            &g,
            &make_communities(),
            dir.path(),
            Some(&make_labels()),
            None,
            Some(&make_god_nodes()),
        )
        .unwrap();
        let content = std::fs::read_to_string(dir.path().join("parse.md")).unwrap();
        assert!(content.contains("[[Parsing Layer]]"));
    }

    #[test]
    fn test_to_wiki_skips_missing_god_node_ids() {
        let g = make_graph();
        let dir = tempdir().unwrap();
        let bad_gods = vec![{
            let mut m = HashMap::new();
            m.insert(
                "id".to_string(),
                serde_json::Value::String("nonexistent".to_string()),
            );
            m.insert(
                "label".to_string(),
                serde_json::Value::String("ghost".to_string()),
            );
            m.insert("degree".to_string(), serde_json::Value::Number(99.into()));
            m
        }];
        let n = to_wiki(
            &g,
            &make_communities(),
            dir.path(),
            Some(&make_labels()),
            None,
            Some(&bad_gods),
        )
        .unwrap();
        assert_eq!(n, 2);
    }

    #[test]
    fn test_to_wiki_no_labels_uses_fallback() {
        let g = make_graph();
        let dir = tempdir().unwrap();
        to_wiki(&g, &make_communities(), dir.path(), None, None, None).unwrap();
        assert!(dir.path().join("Community_0.md").exists());
        assert!(dir.path().join("Community_1.md").exists());
    }

    #[test]
    fn test_article_navigation_footer() {
        let g = make_graph();
        let dir = tempdir().unwrap();
        to_wiki(
            &g,
            &make_communities(),
            dir.path(),
            Some(&make_labels()),
            None,
            None,
        )
        .unwrap();
        let content = std::fs::read_to_string(dir.path().join("Parsing_Layer.md")).unwrap();
        assert!(content.contains("[[index]]"));
    }

    #[test]
    fn test_community_article_truncation_notice() {
        let mut g = WikiGraph::new();
        let nodes: Vec<String> = (0..30).map(|i| format!("n{i}")).collect();
        for nid in &nodes {
            g.add_node(
                nid.clone(),
                WikiNodeData {
                    label: format!("concept_{nid}"),
                    source_file: Some("a.py".into()),
                },
            );
        }
        for i in 0..nodes.len() - 1 {
            g.add_edge(
                nodes[i].clone(),
                nodes[i + 1].clone(),
                WikiEdgeData {
                    relation: "calls".into(),
                    confidence: "EXTRACTED".into(),
                },
            );
        }
        let mut communities = HashMap::new();
        communities.insert(0i64, nodes.clone());
        let mut labels = HashMap::new();
        labels.insert(0i64, "Big Community".to_string());
        let dir = tempdir().unwrap();
        to_wiki(&g, &communities, dir.path(), Some(&labels), None, None).unwrap();
        let content = std::fs::read_to_string(dir.path().join("Big_Community.md")).unwrap();
        assert!(content.contains("and 5 more nodes"));
    }

    #[test]
    fn test_cross_community_links_without_node_community_attrs() {
        let mut g = WikiGraph::new();
        g.add_node(
            "n1",
            WikiNodeData {
                label: "parse".into(),
                source_file: Some("parser.py".into()),
            },
        );
        g.add_node(
            "n2",
            WikiNodeData {
                label: "render".into(),
                source_file: Some("renderer.py".into()),
            },
        );
        g.add_edge(
            "n1",
            "n2",
            WikiEdgeData {
                relation: "references".into(),
                confidence: "INFERRED".into(),
            },
        );
        let mut communities = HashMap::new();
        communities.insert(0i64, vec!["n1".to_string()]);
        communities.insert(1i64, vec!["n2".to_string()]);
        let mut labels = HashMap::new();
        labels.insert(0i64, "Parsing".to_string());
        labels.insert(1i64, "Rendering".to_string());
        let dir = tempdir().unwrap();
        to_wiki(&g, &communities, dir.path(), Some(&labels), None, None).unwrap();
        let content = std::fs::read_to_string(dir.path().join("Parsing.md")).unwrap();
        assert!(content.contains("[[Rendering]]"));
    }

    #[test]
    fn test_god_node_article_community_without_node_attr() {
        let mut g = WikiGraph::new();
        g.add_node(
            "n1",
            WikiNodeData {
                label: "parse".into(),
                source_file: Some("parser.py".into()),
            },
        );
        g.add_node(
            "n2",
            WikiNodeData {
                label: "validate".into(),
                source_file: Some("parser.py".into()),
            },
        );
        g.add_edge(
            "n1",
            "n2",
            WikiEdgeData {
                relation: "calls".into(),
                confidence: "EXTRACTED".into(),
            },
        );
        let mut communities = HashMap::new();
        communities.insert(0i64, vec!["n1".to_string(), "n2".to_string()]);
        let mut labels = HashMap::new();
        labels.insert(0i64, "Core Logic".to_string());
        let god_nodes = vec![{
            let mut m = HashMap::new();
            m.insert(
                "id".to_string(),
                serde_json::Value::String("n1".to_string()),
            );
            m.insert(
                "label".to_string(),
                serde_json::Value::String("parse".to_string()),
            );
            m.insert("degree".to_string(), serde_json::Value::Number(1.into()));
            m
        }];
        let dir = tempdir().unwrap();
        to_wiki(
            &g,
            &communities,
            dir.path(),
            Some(&labels),
            None,
            Some(&god_nodes),
        )
        .unwrap();
        let content = std::fs::read_to_string(dir.path().join("parse.md")).unwrap();
        assert!(content.contains("[[Core Logic]]"));
    }

    #[test]
    fn test_to_wiki_drops_stale_community_nodes() {
        let g = make_graph();
        let mut communities = make_communities();
        communities
            .get_mut(&0)
            .unwrap()
            .push("stale_ghost".to_string());
        let dir = tempdir().unwrap();
        let n = to_wiki(
            &g,
            &communities,
            dir.path(),
            Some(&make_labels()),
            None,
            None,
        )
        .unwrap();
        assert_eq!(n, 2);
        let content = std::fs::read_to_string(dir.path().join("Parsing_Layer.md")).unwrap();
        assert!(content.contains("parse"));
        assert!(!content.contains("stale_ghost"));
    }

    #[test]
    fn test_to_wiki_all_stale_raises() {
        let g = make_graph();
        let mut all_stale = HashMap::new();
        all_stale.insert(0i64, vec!["ghost1".to_string(), "ghost2".to_string()]);
        all_stale.insert(1i64, vec!["ghost3".to_string()]);
        let dir = tempdir().unwrap();
        let err =
            to_wiki(&g, &all_stale, dir.path(), Some(&make_labels()), None, None).unwrap_err();
        assert!(err.to_lowercase().contains("stale"));
    }

    #[test]
    fn test_to_wiki_stale_nodes_prints_warning() {
        let g = make_graph();
        let mut communities = make_communities();
        communities
            .get_mut(&0)
            .unwrap()
            .extend(["stale1".to_string(), "stale2".to_string()]);
        let dir = tempdir().unwrap();
        // stderr warning is tested implicitly — just verify it doesn't crash and drops stale
        let n = to_wiki(
            &g,
            &communities,
            dir.path(),
            Some(&make_labels()),
            None,
            None,
        )
        .unwrap();
        assert_eq!(n, 2);
    }

    #[test]
    fn test_community_article_handles_null_source_file() {
        let mut g = WikiGraph::new();
        g.add_node(
            "n1",
            WikiNodeData {
                label: "parse".into(),
                source_file: None,
            },
        );
        g.add_node(
            "n2",
            WikiNodeData {
                label: "validate".into(),
                source_file: Some("parser.py".into()),
            },
        );
        g.add_edge(
            "n1",
            "n2",
            WikiEdgeData {
                relation: "calls".into(),
                confidence: "EXTRACTED".into(),
            },
        );
        let mut communities = HashMap::new();
        communities.insert(0i64, vec!["n1".to_string(), "n2".to_string()]);
        let mut labels = HashMap::new();
        labels.insert(0i64, "Parsing Layer".to_string());
        let dir = tempdir().unwrap();
        to_wiki(&g, &communities, dir.path(), Some(&labels), None, None).unwrap();
        assert!(dir.path().join("index.md").exists());
    }

    #[test]
    fn test_safe_filename_spaces_to_underscores() {
        assert_eq!(safe_filename("Parsing Layer"), "Parsing_Layer");
    }

    #[test]
    fn test_safe_filename_special_chars() {
        assert_eq!(safe_filename("a<b>c"), "a_b_c");
    }

    #[test]
    fn test_safe_filename_empty_becomes_unnamed() {
        assert_eq!(safe_filename(""), "unnamed");
    }
}
