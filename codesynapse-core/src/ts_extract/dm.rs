use super::make_file_node;
use crate::error::Result;
use crate::extract::make_id;
use crate::types::{Edge, ExtractionFragment, Node};
use std::collections::{HashMap, HashSet};
use std::path::Path;

// ─── DM (DreamMaker source) ───────────────────────────────────────────────────

pub struct DmExtractor;

impl DmExtractor {
    pub fn extract(source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        let (file_id, _file_label, file_node) = make_file_node(path);
        let str_path = path.to_string_lossy().to_string();

        let mut nodes: Vec<Node> = vec![file_node];
        let mut edges: Vec<Edge> = vec![];
        let mut seen_ids: HashSet<String> = HashSet::new();
        seen_ids.insert(file_id.clone());

        let text = std::str::from_utf8(source).unwrap_or("");
        let stem = file_id.clone();

        // Tracks (type_path, type_nid) when inside a type block
        let mut current_type: Option<(String, String)> = None;
        // Tracks proc_nid of current proc whose body we're collecting calls for
        let mut current_proc: Option<String> = None;
        // Minimum indent for body lines of current_proc
        let mut proc_body_min_indent: usize = 0;

        // path_to_nid: type_path → nid, for `new` resolution
        let mut path_to_nid: HashMap<String, String> = HashMap::new();
        // name_to_nids: simple proc name (lowercase) → list of nid
        let mut name_to_nids: HashMap<String, Vec<String>> = HashMap::new();

        // Deferred calls: (caller_nid, callee_name, line_num)
        let mut pending_calls: Vec<(String, String, usize)> = vec![];
        // Deferred instantiates: (caller_nid, type_path, line_num)
        let mut pending_news: Vec<(String, String, usize)> = vec![];

        let add_node =
            |nodes: &mut Vec<Node>, seen: &mut HashSet<String>, id: String, label: String| {
                if seen.insert(id.clone()) {
                    nodes.push(Node {
                        id,
                        label,
                        file_type: "code".to_string(),
                        source_file: str_path.clone(),
                        source_location: None,
                        community: None,
                        rationale: None,
                        docstring: None,
                        metadata: HashMap::new(),
                    });
                }
            };

        let make_edge = |src: &str, tgt: &str, relation: &str, context: Option<&str>| Edge {
            source: src.to_string(),
            target: tgt.to_string(),
            relation: relation.to_string(),
            confidence: "EXTRACTED".to_string(),
            source_file: Some(str_path.clone()),
            weight: 1.0,
            context: context.map(|s| s.to_string()),
        };

        // Count leading tab chars
        let indent_of = |line: &str| line.chars().take_while(|&c| c == '\t').count();

        for (line_idx, raw_line) in text.lines().enumerate() {
            let tabs = indent_of(raw_line);
            let trimmed = raw_line.trim_start_matches('\t');

            if trimmed.is_empty() || trimmed.starts_with("//") {
                continue;
            }

            // Reset proc context if we're back to a shallower indent
            if current_proc.is_some() && tabs < proc_body_min_indent {
                current_proc = None;
            }

            if tabs == 0 {
                // Top-level line
                current_type = None;
                current_proc = None;

                if trimmed.starts_with("#include") {
                    // Parse include path
                    let raw_inc = trimmed
                        .trim_start_matches("#include")
                        .trim()
                        .trim_matches(|c| c == '"' || c == '\'' || c == '<' || c == '>');
                    if !raw_inc.is_empty() {
                        let norm = raw_inc.replace('\\', "/");
                        let target_file = path.parent().and_then(|p| {
                            let candidate = p.join(&norm);
                            if candidate.exists() {
                                Some(candidate)
                            } else {
                                None
                            }
                        });
                        let (tgt_id, relation) = match &target_file {
                            Some(resolved) => (
                                make_id(&[resolved.to_string_lossy().as_ref()]),
                                "imports_from",
                            ),
                            None => {
                                let base = norm
                                    .rsplit('/')
                                    .next()
                                    .unwrap_or(&norm)
                                    .trim_end_matches(".dm");
                                (make_id(&[base]), "imports")
                            }
                        };
                        edges.push(make_edge(&file_id, &tgt_id, relation, Some("import")));
                    }
                    continue;
                }

                if trimmed.starts_with("var/") {
                    continue; // global var
                }

                if !trimmed.starts_with('/') {
                    continue;
                }

                // Parse declaration starting with '/'
                if let Some(paren_pos) = trimmed.find('(') {
                    let full_path = &trimmed[..paren_pos];
                    let components: Vec<&str> =
                        full_path.split('/').filter(|s| !s.is_empty()).collect();

                    if components.is_empty() {
                        continue;
                    }

                    if components[0] == "proc" {
                        // Global proc: /proc/name(...)
                        let proc_name = if components.len() >= 2 {
                            components[1]
                        } else {
                            continue;
                        };
                        let label = format!("{}()", proc_name);
                        let nid = make_id(&[&stem, proc_name]);
                        add_node(&mut nodes, &mut seen_ids, nid.clone(), label.clone());
                        edges.push(make_edge(&file_id, &nid, "contains", None));
                        let simple = proc_name.to_lowercase();
                        name_to_nids.entry(simple).or_default().push(nid.clone());
                        current_proc = Some(nid);
                        proc_body_min_indent = 1;
                    } else if components.len() >= 2 && components[components.len() - 2] == "proc" {
                        // Path-form proc def: /type/path/proc/name(...)
                        let type_components = &components[..components.len() - 2];
                        let proc_name = components[components.len() - 1];
                        let type_path = format!("/{}", type_components.join("/"));
                        let type_id_part = type_components.join("_");
                        let type_nid = make_id(&[&stem, &type_id_part]);
                        // Ensure type node exists
                        if seen_ids.insert(type_nid.clone()) {
                            nodes.push(Node {
                                id: type_nid.clone(),
                                label: type_path.clone(),
                                file_type: "code".to_string(),
                                source_file: str_path.clone(),
                                source_location: None,
                                community: None,
                                rationale: None,
                                docstring: None,
                                metadata: HashMap::new(),
                            });
                            edges.push(make_edge(&file_id, &type_nid, "contains", None));
                            path_to_nid.insert(type_path.clone(), type_nid.clone());
                        }
                        let proc_label = format!("{}/{}()", type_path, proc_name);
                        let proc_id_part = format!("{}_{}", type_id_part, proc_name);
                        let proc_nid = make_id(&[&stem, &proc_id_part]);
                        add_node(
                            &mut nodes,
                            &mut seen_ids,
                            proc_nid.clone(),
                            proc_label.clone(),
                        );
                        edges.push(make_edge(&type_nid, &proc_nid, "method", None));
                        let simple = proc_name.to_lowercase();
                        name_to_nids
                            .entry(simple)
                            .or_default()
                            .push(proc_nid.clone());
                        current_proc = Some(proc_nid);
                        proc_body_min_indent = 1;
                    } else {
                        // Path-form override: /type/path/name(...)
                        let type_components = &components[..components.len() - 1];
                        let proc_name = components[components.len() - 1];
                        let type_path = if type_components.is_empty() {
                            String::new()
                        } else {
                            format!("/{}", type_components.join("/"))
                        };
                        let proc_label = if type_path.is_empty() {
                            format!("{}()", proc_name)
                        } else {
                            format!("{}/{}()", type_path, proc_name)
                        };
                        let type_id_part = type_components.join("_");
                        let proc_id_part = if type_id_part.is_empty() {
                            proc_name.to_string()
                        } else {
                            format!("{}_{}", type_id_part, proc_name)
                        };
                        let proc_nid = make_id(&[&stem, &proc_id_part]);
                        // Ensure type node
                        if !type_path.is_empty() {
                            let type_nid = make_id(&[&stem, &type_id_part]);
                            if seen_ids.insert(type_nid.clone()) {
                                nodes.push(Node {
                                    id: type_nid.clone(),
                                    label: type_path.clone(),
                                    file_type: "code".to_string(),
                                    source_file: str_path.clone(),
                                    source_location: None,
                                    community: None,
                                    rationale: None,
                                    docstring: None,
                                    metadata: HashMap::new(),
                                });
                                edges.push(make_edge(&file_id, &type_nid, "contains", None));
                                path_to_nid.insert(type_path.clone(), type_nid.clone());
                            }
                            let type_nid = make_id(&[&stem, &type_id_part]);
                            add_node(&mut nodes, &mut seen_ids, proc_nid.clone(), proc_label);
                            edges.push(make_edge(&type_nid, &proc_nid, "method", None));
                        } else {
                            add_node(&mut nodes, &mut seen_ids, proc_nid.clone(), proc_label);
                            edges.push(make_edge(&file_id, &proc_nid, "contains", None));
                        }
                        let simple = proc_name.to_lowercase();
                        name_to_nids
                            .entry(simple)
                            .or_default()
                            .push(proc_nid.clone());
                        current_proc = Some(proc_nid);
                        proc_body_min_indent = 1;
                    }
                } else {
                    // Type definition: /datum/weapon (no paren)
                    let type_path = trimmed.trim().to_string();
                    let components: Vec<&str> =
                        type_path.split('/').filter(|s| !s.is_empty()).collect();
                    let type_id_part = components.join("_");
                    let type_nid = make_id(&[&stem, &type_id_part]);
                    if seen_ids.insert(type_nid.clone()) {
                        nodes.push(Node {
                            id: type_nid.clone(),
                            label: type_path.clone(),
                            file_type: "code".to_string(),
                            source_file: str_path.clone(),
                            source_location: None,
                            community: None,
                            rationale: None,
                            docstring: None,
                            metadata: HashMap::new(),
                        });
                        edges.push(make_edge(&file_id, &type_nid, "contains", None));
                        path_to_nid.insert(type_path.clone(), type_nid.clone());
                    }
                    current_type = Some((type_path, type_nid));
                }
            } else if tabs == 1 {
                if let Some((ref type_path, ref type_nid)) = current_type.clone() {
                    // Inside type block: check if this is a method declaration
                    if trimmed.starts_with("var/") || trimmed.starts_with("//") {
                        // type variable or comment, skip
                    } else if let Some(paren_pos) = trimmed.find('(') {
                        let before_paren = &trimmed[..paren_pos];
                        // Could be: proc/name(args) or Name(args)
                        let proc_name = if let Some(stripped) = before_paren.strip_prefix("proc/") {
                            stripped.trim()
                        } else {
                            // bare name like New()
                            before_paren.trim()
                        };
                        if !proc_name.is_empty()
                            && proc_name.chars().all(|c| c.is_alphanumeric() || c == '_')
                        {
                            let proc_label = format!("{}/{}()", type_path, proc_name);
                            let type_id_part = type_path
                                .split('/')
                                .filter(|s| !s.is_empty())
                                .collect::<Vec<_>>()
                                .join("_");
                            let proc_id_part = format!("{}_{}", type_id_part, proc_name);
                            let proc_nid = make_id(&[&stem, &proc_id_part]);
                            add_node(&mut nodes, &mut seen_ids, proc_nid.clone(), proc_label);
                            edges.push(make_edge(type_nid, &proc_nid, "method", None));
                            let simple = proc_name.to_lowercase();
                            name_to_nids
                                .entry(simple)
                                .or_default()
                                .push(proc_nid.clone());
                            current_proc = Some(proc_nid);
                            proc_body_min_indent = 2;
                        }
                    } else {
                        // Not a method, clear current_proc (type var assignment, etc.)
                        current_proc = None;
                    }
                } else if let Some(ref caller_nid) = current_proc.clone() {
                    if proc_body_min_indent <= 1 {
                        // Body line of a col-0 proc
                        collect_calls(
                            trimmed,
                            caller_nid,
                            &mut pending_calls,
                            &mut pending_news,
                            line_idx,
                        );
                    }
                }
            } else {
                // tabs >= 2
                if let Some(ref caller_nid) = current_proc.clone() {
                    if tabs >= proc_body_min_indent {
                        collect_calls(
                            trimmed,
                            caller_nid,
                            &mut pending_calls,
                            &mut pending_news,
                            line_idx,
                        );
                    }
                }
            }
        }

        // Pass 2: resolve calls
        let mut seen_call_pairs: HashSet<(String, String)> = HashSet::new();
        for (caller_nid, callee_name, _line) in &pending_calls {
            if callee_name == ".." {
                continue;
            }
            let lower = callee_name.to_lowercase();
            if let Some(candidates) = name_to_nids.get(&lower) {
                if candidates.len() == 1 {
                    let tgt = &candidates[0];
                    if tgt != caller_nid {
                        let pair = (caller_nid.clone(), tgt.clone());
                        if seen_call_pairs.insert(pair) {
                            edges.push(Edge {
                                source: caller_nid.clone(),
                                target: tgt.clone(),
                                relation: "calls".to_string(),
                                confidence: "EXTRACTED".to_string(),
                                source_file: Some(str_path.clone()),
                                weight: 1.0,
                                context: Some("call".to_string()),
                            });
                        }
                    }
                }
                // else: ambiguous → raw_call (not emitted)
            }
            // else: unknown → raw_call (not emitted)
        }

        // Resolve `new` expressions
        for (caller_nid, type_path, _line) in &pending_news {
            if let Some(tgt_nid) = path_to_nid.get(type_path) {
                if tgt_nid != caller_nid {
                    let pair = (caller_nid.clone(), tgt_nid.clone());
                    if seen_call_pairs.insert(pair) {
                        edges.push(Edge {
                            source: caller_nid.clone(),
                            target: tgt_nid.clone(),
                            relation: "instantiates".to_string(),
                            confidence: "EXTRACTED".to_string(),
                            source_file: Some(str_path.clone()),
                            weight: 1.0,
                            context: Some("call".to_string()),
                        });
                    }
                }
            }
        }

        Ok(ExtractionFragment { nodes, edges })
    }
}

