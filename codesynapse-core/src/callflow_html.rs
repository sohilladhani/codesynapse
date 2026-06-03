use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::error::{CodeSynapseError, Result};
use crate::security::{check_file_size, MAX_GRAPH_FILE_BYTES};

// ─── Data types ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct CfNode {
    pub id: String,
    pub label: String,
    pub source_file: String,
    pub community: String,
    pub node_type: String,
    pub file_type: String,
}

#[derive(Debug, Clone)]
pub struct CfEdge {
    pub id: String,
    pub source: String,
    pub target: String,
    pub relation: String,
    pub confidence: String,
    pub confidence_score: f64,
}

#[derive(Debug, Clone)]
pub struct CallflowSection {
    pub id: String,
    pub name: String,
    pub communities: Vec<String>,
}

// ─── Section archetypes ───────────────────────────────────────────────────────

struct Archetype {
    id: &'static str,
    en_name: &'static str,
    keywords: &'static [&'static str],
}

static ARCHETYPES: &[Archetype] = &[
    Archetype {
        id: "extract-pipeline",
        en_name: "Extraction Pipeline",
        keywords: &[
            "extract",
            "extractor",
            "tree",
            "sitter",
            "parser",
            "language",
            "python",
            "javascript",
            "typescript",
            "rust",
            "java",
            "go",
            "ast",
            "calls",
            "imports",
            "multilang",
        ],
    },
    Archetype {
        id: "build-graph",
        en_name: "Graph Build",
        keywords: &[
            "build",
            "graph",
            "merge",
            "dedup",
            "node",
            "edge",
            "hyperedge",
            "json",
            "schema",
            "normalize",
            "confidence",
        ],
    },
    Archetype {
        id: "analysis-clustering",
        en_name: "Analysis & Clustering",
        keywords: &[
            "cluster",
            "community",
            "leiden",
            "cohesion",
            "analyze",
            "god",
            "surprise",
            "question",
            "query",
            "path",
            "explain",
            "benchmark",
        ],
    },
    Archetype {
        id: "outputs-docs",
        en_name: "Outputs & Docs",
        keywords: &[
            "export",
            "html",
            "wiki",
            "obsidian",
            "canvas",
            "svg",
            "graphml",
            "report",
            "callflow",
            "mermaid",
            "tree",
            "documentation",
        ],
    },
    Archetype {
        id: "cli-skills",
        en_name: "CLI & Skill Installers",
        keywords: &[
            "main",
            "install",
            "uninstall",
            "skill",
            "agent",
            "claude",
            "codex",
            "opencode",
            "aider",
            "copilot",
            "kiro",
            "vscode",
            "hook",
            "command",
        ],
    },
    Archetype {
        id: "ingest-cache-update",
        en_name: "Ingestion & Updates",
        keywords: &[
            "ingest",
            "fetch",
            "download",
            "url",
            "markdown",
            "cache",
            "manifest",
            "watch",
            "update",
            "incremental",
            "transcribe",
            "video",
            "audio",
            "google",
        ],
    },
    Archetype {
        id: "serve-api",
        en_name: "Serving API",
        keywords: &[
            "serve", "api", "request", "response", "endpoint", "router", "handle", "upload",
            "search", "delete", "enrich",
        ],
    },
    Archetype {
        id: "security-global",
        en_name: "Security & Global Graph",
        keywords: &[
            "security",
            "safe",
            "ssrf",
            "xss",
            "path",
            "traversal",
            "global",
            "prefix",
            "prune",
            "repo",
            "clone",
        ],
    },
    Archetype {
        id: "tests-fixtures",
        en_name: "Tests & Fixtures",
        keywords: &[
            "test", "tests", "fixture", "fixtures", "sample", "assert", "pytest", "mock",
        ],
    },
];

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn html_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

pub fn safe_mermaid_text(text: &str) -> String {
    let mut s = text.to_string();
    s = s.replace('"', "'");
    s = s.replace('`', "");
    s = s.replace('#', "");
    s = s.replace('|', " ");
    s = s.replace(['{', '}'], "");
    s = s
        .replace("->>", " to ")
        .replace("-->", " to ")
        .replace("->", " to ");
    let s: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    html_escape(&s)
}

pub fn stable_ascii_id(raw: &str, prefix: &str, limit: usize) -> String {
    let mut hasher = Sha256::new();
    hasher.update(raw.as_bytes());
    let digest = hasher.finalize();
    let hex = format!("{:x}", digest);
    let short = &hex[..8];

    let slug: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let slug = slug.trim_matches('_');
    let slug = if slug.is_empty() {
        prefix.to_string()
    } else {
        slug.to_string()
    };
    let slug = if slug
        .chars()
        .next()
        .map(|c| c.is_ascii_digit())
        .unwrap_or(false)
    {
        format!("{}_{}", prefix, slug)
    } else {
        slug
    };
    let slug = &slug[..slug.len().min(limit)];
    let slug = slug.trim_end_matches('_');
    format!("{}_{}", slug, short)
}

pub fn node_mermaid_id(node: &CfNode) -> String {
    stable_ascii_id(&node.id, "node", 48)
}

pub fn mermaid_section_id(section_id: &str) -> String {
    stable_ascii_id(section_id, "section", 48).to_uppercase()
}

pub fn safe_file_path(path: &str) -> String {
    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() > 3 {
        parts[parts.len() - 3..].join("/")
    } else {
        path.to_string()
    }
}

pub fn truncate_text(text: &str, limit: usize) -> String {
    let s: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.len() <= limit {
        s
    } else {
        let end = limit.saturating_sub(3);
        format!("{}...", s[..end].trim_end())
    }
}

pub fn humanize_label(label: &str, source_file: &str) -> String {
    let label = label.trim();
    if label.is_empty() {
        return Path::new(source_file)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Unknown")
            .to_string();
    }
    if label.starts_with('.') && label.ends_with("()") {
        return label[1..].to_string();
    }
    if label.ends_with(".py")
        || label.ends_with(".ts")
        || label.ends_with(".tsx")
        || label.ends_with(".js")
        || label.ends_with(".jsx")
        || label.ends_with(".go")
        || label.ends_with(".rs")
        || label.ends_with(".java")
        || label.ends_with(".rb")
    {
        return Path::new(label)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(label)
            .to_string();
    }
    if label.contains('_') && !label.contains(' ') && label.len() > 28 {
        let parts: Vec<&str> = label.split('_').filter(|p| !p.is_empty()).collect();
        if !parts.is_empty() {
            let tail: Vec<&str> = if parts.len() > 3 {
                parts[parts.len() - 3..].to_vec()
            } else {
                parts.clone()
            };
            return truncate_text(&tail.join(" "), 42);
        }
    }
    truncate_text(label, 42)
}

