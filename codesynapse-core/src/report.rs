use crate::types::{Edge, GraphData, Node};
use std::collections::HashMap;

pub struct GodNodeEntry {
    pub label: String,
    pub degree: usize,
}

pub struct SurprisingConnectionEntry {
    pub source: String,
    pub target: String,
    pub relation: String,
    pub confidence: String,
    pub confidence_score: Option<f64>,
    pub source_files: Vec<String>,
    pub note: String,
}

pub struct DetectionResult {
    pub total_files: usize,
    pub total_words: usize,
    pub warning: Option<String>,
}

pub struct TokenCost {
    pub input: usize,
    pub output: usize,
}

pub struct SuggestedQuestion {
    pub question: Option<String>,
    pub why: String,
    pub question_type: Option<String>,
}

fn fmt_thousands(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out.chars().rev().collect()
}

fn safe_community_name(label: &str) -> String {
    let cleaned: String = label
        .replace("\r\n", " ")
        .replace(['\r', '\n'], " ")
        .chars()
        .filter(|c| !r#"\/*?:"<>|#^[]"#.contains(*c))
        .collect::<String>()
        .trim()
        .to_string();
    let re_ext = regex::Regex::new(r"(?i)\.(md|mdx|markdown)$").unwrap();
    let cleaned = re_ext.replace(&cleaned, "").to_string();
    if cleaned.is_empty() {
        "unnamed".to_string()
    } else {
        cleaned
    }
}

fn is_file_node(node: &Node) -> bool {
    node.label.ends_with("()") || node.label.starts_with('.')
}

#[allow(clippy::too_many_arguments)]
pub fn generate(
    graph: &GraphData,
    communities: &HashMap<usize, Vec<String>>,
    cohesion_scores: &HashMap<usize, f64>,
    community_labels: &HashMap<usize, String>,
    god_node_list: &[GodNodeEntry],
    surprise_list: &[SurprisingConnectionEntry],
    detection_result: &DetectionResult,
    token_cost: &TokenCost,
    root: &str,
    suggested_questions: Option<&[SuggestedQuestion]>,
    min_community_size: usize,
    built_at_commit: Option<&str>,
) -> String {
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();

    let node_map: HashMap<&str, &Node> = graph.nodes.iter().map(|n| (n.id.as_str(), n)).collect();

    let confidences: Vec<&str> = graph.edges.iter().map(|e| e.confidence.as_str()).collect();
    let total = confidences.len().max(1);
    let ext_pct = (confidences.iter().filter(|&&c| c == "EXTRACTED").count() * 100) / total;
    let inf_pct = (confidences.iter().filter(|&&c| c == "INFERRED").count() * 100) / total;
    let amb_pct = (confidences.iter().filter(|&&c| c == "AMBIGUOUS").count() * 100) / total;

    let inf_edges: Vec<&Edge> = graph
        .edges
        .iter()
        .filter(|e| e.confidence == "INFERRED")
        .collect();

    let thin_count = communities
        .values()
        .filter(|nodes| {
            let real = nodes
                .iter()
                .filter(|id| {
                    node_map
                        .get(id.as_str())
                        .map(|n| !is_file_node(n))
                        .unwrap_or(true)
                })
                .count();
            real > 0 && real < min_community_size
        })
        .count();

    let non_empty: Vec<usize> = communities
        .keys()
        .copied()
        .filter(|cid| {
            communities[cid].iter().any(|id| {
                node_map
                    .get(id.as_str())
                    .map(|n| !is_file_node(n))
                    .unwrap_or(true)
            })
        })
        .collect();

    let shown_count = non_empty.len() - thin_count;

    let mut lines: Vec<String> = vec![
        format!("# Graph Report - {}  ({})", root, today),
        String::new(),
        "## Corpus Check".to_string(),
    ];

    if let Some(warn) = &detection_result.warning {
        lines.push(format!("- {}", warn));
    } else {
        lines.push(format!(
            "- {} files · ~{} words",
            detection_result.total_files,
            fmt_thousands(detection_result.total_words)
        ));
        lines
            .push("- Verdict: corpus is large enough that graph structure adds value.".to_string());
    }

    let summary_suffix = if thin_count > 0 {
        format!(" ({} shown, {} thin omitted)", shown_count, thin_count)
    } else {
        String::new()
    };

    lines.push(String::new());
    lines.push("## Summary".to_string());
    lines.push(format!(
        "- {} nodes · {} edges · {} communities{}",
        graph.nodes.len(),
        graph.edges.len(),
        communities.len(),
        summary_suffix
    ));

    let inf_avg_part = if !inf_edges.is_empty() {
        format!(" · INFERRED: {} edges", inf_edges.len())
    } else {
        String::new()
    };
    lines.push(format!(
        "- Extraction: {}% EXTRACTED · {}% INFERRED · {}% AMBIGUOUS{}",
        ext_pct, inf_pct, amb_pct, inf_avg_part
    ));
    lines.push(format!(
        "- Token cost: {} input · {} output",
        fmt_thousands(token_cost.input),
        fmt_thousands(token_cost.output)
    ));

    if let Some(commit) = built_at_commit {
        lines.push(String::new());
        lines.push("## Graph Freshness".to_string());
        lines.push(format!(
            "- Built from commit: `{}`",
            &commit[..commit.len().min(8)]
        ));
        lines.push(
            "- Run `git rev-parse HEAD` and compare to check if the graph is stale.".to_string(),
        );
        lines.push("- Run `codesynapse update .` after code changes (no API cost).".to_string());
    }

    if !non_empty.is_empty() {
        lines.push(String::new());
        lines.push("## Community Hubs (Navigation)".to_string());
        let mut sorted_cids: Vec<usize> = non_empty.clone();
        sorted_cids.sort();
        for cid in &sorted_cids {
            let label = community_labels
                .get(cid)
                .map(|s| s.as_str())
                .unwrap_or("unnamed");
            let safe = safe_community_name(label);
            lines.push(format!("- [[_COMMUNITY_{}|{}]]", safe, label));
        }
    }

    lines.push(String::new());
    lines.push("## God Nodes (most connected - your core abstractions)".to_string());
    for (i, node) in god_node_list.iter().enumerate() {
        lines.push(format!(
            "{}. `{}` - {} edges",
            i + 1,
            node.label,
            node.degree
        ));
    }

    lines.push(String::new());
    lines.push("## Surprising Connections (you probably didn't know these)".to_string());
    if surprise_list.is_empty() {
        lines.push(
            "- None detected - all connections are within the same source files.".to_string(),
        );
    } else {
        for s in surprise_list {
            let conf_tag = if s.confidence == "INFERRED" {
                if let Some(score) = s.confidence_score {
                    format!("INFERRED {:.2}", score)
                } else {
                    s.confidence.clone()
                }
            } else {
                s.confidence.clone()
            };
            let sem_tag = if s.relation == "semantically_similar_to" {
                " [semantically similar]"
            } else {
                ""
            };
            lines.push(format!(
                "- `{}` --{}--> `{}`  [{}]{}",
                s.source, s.relation, s.target, conf_tag, sem_tag
            ));
            let f0 = s.source_files.first().map(|s| s.as_str()).unwrap_or("");
            let f1 = s.source_files.get(1).map(|s| s.as_str()).unwrap_or("");
            let note_part = if s.note.is_empty() {
                String::new()
            } else {
                format!("  _{}_", s.note)
            };
            lines.push(format!("  {} → {}{}", f0, f1, note_part));
        }
    }

    if let Some(hyperedges) = &graph.hyperedges {
        if !hyperedges.is_empty() {
            lines.push(String::new());
            lines.push("## Hyperedges (group relationships)".to_string());
            for h in hyperedges {
                let node_labels = h.members.join(", ");
                lines.push(format!("- **{}** — {}", h.label, node_labels));
            }
        }
    }

    lines.push(String::new());
    lines.push(format!(
        "## Communities ({} total, {} thin omitted)",
        communities.len(),
        thin_count
    ));

    let mut sorted_cids: Vec<usize> = communities.keys().copied().collect();
    sorted_cids.sort();
    for cid in &sorted_cids {
        let nodes = &communities[cid];
        let label = community_labels
            .get(cid)
            .map(|s| s.as_str())
            .unwrap_or("unnamed");
        let score = cohesion_scores.get(cid).copied().unwrap_or(0.0);
        let real_nodes: Vec<&str> = nodes
            .iter()
            .filter(|id| {
                node_map
                    .get(id.as_str())
                    .map(|n| !is_file_node(n))
                    .unwrap_or(true)
            })
            .map(|s| s.as_str())
            .collect();
        if real_nodes.is_empty() || real_nodes.len() < min_community_size {
            continue;
        }
        let display: Vec<String> = real_nodes[..real_nodes.len().min(8)]
            .iter()
            .map(|id| {
                node_map
                    .get(*id)
                    .map(|n| n.label.as_str())
                    .unwrap_or(id)
                    .to_string()
            })
            .collect();
        let suffix = if real_nodes.len() > 8 {
            format!(" (+{} more)", real_nodes.len() - 8)
        } else {
            String::new()
        };
        lines.push(String::new());
        lines.push(format!("### Community {} - \"{}\"", cid, label));
        lines.push(format!("Cohesion: {:.2}", score));
        lines.push(format!(
            "Nodes ({}): {}{}",
            real_nodes.len(),
            display.join(", "),
            suffix
        ));
    }

    let ambiguous: Vec<&Edge> = graph
        .edges
        .iter()
        .filter(|e| e.confidence == "AMBIGUOUS")
        .collect();
    if !ambiguous.is_empty() {
        lines.push(String::new());
        lines.push("## Ambiguous Edges - Review These".to_string());
        for e in &ambiguous {
            let ul = node_map
                .get(e.source.as_str())
                .map(|n| n.label.as_str())
                .unwrap_or(&e.source);
            let vl = node_map
                .get(e.target.as_str())
                .map(|n| n.label.as_str())
                .unwrap_or(&e.target);
            lines.push(format!("- `{}` → `{}`  [AMBIGUOUS]", ul, vl));
            lines.push(format!(
                "  {} · relation: {}",
                e.source_file.as_deref().unwrap_or(""),
                e.relation
            ));
        }
    }

    let degree_map: HashMap<&str, usize> = {
        let mut m: HashMap<&str, usize> = HashMap::new();
        for e in &graph.edges {
            *m.entry(e.source.as_str()).or_insert(0) += 1;
            *m.entry(e.target.as_str()).or_insert(0) += 1;
        }
        m
    };
    let isolated: Vec<&Node> = graph
        .nodes
        .iter()
        .filter(|n| {
            *degree_map.get(n.id.as_str()).unwrap_or(&0) <= 1
                && !is_file_node(n)
                && n.file_type != "rationale"
        })
        .collect();
    let thin_communities_count = communities
        .values()
        .filter(|nodes| {
            let cnt = nodes
                .iter()
                .filter(|id| {
                    node_map
                        .get(id.as_str())
                        .map(|n| !is_file_node(n))
                        .unwrap_or(true)
                })
                .count();
            cnt > 0 && cnt < 3
        })
        .count();
    let gap_count = isolated.len() + thin_communities_count;

    if gap_count > 0 || amb_pct > 20 {
        lines.push(String::new());
        lines.push("## Knowledge Gaps".to_string());
        if !isolated.is_empty() {
            let isolated_labels: Vec<String> = isolated[..isolated.len().min(5)]
                .iter()
                .map(|n| format!("`{}`", n.label))
                .collect();
            let suffix = if isolated.len() > 5 {
                format!(" (+{} more)", isolated.len() - 5)
            } else {
                String::new()
            };
            lines.push(format!(
                "- **{} isolated node(s):** {}{}",
                isolated.len(),
                isolated_labels.join(", "),
                suffix
            ));
            lines.push(
                "  These have ≤1 connection - possible missing edges or undocumented components."
                    .to_string(),
            );
        }
        if thin_communities_count > 0 {
            lines.push(format!(
                "- **{} thin communities (<{} nodes) omitted from report** — run `codesynapse query` to explore isolated nodes.",
                thin_communities_count, min_community_size
            ));
        }
        if amb_pct > 20 {
            lines.push(format!(
                "- **High ambiguity: {}% of edges are AMBIGUOUS.** Review the Ambiguous Edges section above.",
                amb_pct
            ));
        }
    }

    if let Some(questions) = suggested_questions {
        if !questions.is_empty() {
            lines.push(String::new());
            lines.push("## Suggested Questions".to_string());
            let no_signal = questions.len() == 1
                && questions[0].question_type.as_deref().unwrap_or("") == "no_signal";
            if no_signal {
                lines.push(format!("_{}_", questions[0].why));
            } else {
                lines.push("_Questions this graph is uniquely positioned to answer:_".to_string());
                lines.push(String::new());
                for q in questions {
                    if let Some(question) = &q.question {
                        lines.push(format!("- **{}**", question));
                        lines.push(format!("  _{}_", q.why));
                    }
                }
            }
        }
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Edge, Node};
    use std::collections::HashMap;

    fn make_node(id: &str, label: &str) -> Node {
        Node {
            id: id.into(),
            label: label.into(),
            file_type: "code".into(),
            source_file: "src.py".into(),
            source_location: None,
            community: None,
            rationale: None,
            docstring: None,
            metadata: HashMap::new(),
        }
    }

    fn make_edge(src: &str, tgt: &str, conf: &str, rel: &str) -> Edge {
        Edge {
            source: src.into(),
            target: tgt.into(),
            relation: rel.into(),
            confidence: conf.into(),
            source_file: Some("src.py".into()),
            weight: 1.0,
            context: None,
        }
    }

    #[allow(clippy::type_complexity)]
    fn test_inputs() -> (
        GraphData,
        HashMap<usize, Vec<String>>,
        HashMap<usize, f64>,
        HashMap<usize, String>,
        Vec<GodNodeEntry>,
        Vec<SurprisingConnectionEntry>,
        DetectionResult,
        TokenCost,
    ) {
        let graph = GraphData {
            nodes: vec![
                make_node("n1", "Foo"),
                make_node("n2", "Bar"),
                make_node("n3", "Baz"),
                make_node("n4", "Qux"),
            ],
            edges: vec![
                make_edge("n1", "n2", "EXTRACTED", "calls"),
                make_edge("n2", "n3", "INFERRED", "references"),
                make_edge("n3", "n4", "AMBIGUOUS", "imports"),
            ],
            hyperedges: None,
        };
        let communities: HashMap<usize, Vec<String>> = {
            let mut m = HashMap::new();
            m.insert(0, vec!["n1".into(), "n2".into(), "n3".into()]);
            m.insert(1, vec!["n4".into()]);
            m
        };
        let cohesion: HashMap<usize, f64> = {
            let mut m = HashMap::new();
            m.insert(0, 0.75);
            m.insert(1, 0.50);
            m
        };
        let labels: HashMap<usize, String> = {
            let mut m = HashMap::new();
            m.insert(0, "Community 0".into());
            m.insert(1, "Community 1".into());
            m
        };
        let gods = vec![GodNodeEntry {
            label: "Foo".into(),
            degree: 5,
        }];
        let surprises = vec![SurprisingConnectionEntry {
            source: "Foo".into(),
            target: "Baz".into(),
            relation: "calls".into(),
            confidence: "EXTRACTED".into(),
            confidence_score: None,
            source_files: vec!["a.py".into(), "b.py".into()],
            note: String::new(),
        }];
        let detection = DetectionResult {
            total_files: 4,
            total_words: 62400,
            warning: None,
        };
        let tokens = TokenCost {
            input: 1200,
            output: 300,
        };
        (
            graph,
            communities,
            cohesion,
            labels,
            gods,
            surprises,
            detection,
            tokens,
        )
    }

    fn run_generate(min_community_size: usize) -> String {
        let (graph, communities, cohesion, labels, gods, surprises, detection, tokens) =
            test_inputs();
        generate(
            &graph,
            &communities,
            &cohesion,
            &labels,
            &gods,
            &surprises,
            &detection,
            &tokens,
            "./project",
            None,
            min_community_size,
            None,
        )
    }

    #[test]
    fn test_report_contains_header() {
        let report = run_generate(3);
        assert!(
            report.contains("# Graph Report"),
            "header missing: {report}"
        );
    }

    #[test]
    fn test_report_contains_corpus_check() {
        let report = run_generate(3);
        assert!(
            report.contains("## Corpus Check"),
            "missing corpus check: {report}"
        );
    }

    #[test]
    fn test_report_contains_god_nodes() {
        let report = run_generate(3);
        assert!(
            report.contains("## God Nodes"),
            "missing god nodes: {report}"
        );
    }

    #[test]
    fn test_report_contains_surprising_connections() {
        let report = run_generate(3);
        assert!(
            report.contains("## Surprising Connections"),
            "missing surprising connections: {report}"
        );
    }

    #[test]
    fn test_report_contains_communities() {
        let report = run_generate(3);
        assert!(
            report.contains("## Communities"),
            "missing communities: {report}"
        );
    }

    #[test]
    fn test_report_contains_ambiguous_section() {
        let report = run_generate(3);
        assert!(
            report.contains("## Ambiguous Edges"),
            "missing ambiguous edges section: {report}"
        );
    }

    #[test]
    fn test_report_shows_token_cost() {
        let report = run_generate(3);
        assert!(
            report.contains("Token cost"),
            "token cost label missing: {report}"
        );
        assert!(report.contains("1,200"), "1,200 missing: {report}");
    }

    #[test]
    fn test_report_shows_raw_cohesion_scores() {
        let report = run_generate(1);
        assert!(report.contains("Cohesion:"), "cohesion missing: {report}");
        assert!(!report.contains('✓'), "unexpected ✓: {report}");
        assert!(!report.contains('⚠'), "unexpected ⚠: {report}");
    }
}