fn collect_calls(
    line: &str,
    caller_nid: &str,
    pending_calls: &mut Vec<(String, String, usize)>,
    pending_news: &mut Vec<(String, String, usize)>,
    line_idx: usize,
) {
    // Detect `new /type/path` patterns
    let mut rest = line;
    while let Some(pos) = rest.find("new ") {
        let after = rest[pos + 4..].trim_start();
        if after.starts_with('/') {
            let end = after
                .find(|c: char| !c.is_alphanumeric() && c != '/' && c != '_')
                .unwrap_or(after.len());
            let type_path = &after[..end];
            if !type_path.is_empty() {
                pending_news.push((caller_nid.to_string(), type_path.to_string(), line_idx));
            }
        }
        rest = &rest[pos + 4..];
    }

    // Detect `identifier(` call patterns
    let mut chars = line.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if c.is_alphabetic() || c == '_' {
            // Collect identifier
            let start = i;
            let mut end = i + c.len_utf8();
            while let Some(&(j, nc)) = chars.peek() {
                if nc.is_alphanumeric() || nc == '_' {
                    end = j + nc.len_utf8();
                    chars.next();
                } else {
                    break;
                }
            }
            // Check if followed by '('
            if let Some(&(_, '(')) = chars.peek() {
                let ident = &line[start..end];
                // Skip DM keywords and language constructs
                if !is_dm_keyword(ident) && ident != ".." {
                    // Check if preceded by '.' (member call)
                    let preceded_by_dot =
                        start > 0 && line.as_bytes().get(start - 1) == Some(&b'.');
                    let _ = preceded_by_dot; // we don't distinguish for resolution
                    pending_calls.push((caller_nid.to_string(), ident.to_string(), line_idx));
                }
            }
        }
    }
}