pub fn node_kind(node: &CfNode) -> &'static str {
    let label = node.label.to_lowercase();
    let source_file = node.source_file.to_lowercase();
    let file_type = node.file_type.to_lowercase();
    let node_type = node.node_type.to_lowercase();

    if matches!(
        node_type.as_str(),
        "class" | "klass" | "struct" | "interface" | "enum" | "trait" | "model"
    ) {
        return "klass";
    }
    if matches!(
        node_type.as_str(),
        "module" | "file" | "package" | "namespace"
    ) {
        return "module";
    }
    if matches!(
        node_type.as_str(),
        "endpoint" | "route" | "api" | "handler" | "controller"
    ) {
        return "api";
    }
    if matches!(node_type.as_str(), "test" | "spec") {
        return "test";
    }
    if matches!(node_type.as_str(), "component" | "hook" | "view" | "page") {
        return "ui";
    }
    if matches!(file_type.as_str(), "rationale" | "document") {
        return "concept";
    }
    if source_file.contains("test") || label.starts_with("test_") || source_file.contains("spec") {
        return "test";
    }
    if label.contains("endpoint")
        || label.contains("router")
        || label.contains("api")
        || label.contains("route")
    {
        return "api";
    }
    if label.contains("cli")
        || label.contains("command")
        || label.contains("click")
        || label.contains("typer")
    {
        return "entry";
    }
    if label.contains("async")
        || label.contains("await")
        || label.contains("stream")
        || label.contains("sse")
    {
        return "async";
    }
    let raw_label = &node.label;
    if (raw_label.starts_with("use")
        && raw_label.len() > 3
        && (raw_label
            .chars()
            .nth(3)
            .map(|c| c.is_uppercase())
            .unwrap_or(false)
            || raw_label
                .chars()
                .nth(3)
                .map(|c| c == '_' || c == '-')
                .unwrap_or(false)))
        || label.contains("component")
        || label.contains("props")
        || label.contains("hook")
        || label.contains("store")
        || source_file.ends_with(".tsx")
        || source_file.ends_with(".jsx")
        || source_file.ends_with(".vue")
        || source_file.ends_with(".svelte")
    {
        return "ui";
    }
    if raw_label
        .chars()
        .next()
        .map(|c| c.is_uppercase())
        .unwrap_or(false)
        && !raw_label.ends_with("()")
    {
        return "klass";
    }
    if raw_label.ends_with(".py")
        || raw_label.ends_with(".ts")
        || raw_label.ends_with(".tsx")
        || raw_label.ends_with(".js")
        || raw_label.ends_with(".jsx")
        || raw_label.ends_with(".go")
        || raw_label.ends_with(".rs")
        || raw_label.ends_with(".java")
        || raw_label.ends_with(".kt")
        || raw_label.ends_with(".rb")
        || raw_label.ends_with(".php")
        || raw_label.ends_with(".cs")
        || raw_label.ends_with(".swift")
        || raw_label.ends_with(".vue")
        || raw_label.ends_with(".svelte")
    {
        return "module";
    }
    "function"
}

fn mermaid_class_defs() -> Vec<&'static str> {
    vec![
        "    classDef entry fill:#422006,stroke:#fbbf24,color:#fde68a,stroke-width:1px;",
        "    classDef api fill:#450a0a,stroke:#f87171,color:#fee2e2,stroke-width:1px;",
        "    classDef async fill:#2e1065,stroke:#a78bfa,color:#ede9fe,stroke-width:1px;",
        "    classDef klass fill:#064e3b,stroke:#34d399,color:#d1fae5,stroke-width:1px;",
        "    classDef ui fill:#831843,stroke:#f472b6,color:#fce7f3,stroke-width:1px;",
        "    classDef module fill:#172554,stroke:#60a5fa,color:#dbeafe,stroke-width:1px;",
        "    classDef test fill:#3f3f46,stroke:#a1a1aa,color:#f4f4f5,stroke-width:1px;",
        "    classDef concept fill:#292524,stroke:#a8a29e,color:#fafaf9,stroke-dasharray:4 3;",
        "    classDef function fill:#0f172a,stroke:#38bdf8,color:#e0f2fe,stroke-width:1px;",
    ]
}

fn mermaid_init(scale: f64, direction: &str) -> String {
    let scale = scale.clamp(0.65, 1.8);
    let font_size = (15.0 * scale * 10.0).round() / 10.0;
    let node_spacing = (48.0 * scale).round() as i64;
    let rank_spacing = (64.0 * scale).round() as i64;
    let padding = (14.0 * scale).round() as i64;
    let diagram_padding = (10.0 * scale).round() as i64;
    format!(
        "%%{{init: {{\"theme\":\"dark\",\"themeVariables\":{{\"fontSize\":\"{font_size}px\",\"primaryColor\":\"#1e293b\",\"primaryTextColor\":\"#e2e8f0\",\"primaryBorderColor\":\"#38bdf8\",\"secondaryColor\":\"#0f172a\",\"tertiaryColor\":\"#334155\",\"lineColor\":\"#64748b\",\"textColor\":\"#e2e8f0\"}},\"flowchart\":{{\"htmlLabels\":true,\"curve\":\"basis\",\"nodeSpacing\":{node_spacing},\"rankSpacing\":{rank_spacing},\"padding\":{padding},\"diagramPadding\":{diagram_padding},\"useMaxWidth\":true}}}}}}%%\nflowchart {direction}"
    )
}

// ─── Community text helpers ────────────────────────────────────────────────────

static LABEL_STOPWORDS: &[&str] = &[
    "the", "and", "for", "with", "from", "this", "that", "class", "function", "method", "file",
    "src", "lib", "core", "index", "main", "init", "py", "ts", "tsx", "js", "jsx", "go", "rs",
    "java", "html", "css",
];

fn section_keywords(nodes: &[&CfNode], limit: usize) -> Vec<String> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for node in nodes {
        let text = format!("{} {}", node.label, node.source_file)
            .replace('/', " ")
            .replace(['_', '-'], " ");
        for raw in text.split_whitespace() {
            let word: String = raw.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
            let word = word.to_lowercase();
            if word.len() < 3 || LABEL_STOPWORDS.contains(&word.as_str()) {
                continue;
            }
            *counts.entry(word).or_insert(0) += 1;
        }
    }
    let mut pairs: Vec<(String, usize)> = counts.into_iter().collect();
    pairs.sort_by(|a, b| b.1.cmp(&a.1).then(b.0.cmp(&a.0)));
    pairs.into_iter().take(limit).map(|(w, _)| w).collect()
}

fn community_text(nodes: &[&CfNode], label: &str) -> String {
    let mut parts = vec![label.to_string()];
    for node in nodes.iter().take(80) {
        parts.push(node.label.clone());
        parts.push(node.source_file.clone());
        parts.push(node.node_type.clone());
        parts.push(node.file_type.clone());
    }
    parts.join(" ").to_lowercase()
}

fn keyword_score(text: &str, keywords: &[&str]) -> usize {
    let tokens: Vec<String> = text
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(|t| t.to_lowercase())
        .collect();
    let mut score = 0usize;
    for kw in keywords {
        score += tokens.iter().filter(|t| t.as_str() == *kw).count();
    }
    score
}