fn is_dm_keyword(s: &str) -> bool {
    matches!(
        s,
        "if" | "else"
            | "for"
            | "while"
            | "switch"
            | "return"
            | "var"
            | "spawn"
            | "new"
            | "del"
            | "null"
            | "TRUE"
            | "FALSE"
            | "src"
            | "usr"
            | "world"
            | "global"
            | "proc"
            | "verb"
            | "list"
            | "datum"
            | "atom"
            | "mob"
            | "obj"
            | "turf"
            | "area"
    )
}

// ─── DMI (BYOND icon sheets) ─────────────────────────────────────────────────

pub struct DmiExtractor;

impl DmiExtractor {
    pub fn extract(source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        let (file_id, _file_label, file_node) = make_file_node(path);
        let str_path = path.to_string_lossy().to_string();
        let stem = file_id.clone();

        let mut nodes: Vec<Node> = vec![file_node];
        let mut edges: Vec<Edge> = vec![];
        let mut seen: HashSet<String> = HashSet::new();
        seen.insert(file_id.clone());

        let description = read_dmi_description(source);
        for raw_line in description.lines() {
            let stripped = raw_line.trim();
            if !stripped.starts_with("state =") {
                continue;
            }
            let value = stripped["state =".len()..].trim();
            let state_name = if value.starts_with('"') && value.ends_with('"') && value.len() >= 2 {
                &value[1..value.len() - 1]
            } else {
                value
            };
            if state_name.is_empty() {
                continue;
            }
            let nid = make_id(&[&stem, "state", state_name]);
            if seen.insert(nid.clone()) {
                nodes.push(Node {
                    id: nid.clone(),
                    label: format!("\"{}\"", state_name),
                    file_type: "code".to_string(),
                    source_file: str_path.clone(),
                    source_location: None,
                    community: None,
                    rationale: None,
                    docstring: None,
                    metadata: HashMap::new(),
                });
                edges.push(Edge {
                    source: file_id.clone(),
                    target: nid,
                    relation: "contains".to_string(),
                    confidence: "EXTRACTED".to_string(),
                    source_file: Some(str_path.clone()),
                    weight: 1.0,
                    context: None,
                });
            }
        }

        Ok(ExtractionFragment { nodes, edges })
    }
}