fn label_for_community(cid: &str, labels: &HashMap<String, String>, nodes: &[&CfNode]) -> String {
    if let Some(lbl) = labels.get(cid) {
        if !lbl.is_empty() {
            return lbl.clone();
        }
    }
    let kws = section_keywords(nodes, 3);
    if !kws.is_empty() {
        kws.iter()
            .map(|w| {
                let mut c = w.chars();
                match c.next() {
                    None => String::new(),
                    Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    } else {
        format!("Community {cid}")
    }
}

// ─── Section derivation ───────────────────────────────────────────────────────

pub fn derive_sections_from_communities(
    nodes: &[CfNode],
    labels: &HashMap<String, String>,
    _lang: &str,
    max_sections: usize,
) -> Vec<CallflowSection> {
    let mut comm_idx: HashMap<String, Vec<&CfNode>> = HashMap::new();
    for node in nodes {
        comm_idx
            .entry(node.community.clone())
            .or_default()
            .push(node);
    }

    let mut sections = vec![CallflowSection {
        id: "overview".to_string(),
        name: "Architecture Overview".to_string(),
        communities: vec![],
    }];

    struct GroupEntry {
        id: String,
        name: String,
        communities: Vec<String>,
        node_count: usize,
        priority: usize,
    }

    let mut grouped: Vec<GroupEntry> = Vec::new();
    let mut grouped_idx: HashMap<String, usize> = HashMap::new();
    let mut unassigned: Vec<(String, Vec<String>)> = Vec::new();

    let mut sorted_communities: Vec<(&String, &Vec<&CfNode>)> = comm_idx.iter().collect();
    sorted_communities.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then(a.0.cmp(b.0)));

    for (cid, community_nodes) in &sorted_communities {
        let refs: Vec<&CfNode> = community_nodes.to_vec();
        let lbl = label_for_community(cid, labels, &refs);
        let text = community_text(&refs, &lbl);

        let mut best_priority = usize::MAX;
        let mut best_sid = String::new();
        let mut best_name = String::new();
        let mut best_score = 0usize;

        for (priority, archetype) in ARCHETYPES.iter().enumerate() {
            let score = keyword_score(&text, archetype.keywords);
            if score > best_score {
                best_score = score;
                best_priority = priority;
                best_sid = archetype.id.to_string();
                best_name = archetype.en_name.to_string();
            }
        }

        if best_score >= 2 {
            if let Some(&idx) = grouped_idx.get(&best_sid) {
                grouped[idx].communities.push((*cid).clone());
                grouped[idx].node_count += community_nodes.len();
            } else {
                let idx = grouped.len();
                grouped_idx.insert(best_sid.clone(), idx);
                grouped.push(GroupEntry {
                    id: best_sid,
                    name: best_name,
                    communities: vec![(*cid).clone()],
                    node_count: community_nodes.len(),
                    priority: best_priority,
                });
            }
        } else {
            let node_labels: Vec<String> = community_nodes
                .iter()
                .take(3)
                .map(|n| n.label.clone())
                .collect();
            unassigned.push(((*cid).clone(), node_labels));
        }
    }

    let cap = max_sections.max(1);
    grouped.sort_by(|a, b| {
        a.priority
            .cmp(&b.priority)
            .then(b.node_count.cmp(&a.node_count))
            .then(a.id.cmp(&b.id))
    });
    let selected_count = grouped.len().min(cap - 1);
    let overflow: Vec<String> = grouped[selected_count..]
        .iter()
        .flat_map(|g| g.communities.clone())
        .collect();

    for g in &grouped[..selected_count] {
        sections.push(CallflowSection {
            id: g.id.clone(),
            name: g.name.clone(),
            communities: g.communities.clone(),
        });
    }

    let remaining_slots = cap.saturating_sub(sections.len()).saturating_sub(1);
    for (cid, node_labels) in unassigned.iter().take(remaining_slots) {
        let name = if node_labels.is_empty() {
            format!("Community {cid}")
        } else {
            node_labels[0].clone()
        };
        sections.push(CallflowSection {
            id: cid.clone(),
            name,
            communities: vec![cid.clone()],
        });
    }

    let other_communities: Vec<String> = overflow
        .into_iter()
        .chain(
            unassigned
                .iter()
                .skip(remaining_slots)
                .map(|(cid, _)| cid.clone()),
        )
        .collect();
    if !other_communities.is_empty() {
        sections.push(CallflowSection {
            id: "other".to_string(),
            name: "Other".to_string(),
            communities: other_communities,
        });
    }

    sections
}

// ─── Graph loading ────────────────────────────────────────────────────────────

fn first_str<'a>(map: &'a serde_json::Map<String, Value>, keys: &[&str]) -> Option<&'a str> {
    for k in keys {
        if let Some(Value::String(s)) = map.get(*k) {
            if !s.is_empty() {
                return Some(s.as_str());
            }
        }
    }
    None
}

fn normalize_node(raw: &Value, index: usize) -> Option<CfNode> {
    let map = raw.as_object()?;
    let id = first_str(
        map,
        &[
            "id",
            "node_id",
            "key",
            "uid",
            "name",
            "qualified_name",
            "fqname",
            "symbol",
        ],
    )
    .map(|s| s.to_string())
    .unwrap_or_else(|| {
        // try integer id
        if let Some(v) = map.get("id") {
            return v.to_string().trim_matches('"').to_string();
        }
        format!("node_{}", index + 1)
    });
    let source_file = first_str(
        map,
        &[
            "source_file",
            "file",
            "file_path",
            "filepath",
            "path",
            "module_path",
            "defined_in",
        ],
    )
    .unwrap_or("")
    .to_string();
    let label = first_str(
        map,
        &[
            "label",
            "display_name",
            "title",
            "name",
            "qualified_name",
            "fqname",
            "symbol",
        ],
    )
    .unwrap_or(id.as_str())
    .to_string();
    let community = map
        .get("community")
        .map(|v| match v {
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            _ => "unknown".to_string(),
        })
        .unwrap_or_else(|| "unknown".to_string());
    let node_type = first_str(map, &["node_type", "kind", "type", "category"])
        .unwrap_or("")
        .to_string();
    let file_type = first_str(map, &["file_type", "content_type", "artifact_type"])
        .unwrap_or("code")
        .to_string();

    Some(CfNode {
        id,
        label,
        source_file,
        community,
        node_type,
        file_type,
    })
}