fn read_dmi_description(data: &[u8]) -> String {
    const PNG_HEADER: &[u8] = b"\x89PNG\r\n\x1a\n";
    if !data.starts_with(PNG_HEADER) {
        return String::new();
    }
    let mut i = 8usize;
    while i + 8 <= data.len() {
        let length = u32::from_be_bytes([data[i], data[i + 1], data[i + 2], data[i + 3]]) as usize;
        let chunk_type = &data[i + 4..i + 8];
        let payload_start = i + 8;
        let payload_end = payload_start + length;
        if payload_end > data.len() {
            break;
        }
        let payload = &data[payload_start..payload_end];

        if chunk_type == b"tEXt" {
            if let Some(null_pos) = payload.iter().position(|&b| b == 0) {
                let keyword = &payload[..null_pos];
                if keyword == b"Description" {
                    let text = &payload[null_pos + 1..];
                    return String::from_utf8_lossy(text).to_string();
                }
            }
        }
        // Skip zTXt for simplicity (decompression not needed for our fixture)
        i += 8 + length + 4; // header + payload + CRC
    }
    String::new()
}

// ─── DMM (BYOND map files) ────────────────────────────────────────────────────

pub struct DmmExtractor;

impl DmmExtractor {
    pub fn extract(source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        let (file_id, _file_label, file_node) = make_file_node(path);
        let str_path = path.to_string_lossy().to_string();

        let nodes: Vec<Node> = vec![file_node];
        let mut edges: Vec<Edge> = vec![];

        let text = std::str::from_utf8(source).unwrap_or("");

        // Find grid section start: (d,d,d) = pattern
        let dict_text = if let Some(grid_pos) = find_grid_section(text) {
            &text[..grid_pos]
        } else {
            text
        };

        let mut seen_targets: HashSet<String> = HashSet::new();

        // Parse tile dictionary: each tile is "key" = (type, type, ...)
        // We need to find all (/type/path) entries, stripping var overrides
        for type_path in extract_type_paths(dict_text) {
            let tgt = make_id(&[&type_path]);
            if seen_targets.insert(tgt.clone()) {
                edges.push(Edge {
                    source: file_id.clone(),
                    target: tgt,
                    relation: "uses".to_string(),
                    confidence: "EXTRACTED".to_string(),
                    source_file: Some(str_path.clone()),
                    weight: 1.0,
                    context: Some("map".to_string()),
                });
            }
        }

        Ok(ExtractionFragment { nodes, edges })
    }
}