fn endpoint_id(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Object(m) => first_str(m, &["id", "node_id", "key", "name", "qualified_name"])
            .unwrap_or("")
            .to_string(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

fn normalize_edge(raw: &Value, index: usize) -> Option<CfEdge> {
    let map = raw.as_object()?;
    let source = map
        .get("source")
        .or_else(|| map.get("src"))
        .or_else(|| map.get("from"))
        .map(endpoint_id)
        .unwrap_or_default();
    let target = map
        .get("target")
        .or_else(|| map.get("dst"))
        .or_else(|| map.get("to"))
        .map(endpoint_id)
        .unwrap_or_default();
    if source.is_empty() || target.is_empty() {
        return None;
    }
    let relation = first_str(map, &["relation", "type", "kind", "label", "predicate"])
        .unwrap_or("relates")
        .to_string();
    let confidence = first_str(map, &["confidence", "evidence", "provenance"])
        .unwrap_or("EXTRACTED")
        .to_uppercase();
    let confidence_score = match map
        .get("confidence_score")
        .or_else(|| map.get("score"))
        .or_else(|| map.get("weight"))
    {
        Some(Value::Number(n)) => n.as_f64().unwrap_or(1.0),
        Some(Value::String(s)) => s.parse::<f64>().unwrap_or(1.0),
        _ => 1.0,
    };
    let id = first_str(map, &["id", "edge_id"])
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("edge_{}", index + 1));
    Some(CfEdge {
        id,
        source,
        target,
        relation,
        confidence,
        confidence_score,
    })
}

#[allow(clippy::type_complexity)]
pub fn load_graph(
    path: &Path,
    max_bytes: u64,
) -> Result<(Vec<CfNode>, Vec<CfEdge>, Vec<Value>, HashMap<String, Value>)> {
    check_file_size(path, max_bytes)?;
    let text = std::fs::read_to_string(path).map_err(CodeSynapseError::Io)?;
    let data: Value = serde_json::from_str(&text).map_err(CodeSynapseError::Serialization)?;
    let obj = data
        .as_object()
        .ok_or_else(|| CodeSynapseError::Validation("graph file must be a JSON object".into()))?;

    let raw_nodes = obj
        .get("nodes")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let raw_edges = obj
        .get("links")
        .and_then(|v| v.as_array())
        .or_else(|| obj.get("edges").and_then(|v| v.as_array()))
        .cloned()
        .unwrap_or_default();
    let hyperedges = obj
        .get("hyperedges")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let nodes: Vec<CfNode> = raw_nodes
        .iter()
        .enumerate()
        .filter_map(|(i, v)| normalize_node(v, i))
        .collect();
    let edges: Vec<CfEdge> = raw_edges
        .iter()
        .enumerate()
        .filter_map(|(i, v)| normalize_edge(v, i))
        .collect();

    let mut meta: HashMap<String, Value> = HashMap::new();
    if let Some(graph_block) = obj.get("graph").and_then(|v| v.as_object()) {
        for (k, v) in graph_block {
            meta.insert(k.clone(), v.clone());
        }
    }
    for key in &[
        "built_at_commit",
        "commit",
        "project_name",
        "repo",
        "language_breakdown",
    ] {
        if let Some(v) = obj.get(*key) {
            meta.entry(key.to_string()).or_insert_with(|| v.clone());
        }
    }

    Ok((nodes, edges, hyperedges, meta))
}

fn load_labels(path: &Path) -> HashMap<String, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(_) => return HashMap::new(),
    };
    let data: Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(_) => return HashMap::new(),
    };
    let mut result = HashMap::new();
    if let Some(obj) = data.as_object() {
        for (k, v) in obj {
            match v {
                Value::String(s) => {
                    result.insert(k.clone(), s.clone());
                }
                Value::Number(n) => {
                    result.insert(k.clone(), n.to_string());
                }
                _ => {}
            }
        }
    }
    result
}