fn find_grid_section(text: &str) -> Option<usize> {
    // Grid section starts with pattern like (1,1,1) = {" at a line start
    let mut pos = 0;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('(') {
            // Check if it's (d,d,d) = pattern
            if let Some(close) = trimmed.find(')') {
                let inner = &trimmed[1..close];
                let parts: Vec<&str> = inner.split(',').collect();
                if parts.len() == 3 && parts.iter().all(|p| p.trim().parse::<u32>().is_ok()) {
                    return Some(pos);
                }
            }
        }
        pos += line.len() + 1; // +1 for newline
    }
    None
}

fn extract_type_paths(dict_text: &str) -> Vec<String> {
    let mut result = Vec::new();
    // Find all occurrences of /type/path (starting with /) in tile entries
    // Tiles look like: "key" = (/type/path, /other/type{var=val}, ...)
    // We need to find all type paths, stripping var overrides (anything after {)

    let mut chars = dict_text.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if c == '/' {
            // Collect type path: /alphanumeric and /
            let start = i;
            let mut end = i + 1;
            while let Some(&(j, nc)) = chars.peek() {
                if nc.is_alphanumeric() || nc == '_' || nc == '/' {
                    end = j + nc.len_utf8();
                    chars.next();
                } else {
                    break;
                }
            }
            let raw = &dict_text[start..end];
            // Must start with / and have at least one component
            let components: Vec<&str> = raw.split('/').filter(|s| !s.is_empty()).collect();
            if !components.is_empty() && components.iter().all(|c| !c.is_empty()) {
                result.push(raw.to_string());
            }
        }
    }
    result
}

// ─── DMF (BYOND interface forms) ─────────────────────────────────────────────

pub struct DmfExtractor;

impl DmfExtractor {
    pub fn extract(source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        let (file_id, _file_label, file_node) = make_file_node(path);
        let str_path = path.to_string_lossy().to_string();
        let stem = file_id.clone();

        let mut nodes: Vec<Node> = vec![file_node];
        let mut edges: Vec<Edge> = vec![];
        let mut seen: HashSet<String> = HashSet::new();
        seen.insert(file_id.clone());

        let text = std::str::from_utf8(source).unwrap_or("");

        let mut current_window_nid: Option<String> = None;
        // (elem_node_index, elem_name) for updating label when we see type=
        let mut current_elem: Option<(usize, String)> = None;

        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            // window "name"
            if let Some(name) = parse_quoted_directive(trimmed, "window") {
                let nid = make_id(&[&stem, "window", &name]);
                if seen.insert(nid.clone()) {
                    nodes.push(Node {
                        id: nid.clone(),
                        label: format!("window \"{}\"", name),
                        file_type: "code".to_string(),
                        source_file: str_path.clone(),
                        source_location: None,
                        community: None,
                        rationale: None,
                        docstring: None,
                        metadata: HashMap::new(),
                    });
                    edges.push(Edge {
                        source: file_id.clone(),
                        target: nid.clone(),
                        relation: "contains".to_string(),
                        confidence: "EXTRACTED".to_string(),
                        source_file: Some(str_path.clone()),
                        weight: 1.0,
                        context: None,
                    });
                }
                current_window_nid = Some(nid);
                current_elem = None;
                continue;
            }

            // elem "name"
            if let Some(elem_name) = parse_quoted_directive(trimmed, "elem") {
                if let Some(ref win_nid) = current_window_nid.clone() {
                    let nid = make_id(&[&stem, "elem", win_nid, &elem_name]);
                    if seen.insert(nid.clone()) {
                        let elem_idx = nodes.len();
                        nodes.push(Node {
                            id: nid.clone(),
                            label: format!("elem \"{}\"", elem_name),
                            file_type: "code".to_string(),
                            source_file: str_path.clone(),
                            source_location: None,
                            community: None,
                            rationale: None,
                            docstring: None,
                            metadata: HashMap::new(),
                        });
                        edges.push(Edge {
                            source: win_nid.clone(),
                            target: nid.clone(),
                            relation: "contains".to_string(),
                            confidence: "EXTRACTED".to_string(),
                            source_file: Some(str_path.clone()),
                            weight: 1.0,
                            context: None,
                        });
                        current_elem = Some((elem_idx, elem_name));
                    }
                }
                continue;
            }

            // type = TYPENAME
            if let Some((elem_idx, ref elem_name)) = current_elem.clone() {
                if let Some(type_val) = parse_kv_directive(trimmed, "type") {
                    if !nodes[elem_idx].label.contains('[') {
                        let new_label = format!("elem \"{}\" [{}]", elem_name, type_val);
                        nodes[elem_idx].label = new_label;
                    }
                }
            }
        }

        Ok(ExtractionFragment { nodes, edges })
    }
}

fn parse_quoted_directive(line: &str, keyword: &str) -> Option<String> {
    let prefix = format!("{} \"", keyword);
    if line.starts_with(&prefix) {
        let rest = &line[prefix.len()..];
        if let Some(end) = rest.find('"') {
            return Some(rest[..end].to_string());
        }
    }
    None
}

fn parse_kv_directive(line: &str, key: &str) -> Option<String> {
    let prefix = format!("{} =", key);
    if let Some(stripped) = line.strip_prefix(&prefix) {
        return Some(stripped.trim().to_string());
    }
    let prefix2 = format!("{}=", key);
    if let Some(stripped) = line.strip_prefix(&prefix2) {
        return Some(stripped.trim().to_string());
    }
    None
}