fn load_report(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

fn infer_project_name(graph_path: &Path, meta: &HashMap<String, Value>) -> String {
    if let Some(Value::String(s)) = meta.get("project_name") {
        return s.clone();
    }
    if let Some(parent) = graph_path.parent() {
        if parent.file_name().and_then(|n| n.to_str()) == Some("codesynapse-out") {
            if let Some(pp) = parent.parent() {
                if let Some(name) = pp.file_name().and_then(|n| n.to_str()) {
                    return name.to_string();
                }
            }
        }
        if let Some(name) = parent.file_name().and_then(|n| n.to_str()) {
            return name.to_string();
        }
    }
    "Project".to_string()
}

// ─── Mermaid diagram generators ───────────────────────────────────────────────

fn edge_score(edge: &CfEdge) -> f64 {
    let mut score = edge.confidence_score;
    if edge.confidence == "EXTRACTED" {
        score += 2.0;
    }
    match edge.relation.as_str() {
        "calls" | "uses" | "method" => score += 1.0,
        "imports" | "imports_from" => score += 0.6,
        "contains" => score -= 0.2,
        "rationale_for" => score -= 0.6,
        _ => {}
    }
    score
}

fn node_label_mermaid(node: &CfNode) -> String {
    let label = humanize_label(&node.label, &node.source_file);
    let sf = safe_file_path(&node.source_file);
    let label_safe = safe_mermaid_text(&label);
    if !sf.is_empty()
        && !label.ends_with(
            Path::new(&sf)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(""),
        )
    {
        format!(
            "{}<br/><small>{}</small>",
            label_safe,
            safe_mermaid_text(&sf)
        )
    } else {
        label_safe
    }
}

fn generate_section_flowchart(
    section_id: &str,
    section_name: &str,
    nodes: &[&CfNode],
    edges: &[&CfEdge],
    diagram_scale: f64,
    max_nodes: usize,
    max_edges: usize,
) -> String {
    let mut lines = vec![mermaid_init(diagram_scale, "LR")];
    lines.push(format!(
        "    %% Section: {} ({} nodes, {} edges)",
        safe_mermaid_text(section_name),
        nodes.len(),
        edges.len()
    ));

    if nodes.is_empty() {
        lines.push(format!(
            "    empty(\"{} - no nodes\")",
            safe_mermaid_text(section_name)
        ));
        for def in mermaid_class_defs() {
            lines.push(def.to_string());
        }
        return lines.join("\n");
    }

    let selected_nodes: Vec<&CfNode> = nodes.iter().copied().take(max_nodes).collect();
    let selected_ids: std::collections::HashSet<&str> =
        selected_nodes.iter().map(|n| n.id.as_str()).collect();

    let vis_edges: Vec<&CfEdge> = edges
        .iter()
        .copied()
        .filter(|e| {
            selected_ids.contains(e.source.as_str()) && selected_ids.contains(e.target.as_str())
        })
        .collect();

    let mut class_lines: Vec<String> = Vec::new();
    for node in &selected_nodes {
        let mid = node_mermaid_id(node);
        let lbl = node_label_mermaid(node);
        lines.push(format!("    {}(\"{}\")", mid, lbl));
        class_lines.push(format!("    class {} {};", mid, node_kind(node)));
    }

    let mut sorted_vis: Vec<&CfEdge> = vis_edges.clone();
    sorted_vis.sort_by(|a, b| {
        edge_score(b)
            .partial_cmp(&edge_score(a))
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    for edge in sorted_vis.into_iter().take(max_edges) {
        let src_id = stable_ascii_id(&edge.source, "node", 48);
        let tgt_id = stable_ascii_id(&edge.target, "node", 48);
        let rel = safe_mermaid_text(&edge.relation.replace('_', " "));
        lines.push(format!("    {} -->|{}| {}", src_id, rel, tgt_id));
    }

    let _ = section_id;
    lines.extend(class_lines);
    for def in mermaid_class_defs() {
        lines.push(def.to_string());
    }
    lines.join("\n")
}

fn generate_overview_graph(
    sections: &[CallflowSection],
    section_nodes_map: &HashMap<String, Vec<usize>>,
    nodes: &[CfNode],
    edges: &[CfEdge],
    diagram_scale: f64,
) -> String {
    let mut lines = vec![mermaid_init(diagram_scale, "LR")];

    let node_section: HashMap<&str, &str> = {
        let mut m = HashMap::new();
        for sec in sections {
            if sec.id == "overview" {
                continue;
            }
            for &idx in section_nodes_map.get(&sec.id).unwrap_or(&vec![]) {
                m.insert(nodes[idx].id.as_str(), sec.id.as_str());
            }
        }
        m
    };

    for sec in sections {
        if sec.id == "overview" {
            continue;
        }
        let sid = mermaid_section_id(&sec.id);
        let node_count = section_nodes_map.get(&sec.id).map(|v| v.len()).unwrap_or(0);
        let lbl = format!(
            "{}<br/><small>{} nodes</small>",
            safe_mermaid_text(&sec.name),
            node_count
        );
        lines.push(format!("    {}(\"{}\")", sid, lbl));
        lines.push(format!("    class {} module;", sid));
    }

    let mut inter_counts: HashMap<(&str, &str), usize> = HashMap::new();
    let mut inter_rel: HashMap<(&str, &str), HashMap<String, usize>> = HashMap::new();
    for edge in edges {
        let src_sec = node_section.get(edge.source.as_str()).copied();
        let tgt_sec = node_section.get(edge.target.as_str()).copied();
        if let (Some(ss), Some(ts)) = (src_sec, tgt_sec) {
            if ss != ts {
                *inter_counts.entry((ss, ts)).or_insert(0) += 1;
                *inter_rel
                    .entry((ss, ts))
                    .or_default()
                    .entry(edge.relation.clone())
                    .or_insert(0) += 1;
            }
        }
    }

    let mut inter_vec: Vec<((&str, &str), usize)> = inter_counts.into_iter().collect();
    inter_vec.sort_by_key(|k| std::cmp::Reverse(k.1));
    for ((ss, ts), count) in inter_vec.iter().take(12) {
        let src_id = mermaid_section_id(ss);
        let tgt_id = mermaid_section_id(ts);
        let rels = inter_rel.get(&(*ss, *ts)).cloned().unwrap_or_default();
        let top_rel = rels
            .iter()
            .max_by_key(|(_, c)| *c)
            .map(|(r, _)| r.as_str())
            .unwrap_or("relates");
        let lbl = if *count > 1 {
            format!(
                "{} x{}",
                safe_mermaid_text(&top_rel.replace('_', " ")),
                count
            )
        } else {
            safe_mermaid_text(&top_rel.replace('_', " "))
        };
        lines.push(format!("    {} -->|{}| {}", src_id, lbl, tgt_id));
    }

    if inter_vec.is_empty() {
        let non_overview: Vec<&CallflowSection> =
            sections.iter().filter(|s| s.id != "overview").collect();
        for w in non_overview.windows(2) {
            lines.push(format!(
                "    {} -.-> {}",
                mermaid_section_id(&w[0].id),
                mermaid_section_id(&w[1].id)
            ));
        }
    }

    for def in mermaid_class_defs() {
        lines.push(def.to_string());
    }
    lines.join("\n")
}

// ─── HTML component generators ────────────────────────────────────────────────

fn report_highlights(report_text: &str) -> String {
    if report_text.trim().is_empty() {
        return String::new();
    }
    let mut keep: Vec<String> = Vec::new();
    let mut in_gods = false;
    let mut in_summary = false;
    for line in report_text.lines() {
        let stripped = line.trim();
        if stripped.starts_with("## ") {
            in_summary = stripped == "## Summary";
            in_gods = stripped.starts_with("## God Nodes");
            continue;
        }
        if in_summary && stripped.starts_with("- ") {
            keep.push(stripped[2..].to_string());
        } else if in_gods
            && stripped
                .chars()
                .next()
                .map(|c| c.is_ascii_digit())
                .unwrap_or(false)
        {
            keep.push(stripped.to_string());
        }
        if keep.len() >= 6 {
            break;
        }
    }
    if keep.is_empty() {
        return String::new();
    }
    let items: String = keep
        .iter()
        .map(|item| format!("      <li>{}</li>", html_escape(item)))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        r#"<div class="card">
    <h4>Graph Report Highlights</h4>
    <ul>
{items}
    </ul>
  </div>"#
    )
}

fn generate_nav(sections: &[CallflowSection]) -> String {
    let links: String = sections
        .iter()
        .map(|sec| {
            format!(
                "    <a href=\"#{}\">{}</a>",
                html_escape(&sec.id),
                html_escape(&sec.name)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!("<div class=\"nav\">\n{}\n</div>", links)
}

fn build_section_node_map(
    sections: &[CallflowSection],
    nodes: &[CfNode],
) -> HashMap<String, Vec<usize>> {
    let mut comm_idx: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, node) in nodes.iter().enumerate() {
        comm_idx.entry(node.community.clone()).or_default().push(i);
    }
    let mut result = HashMap::new();
    for sec in sections {
        let indices: Vec<usize> = sec
            .communities
            .iter()
            .flat_map(|cid| comm_idx.get(cid).cloned().unwrap_or_default())
            .collect();
        result.insert(sec.id.clone(), indices);
    }
    result
}

fn generate_overview_cards(
    sections: &[CallflowSection],
    section_nodes_map: &HashMap<String, Vec<usize>>,
) -> String {
    let rows: String = sections
        .iter()
        .filter(|s| s.id != "overview")
        .map(|sec| {
            let node_count = section_nodes_map.get(&sec.id).map(|v| v.len()).unwrap_or(0);
            let comms = sec
                .communities
                .iter()
                .map(|c| c.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "<tr><td>{}</td><td>{node_count}</td><td><code>{}</code></td></tr>",
                html_escape(&sec.name),
                html_escape(&comms)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        r#"<div class="grid">
  <div class="card">
    <h4>Architecture Layers</h4>
    <table style="width:100%;font-size:0.85rem;">
      <tr><th>Layer</th><th>Nodes</th><th>Communities</th></tr>
      {rows}
    </table>
  </div>
</div>"#
    )
}

fn generate_call_table_rows(nodes: &[&CfNode], _edges: &[&CfEdge]) -> String {
    let mut rows = Vec::new();
    for (i, n) in nodes.iter().take(30).enumerate() {
        let label = html_escape(&n.label);
        let sf = html_escape(&safe_file_path(&n.source_file));
        let kind = node_kind(n);
        let tag = match kind {
            "klass" => r#"<span class="tag tag-class">Class</span>"#,
            "api" => r#"<span class="tag tag-endpoint">API</span>"#,
            "async" => r#"<span class="tag tag-async">Async</span>"#,
            "entry" => r#"<span class="tag tag-cmd">Entry</span>"#,
            "test" => r#"<span class="tag tag-func">Test</span>"#,
            "ui" => r#"<span class="tag tag-hook">UI</span>"#,
            "module" => r#"<span class="tag tag-class">Module</span>"#,
            _ => r#"<span class="tag tag-func">Function</span>"#,
        };
        rows.push(format!(
            "<tr>\n  <td>{}</td>\n  <td><code>{label}</code><br><small style=\"color:var(--muted)\">{sf}</small></td>\n  <td>{tag}</td>\n  <td></td>\n  <td></td>\n  <td></td>\n</tr>",
            i + 1
        ));
    }
    rows.join("\n")
}

fn section_intro(sec: &CallflowSection, nodes: &[&CfNode], edge_count: usize) -> String {
    let kws = section_keywords(nodes, 4);
    let kw_text = if kws.is_empty() {
        sec.name.clone()
    } else {
        kws.join(", ")
    };
    let text = format!(
        "{} groups implementation around {kw_text}. This section covers {} nodes and {edge_count} internal edges; the diagram shows only representative relationships to stay readable.",
        sec.name, nodes.len()
    );
    format!("<p>{}</p>", html_escape(&text))
}

fn section_cards(sec: &CallflowSection, nodes: &[&CfNode], _edges: &[&CfEdge]) -> String {
    let mut file_counts: HashMap<&str, usize> = HashMap::new();
    for n in nodes {
        if !n.source_file.is_empty() {
            *file_counts.entry(n.source_file.as_str()).or_insert(0) += 1;
        }
    }
    let mut top_files: Vec<(&str, usize)> = file_counts.into_iter().collect();
    top_files.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    let file_rows: String = if top_files.is_empty() {
        r#"<tr><td colspan="2">No source file mapping</td></tr>"#.to_string()
    } else {
        top_files
            .iter()
            .take(8)
            .map(|(path, count)| {
                format!(
                    "<tr><td><code>{}</code></td><td>{count} nodes</td></tr>",
                    html_escape(&safe_file_path(path))
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    format!(
        r#"<div class="grid">
  <div class="card">
    <h4>Key Files</h4>
    <table style="width:100%;font-size:0.85rem;">
      <tr><th>File</th><th>Coverage</th></tr>
      {file_rows}
    </table>
  </div>
  <div class="card">
    <h4>Design Notes</h4>
    <p>This section comes from {} community clustering.</p>
  </div>
</div>"#,
        html_escape(&sec.name)
    )
}

// ─── Main entry point ─────────────────────────────────────────────────────────

static CSS: &str = r#":root {
  --bg: #0f172a; --surface: #1e293b; --border: #334155;
  --text: #e2e8f0; --muted: #94a3b8; --accent: #38bdf8;
  --warn: #fbbf24; --err: #f87171; --ok: #34d399;
}
* { box-sizing: border-box; margin: 0; padding: 0; }
body { font-family: 'Segoe UI', system-ui, -apple-system, sans-serif; background: var(--bg); color: var(--text); line-height: 1.7; }
.container { max-width: 1200px; margin: 0 auto; padding: 40px 24px; }
h1 { font-size: 2.4rem; margin-bottom: 8px; }
h2 { font-size: 1.7rem; margin: 48px 0 16px; padding-bottom: 8px; border-bottom: 2px solid var(--accent); }
h3 { font-size: 1.25rem; margin: 32px 0 12px; color: var(--accent); }
h4 { font-size: 1.05rem; margin: 20px 0 8px; color: var(--warn); }
p { margin: 8px 0; color: var(--muted); }
.subtitle { color: var(--muted); font-size: 1.1rem; margin-bottom: 32px; }
.mermaid { background: var(--surface); border: 1px solid var(--border); border-radius: 12px; padding: 24px; margin: 20px 0; overflow-x: auto; }
.call-table { width: 100%; border-collapse: collapse; margin: 16px 0; font-size: 0.92rem; }
.call-table th { background: #1a2744; color: var(--accent); text-align: left; padding: 10px 14px; border: 1px solid var(--border); }
.call-table td { padding: 8px 14px; border: 1px solid var(--border); vertical-align: top; }
.tag { display: inline-block; padding: 2px 8px; border-radius: 4px; font-size: 0.8rem; font-weight: 600; }
.tag-async { background: #7c3aed33; color: #a78bfa; }
.tag-class { background: #05966933; color: var(--ok); }
.tag-func { background: #2563eb33; color: var(--accent); }
.tag-cmd { background: #d9770633; color: var(--warn); }
.tag-endpoint { background: #dc262633; color: var(--err); }
.tag-hook { background: #db277733; color: #f472b6; }
.card { background: var(--surface); border: 1px solid var(--border); border-radius: 10px; padding: 20px; margin: 16px 0; }
.grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(340px, 1fr)); gap: 16px; margin: 16px 0; }
code { font-family: monospace; background: rgba(255,255,255,0.06); padding: 1px 6px; border-radius: 3px; font-size: 0.88em; }
ul, ol { margin: 8px 0 8px 24px; color: var(--muted); }
li { margin: 4px 0; }
a { color: var(--accent); }
hr { border: none; border-top: 1px solid var(--border); margin: 40px 0; }
.nav { position: sticky; top: 0; background: var(--bg); z-index: 10; padding: 12px 0; border-bottom: 1px solid var(--border); display: flex; gap: 20px; flex-wrap: wrap; font-size: 0.9rem; }
.nav a { text-decoration: none; }
"#;

static MERMAID_JS: &str = r#"<script>
(function () {
  mermaid.initialize({
    startOnLoad: false,
    theme: 'dark',
    securityLevel: 'loose',
    flowchart: { htmlLabels: true, useMaxWidth: true },
    themeVariables: {
      primaryColor: '#1e293b',
      primaryTextColor: '#e2e8f0',
      primaryBorderColor: '#38bdf8',
      secondaryColor: '#0f172a',
      tertiaryColor: '#334155',
      lineColor: '#64748b',
      textColor: '#e2e8f0',
    }
  });
  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', () => mermaid.run({ querySelector: '.mermaid' }));
  } else {
    mermaid.run({ querySelector: '.mermaid' });
  }
})();
</script>"#;

#[allow(clippy::too_many_arguments)]
pub fn write_callflow_html(
    project: Option<&Path>,
    codesynapse_out_arg: Option<&Path>,
    graph_arg: Option<&Path>,
    report_arg: Option<&Path>,
    labels_arg: Option<&Path>,
    sections_file: Option<&Path>,
    output_arg: Option<&Path>,
    _lang: &str,
    max_sections: usize,
    diagram_scale: f64,
    max_diagram_nodes: usize,
    max_diagram_edges: usize,
    verbose: bool,
) -> Result<PathBuf> {
    let base = project
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

    let codesynapse_out = if let Some(p) = codesynapse_out_arg {
        p.to_path_buf()
    } else if let Some(g) = graph_arg {
        g.parent().unwrap_or(Path::new(".")).to_path_buf()
    } else if base.join("graph.json").exists() {
        base.clone()
    } else {
        base.join("codesynapse-out")
    };

    let graph_path = graph_arg
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| codesynapse_out.join("graph.json"));
    let report_path = report_arg
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| codesynapse_out.join("GRAPH_REPORT.md"));
    let labels_path = labels_arg
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| codesynapse_out.join(".codesynapse_labels.json"));

    if !graph_path.exists() {
        return Err(CodeSynapseError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("graph file not found: {}", graph_path.display()),
        )));
    }

    let (nodes, edges, hyperedges, mut meta) = load_graph(&graph_path, MAX_GRAPH_FILE_BYTES)?;
    let labels = load_labels(&labels_path);
    let report_text = load_report(&report_path);

    if nodes.is_empty() {
        return Err(CodeSynapseError::Validation(
            "graph.json contains 0 nodes".into(),
        ));
    }

    let sections = derive_sections_from_communities(&nodes, &labels, "en", max_sections);
    if sections.len() <= 1 {
        return Err(CodeSynapseError::Validation("no sections defined".into()));
    }

    let _ = sections_file;

    let project_name = infer_project_name(&graph_path, &meta);
    meta.insert("project_name".into(), Value::String(project_name.clone()));
    meta.insert("node_count".into(), Value::Number(nodes.len().into()));
    meta.insert("edge_count".into(), Value::Number(edges.len().into()));
    meta.insert(
        "hyperedge_count".into(),
        Value::Number(hyperedges.len().into()),
    );

    let commit = meta
        .get("built_at_commit")
        .and_then(|v| v.as_str())
        .map(|s| s.chars().take(7).collect::<String>())
        .unwrap_or_else(|| "unknown".to_string());

    let output_path = if let Some(o) = output_arg {
        if o.is_absolute() {
            o.to_path_buf()
        } else {
            base.join(o)
        }
    } else {
        let stem = project_name
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' {
                    c
                } else {
                    '-'
                }
            })
            .collect::<String>();
        let stem = stem.trim_matches('-').to_string();
        let stem = if stem.is_empty() {
            "project".to_string()
        } else {
            stem
        };
        codesynapse_out.join(format!("{stem}-callflow.html"))
    };

    if verbose {
        eprintln!(
            "Loaded: {} nodes, {} edges, {} sections",
            nodes.len(),
            edges.len(),
            sections.len()
        );
    }

    let section_nodes_map = build_section_node_map(&sections, &nodes);

    let mut comm_idx: HashMap<String, usize> = HashMap::new();
    for node in &nodes {
        comm_idx.entry(node.community.clone()).or_insert(0);
        *comm_idx.get_mut(&node.community).unwrap() += 1;
    }

    let doc_title = format!(
        "{} — Complete Call Flow & Architecture Documentation",
        project_name
    );
    let subtitle = format!(
        "Generated from codesynapse knowledge graph: {} nodes, {} edges, {} communities. Commit: {}",
        nodes.len(), edges.len(), comm_idx.len(), commit
    );

    let mut html = Vec::new();

    html.push(format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>{}</title>
<script src="https://cdn.jsdelivr.net/npm/mermaid@11/dist/mermaid.min.js"></script>
<style>
{}
</style>
</head>
<body>
<div class="container">
"#,
        html_escape(&doc_title),
        CSS
    ));

    // Header
    html.push(format!(
        "<h1>{}</h1>\n<p class=\"subtitle\">{}</p>\n\n{}",
        html_escape(&doc_title),
        html_escape(&subtitle),
        generate_nav(&sections)
    ));

    // Overview section
    let overview_name = sections
        .first()
        .map(|s| s.name.as_str())
        .unwrap_or("Architecture Overview");
    html.push(format!(
        "\n<!-- ====== Architecture Overview ====== -->\n<h2 id=\"overview\">1. {}</h2>\n",
        html_escape(overview_name)
    ));
    html.push(format!(
        "<div class=\"mermaid\">\n{}\n</div>\n",
        generate_overview_graph(&sections, &section_nodes_map, &nodes, &edges, diagram_scale)
    ));
    html.push(generate_overview_cards(&sections, &section_nodes_map));
    let rh = report_highlights(&report_text);
    if !rh.is_empty() {
        html.push(format!("<div class=\"grid\">\n  {}\n</div>", rh));
    }
    html.push("<hr>".to_string());

    // Per-section content
    let mut section_num = 1usize;
    for sec in &sections {
        if sec.id == "overview" {
            continue;
        }
        section_num += 1;
        let sec_node_indices = section_nodes_map.get(&sec.id).cloned().unwrap_or_default();
        let sec_nodes: Vec<&CfNode> = sec_node_indices.iter().map(|&i| &nodes[i]).collect();
        let sec_node_ids: std::collections::HashSet<&str> =
            sec_nodes.iter().map(|n| n.id.as_str()).collect();
        let sec_edges: Vec<&CfEdge> = edges
            .iter()
            .filter(|e| {
                sec_node_ids.contains(e.source.as_str()) && sec_node_ids.contains(e.target.as_str())
            })
            .collect();

        html.push(format!(
            "<!-- ====== {}. {} ====== -->\n<h2 id=\"{}\">{section_num}. {}</h2>\n",
            section_num,
            sec.name,
            html_escape(&sec.id),
            html_escape(&sec.name)
        ));
        html.push(section_intro(sec, &sec_nodes, sec_edges.len()));
        html.push(format!(
            "\n<div class=\"mermaid\">\n{}\n</div>\n",
            generate_section_flowchart(
                &sec.id,
                &sec.name,
                &sec_nodes,
                &sec_edges,
                diagram_scale,
                max_diagram_nodes,
                max_diagram_edges
            )
        ));
        html.push(format!(
            "<h3>Call Details</h3>\n<table class=\"call-table\">\n<tr>\n  <th style=\"width:5%\">#</th>\n  <th style=\"width:28%\">Node</th>\n  <th style=\"width:10%\">Type</th>\n  <th style=\"width:17%\">Caller</th>\n  <th style=\"width:20%\">Callees</th>\n  <th style=\"width:20%\">Description</th>\n</tr>\n{}\n</table>\n",
            generate_call_table_rows(&sec_nodes, &sec_edges)
        ));
        html.push(section_cards(sec, &sec_nodes, &sec_edges));
        html.push("<hr>".to_string());
    }

    // Hyperedges
    if !hyperedges.is_empty() {
        html.push(
            "<h2 id=\"hyperedges\">Group Relationships (Hyperedges)</h2>\n<div class=\"grid\">"
                .to_string(),
        );
        for he in hyperedges.iter().take(9) {
            if let Some(obj) = he.as_object() {
                let hid = obj.get("id").and_then(|v| v.as_str()).unwrap_or("?");
                let hlabel = obj.get("label").and_then(|v| v.as_str()).unwrap_or(hid);
                let hnodes = obj
                    .get("nodes")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();
                let hrel = obj.get("relation").and_then(|v| v.as_str()).unwrap_or("");
                html.push(format!(
                    "  <div class=\"card\"><h4>{}</h4><p><code>{}</code> — {} participants</p><ul>",
                    html_escape(hlabel),
                    html_escape(hrel),
                    hnodes.len()
                ));
                for hn in hnodes.iter().take(5) {
                    html.push(format!(
                        "      <li><code>{}</code></li>",
                        html_escape(&hn.to_string())
                    ));
                }
                if hnodes.len() > 5 {
                    html.push(format!("      <li>... and {} more</li>", hnodes.len() - 5));
                }
                html.push("    </ul>\n  </div>".to_string());
            }
        }
        html.push("</div>\n<hr>".to_string());
    }

    // Stats
    html.push(format!(
        r#"<h2 id="stats">Project Statistics</h2>
<div class="grid">
  <div class="card">
    <h4>Graph</h4>
    <table style="width:100%;font-size:0.85rem;">
      <tr><td>Nodes</td><td>{}</td></tr>
      <tr><td>Edges</td><td>{}</td></tr>
      <tr><td>Hyperedges</td><td>{}</td></tr>
      <tr><td>Communities</td><td>{}</td></tr>
    </table>
  </div>
</div>
"#,
        nodes.len(),
        edges.len(),
        hyperedges.len(),
        comm_idx.len()
    ));

    // Footer
    html.push(format!(
        "<div style=\"text-align:center; padding:40px 0; color: var(--muted); font-size:0.9rem;\">\n  <p>{} — Architecture Documentation</p>\n  <p>Generated by codesynapse callflow-html</p>\n</div>\n",
        html_escape(&project_name)
    ));

    html.push("</div><!-- .container -->\n".to_string());
    html.push(MERMAID_JS.to_string());
    html.push("\n</body>\n</html>".to_string());

    let content = html.join("\n");

    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent).map_err(CodeSynapseError::Io)?;
    }
    std::fs::write(&output_path, &content).map_err(CodeSynapseError::Io)?;

    if verbose {
        println!("callflow HTML written: {}", output_path.display());
    }

    Ok(output_path)
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use tempfile::tempdir;

    fn make_codesynapse_out(dir: &Path) -> PathBuf {
        let out = dir.join("codesynapse-out");
        std::fs::create_dir_all(&out).unwrap();

        let graph = serde_json::json!({
            "directed": false,
            "multigraph": false,
            "graph": {},
            "nodes": [
                {"id": "api", "label": "ApiClient", "source_file": "src/api.py", "file_type": "code", "community": 0},
                {"id": "run", "label": "run()", "source_file": "src/main.py", "file_type": "code", "community": 0},
                {"id": "export", "label": "write_html()", "source_file": "src/export.py", "file_type": "code", "community": 1},
                {"id": "evil", "label": "<script>alert(1)</script>", "source_file": "src/evil.py", "file_type": "code", "community": 1}
            ],
            "links": [
                {"source": "run", "target": "api", "relation": "calls", "confidence": "EXTRACTED", "confidence_score": 1.0},
                {"source": "api", "target": "export", "relation": "uses", "confidence": "EXTRACTED", "confidence_score": 1.0},
                {"source": "export", "target": "evil", "relation": "calls", "confidence": "EXTRACTED", "confidence_score": 1.0}
            ],
            "hyperedges": [],
            "built_at_commit": "abcdef123456"
        });
        std::fs::write(
            out.join("graph.json"),
            serde_json::to_string(&graph).unwrap(),
        )
        .unwrap();

        std::fs::write(
            out.join(".codesynapse_labels.json"),
            r#"{"0": "Runtime", "1": "Export"}"#,
        )
        .unwrap();

        let report = "# Graph Report - sample\n\n## Summary\n- 3 nodes · 2 edges · 1 communities detected\n\n## God Nodes (most connected - your core abstractions)\n1. `Transformer` - 2 edges\n";
        std::fs::write(out.join("GRAPH_REPORT.md"), report).unwrap();

        out
    }

    #[test]
    fn test_write_callflow_html_creates_file_and_uses_report() {
        let dir = tempdir().unwrap();
        let _out = make_codesynapse_out(dir.path());

        let html_path = write_callflow_html(
            Some(dir.path()),
            None,
            None,
            None,
            None,
            None,
            Some(Path::new("codesynapse-out/callflow.html")),
            "en",
            4,
            1.0,
            18,
            24,
            false,
        )
        .unwrap();

        assert_eq!(html_path, dir.path().join("codesynapse-out/callflow.html"));
        let content = std::fs::read_to_string(&html_path).unwrap();
        assert!(content.contains("mermaid"), "should contain mermaid");
        assert!(
            content.contains("Graph Report Highlights"),
            "should contain report highlights heading"
        );
        assert!(
            content.contains("Transformer"),
            "should contain god node from report"
        );
        assert!(content.contains("ApiClient"), "should contain node label");
        assert!(
            content.contains("&lt;script&gt;alert(1)&lt;/script&gt;"),
            "XSS should be escaped"
        );
        assert!(
            !content.contains("<script>alert(1)</script>"),
            "raw XSS must not appear"
        );
    }

    #[test]
    fn test_load_graph_rejects_oversized_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("graph.json");
        std::fs::write(&path, r#"{"nodes": [], "links": []}"#).unwrap();
        let result = load_graph(&path, 8);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("exceeds"),
            "error should mention 'exceeds': {msg}"
        );
    }

    #[test]
    fn test_derive_sections_groups_by_architecture_keywords() {
        let nodes = vec![
            CfNode {
                id: "extract_py".into(),
                label: "extract_python".into(),
                source_file: "codesynapse/extract.py".into(),
                community: "0".into(),
                node_type: String::new(),
                file_type: "code".into(),
            },
            CfNode {
                id: "extract_js".into(),
                label: "extract_js".into(),
                source_file: "codesynapse/extract.py".into(),
                community: "0".into(),
                node_type: String::new(),
                file_type: "code".into(),
            },
            CfNode {
                id: "to_html".into(),
                label: "to_html".into(),
                source_file: "codesynapse/export.py".into(),
                community: "1".into(),
                node_type: String::new(),
                file_type: "code".into(),
            },
            CfNode {
                id: "test_html".into(),
                label: "test_export_html".into(),
                source_file: "tests/test_export.py".into(),
                community: "2".into(),
                node_type: String::new(),
                file_type: "code".into(),
            },
        ];

        let sections = derive_sections_from_communities(&nodes, &HashMap::new(), "en", 6);
        let ids: std::collections::HashSet<&str> = sections.iter().map(|s| s.id.as_str()).collect();

        assert!(
            ids.contains("extract-pipeline"),
            "should contain extract-pipeline, got: {:?}",
            ids
        );
        assert!(
            ids.contains("outputs-docs"),
            "should contain outputs-docs, got: {:?}",
            ids
        );
        assert!(
            ids.contains("tests-fixtures"),
            "should contain tests-fixtures, got: {:?}",
            ids
        );
    }

    #[test]
    fn test_safe_mermaid_text_escapes_html() {
        let result = safe_mermaid_text("<script>alert(1)</script>");
        assert!(!result.contains('<'));
        assert!(!result.contains('>'));
        assert!(result.contains("&lt;") || result.contains("script"));
    }

    #[test]
    fn test_stable_ascii_id_no_collision() {
        let id1 = stable_ascii_id("foo", "node", 48);
        let id2 = stable_ascii_id("bar", "node", 48);
        assert_ne!(id1, id2);
        assert!(id1.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'));
    }

    #[test]
    fn test_humanize_label_strips_leading_dot() {
        assert_eq!(humanize_label(".foo()", ""), "foo()");
    }

    #[test]
    fn test_load_graph_parses_nodes_and_edges() {
        let dir = tempdir().unwrap();
        let _out = make_codesynapse_out(dir.path());
        let graph_path = dir.path().join("codesynapse-out/graph.json");
        let (nodes, edges, _, _) = load_graph(&graph_path, MAX_GRAPH_FILE_BYTES).unwrap();
        assert_eq!(nodes.len(), 4);
        assert_eq!(edges.len(), 3);
    }

    #[test]
    fn test_keyword_score_word_boundary() {
        let text = "test export html test_export_html tests/test_export.py  code";
        let score = keyword_score(text, &["test", "tests"]);
        assert!(score >= 4, "expected >= 4, got {score}");
    }

    #[test]
    fn test_section_keywords_excludes_stopwords() {
        let nodes = [CfNode {
            id: "n1".into(),
            label: "to_html".into(),
            source_file: "export.py".into(),
            community: "0".into(),
            node_type: String::new(),
            file_type: "code".into(),
        }];
        let refs: Vec<&CfNode> = nodes.iter().collect();
        let kws = section_keywords(&refs, 5);
        assert!(!kws.contains(&"html".to_string()), "html is a stopword");
    }
}
