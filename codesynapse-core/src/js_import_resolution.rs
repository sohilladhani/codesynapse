use crate::extract::make_id;
use crate::import_extension::resolve_js_module_path;
use crate::types::{Edge, Node};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Fact structs
// ---------------------------------------------------------------------------

struct SymbolDeclarationFact {
    file_path: PathBuf,
    name: String,
    line: usize,
}

struct SymbolImportFact {
    file_path: PathBuf,
    local_name: String,
    target_path: PathBuf,
    imported_name: String,
    #[allow(dead_code)]
    line: usize,
}

struct SymbolAliasFact {
    file_path: PathBuf,
    alias: String,
    target_name: String,
    #[allow(dead_code)]
    line: usize,
}

struct SymbolExportFact {
    file_path: PathBuf,
    exported_name: String,
    #[allow(dead_code)]
    line: usize,
    local_name: Option<String>,
    target_path: Option<PathBuf>,
    target_name: Option<String>,
}

struct StarExportFact {
    file_path: PathBuf,
    target_path: PathBuf,
    #[allow(dead_code)]
    line: usize,
}

struct SymbolUseFact {
    file_path: PathBuf,
    source_id: String,
    local_name: String,
    relation: String,
    #[allow(dead_code)]
    context: String,
    #[allow(dead_code)]
    line: usize,
}

#[derive(Default)]
struct SymbolResolutionFacts {
    declarations: Vec<SymbolDeclarationFact>,
    imports: Vec<SymbolImportFact>,
    aliases: Vec<SymbolAliasFact>,
    exports: Vec<SymbolExportFact>,
    star_exports: Vec<StarExportFact>,
    uses: Vec<SymbolUseFact>,
}

// ---------------------------------------------------------------------------
// Helper: file stem (mirrors Python _file_stem)
// ---------------------------------------------------------------------------

pub fn js_file_stem(rel_path: &str) -> String {
    let path = Path::new(rel_path);
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let parent = path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("");
    if !parent.is_empty() && parent != "." {
        format!("{}.{}", parent, stem)
    } else {
        stem.to_string()
    }
}

// ---------------------------------------------------------------------------
// AST helpers
// ---------------------------------------------------------------------------

fn walk_nodes(node: tree_sitter::Node) -> Vec<tree_sitter::Node> {
    let mut stack = vec![node];
    let mut result = Vec::new();
    while let Some(n) = stack.pop() {
        result.push(n);
        for i in (0..n.child_count()).rev() {
            if let Some(child) = n.child(i) {
                stack.push(child);
            }
        }
    }
    result
}

fn node_text<'a>(node: tree_sitter::Node<'_>, source: &'a [u8]) -> &'a str {
    std::str::from_utf8(&source[node.start_byte()..node.end_byte()]).unwrap_or("")
}

fn string_content(node: tree_sitter::Node<'_>, source: &[u8]) -> Option<String> {
    let text = node_text(node, source);
    Some(
        text.trim_matches(|c| c == '\'' || c == '"' || c == '`')
            .to_string(),
    )
}

fn js_module_specifier(node: tree_sitter::Node<'_>, source: &[u8]) -> Option<String> {
    if let Some(src) = node.child_by_field_name("source") {
        return string_content(src, source);
    }
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            if child.kind() == "string" {
                return string_content(child, source);
            }
        }
    }
    None
}

fn js_named_specifiers(
    node: tree_sitter::Node<'_>,
    source: &[u8],
    specifier_type: &str,
) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    for n in walk_nodes(node) {
        if n.kind() != specifier_type {
            continue;
        }
        let name_node = match n.child_by_field_name("name") {
            Some(nn) => nn,
            None => continue,
        };
        let name = node_text(name_node, source).to_string();
        let alias_node = n.child_by_field_name("alias");
        let alias = if let Some(a) = alias_node {
            node_text(a, source).to_string()
        } else {
            name.clone()
        };
        if !name.is_empty() && !alias.is_empty() {
            pairs.push((name, alias));
        }
    }
    pairs
}

fn js_lexical_aliases(node: tree_sitter::Node<'_>, source: &[u8]) -> Vec<(String, String)> {
    if node.kind() != "lexical_declaration" {
        return vec![];
    }
    let mut aliases = Vec::new();
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            if child.kind() != "variable_declarator" {
                continue;
            }
            let name_node = child.child_by_field_name("name");
            let value_node = child.child_by_field_name("value");
            if let (Some(name_n), Some(val_n)) = (name_node, value_node) {
                if val_n.kind() == "identifier" || val_n.kind() == "type_identifier" {
                    aliases.push((
                        node_text(name_n, source).to_string(),
                        node_text(val_n, source).to_string(),
                    ));
                }
            }
        }
    }
    aliases
}

fn js_export_is_star(node: tree_sitter::Node<'_>) -> bool {
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            if child.kind() == "*" {
                return true;
            }
        }
    }
    false
}

fn js_exported_declaration_names(node: tree_sitter::Node<'_>, source: &[u8]) -> Vec<String> {
    let decl = match node.child_by_field_name("declaration") {
        Some(d) => d,
        None => return vec![],
    };
    let mut names = Vec::new();
    match decl.kind() {
        "lexical_declaration" => {
            for (alias, _) in js_lexical_aliases(decl, source) {
                names.push(alias);
            }
        }
        "class_declaration"
        | "abstract_class_declaration"
        | "interface_declaration"
        | "type_alias_declaration"
        | "function_declaration" => {
            if let Some(name_node) = decl.child_by_field_name("name") {
                let name = node_text(name_node, source).to_string();
                if !name.is_empty() {
                    names.push(name);
                }
            }
        }
        _ => {}
    }
    names
}

fn ts_collect_type_refs(
    node: tree_sitter::Node<'_>,
    source: &[u8],
    generic: bool,
    out: &mut Vec<(String, String)>,
) {
    const PRIMITIVES: &[&str] = &[
        "string",
        "number",
        "boolean",
        "any",
        "unknown",
        "void",
        "never",
        "object",
        "null",
        "undefined",
        "bigint",
        "symbol",
        "this",
        "T",
    ];

    let kind = node.kind();
    match kind {
        "type_annotation" => {
            for i in 0..node.child_count() {
                if let Some(child) = node.child(i) {
                    if child.is_named() {
                        ts_collect_type_refs(child, source, generic, out);
                    }
                }
            }
        }
        "type_identifier" | "identifier" => {
            let name = node_text(node, source);
            if !name.is_empty() && !PRIMITIVES.contains(&name) {
                out.push((
                    name.to_string(),
                    if generic {
                        "generic_arg".to_string()
                    } else {
                        "type".to_string()
                    },
                ));
            }
        }
        "generic_type" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                let text = node_text(name_node, source)
                    .rsplit('.')
                    .next()
                    .unwrap_or("")
                    .to_string();
                if !text.is_empty() && !PRIMITIVES.contains(&text.as_str()) {
                    out.push((
                        text,
                        if generic {
                            "generic_arg".to_string()
                        } else {
                            "type".to_string()
                        },
                    ));
                }
            }
            for i in 0..node.child_count() {
                if let Some(child) = node.child(i) {
                    if child.kind() == "type_arguments" {
                        for j in 0..child.child_count() {
                            if let Some(sub) = child.child(j) {
                                if sub.is_named() {
                                    ts_collect_type_refs(sub, source, true, out);
                                }
                            }
                        }
                    }
                }
            }
        }
        "nested_type_identifier" => {
            let text = node_text(node, source)
                .rsplit('.')
                .next()
                .unwrap_or("")
                .to_string();
            if !text.is_empty() && !PRIMITIVES.contains(&text.as_str()) {
                out.push((
                    text,
                    if generic {
                        "generic_arg".to_string()
                    } else {
                        "type".to_string()
                    },
                ));
            }
        }
        _ => {
            if node.is_named() {
                for i in 0..node.child_count() {
                    if let Some(child) = node.child(i) {
                        if child.is_named() {
                            ts_collect_type_refs(child, source, generic, out);
                        }
                    }
                }
            }
        }
    }
}

fn ts_heritage_clause_entries(clause_node: tree_sitter::Node<'_>, source: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    for i in 0..clause_node.child_count() {
        let child = match clause_node.child(i) {
            Some(c) => c,
            None => continue,
        };
        if !child.is_named() {
            continue;
        }
        match child.kind() {
            "identifier" | "type_identifier" => {
                let name = node_text(child, source).to_string();
                if !name.is_empty() {
                    out.push(name);
                }
            }
            "generic_type" => {
                if let Some(name_node) = child.child_by_field_name("name") {
                    let text = node_text(name_node, source)
                        .rsplit('.')
                        .next()
                        .unwrap_or("")
                        .to_string();
                    if !text.is_empty() {
                        out.push(text);
                    }
                }
            }
            "nested_type_identifier" => {
                let text = node_text(child, source)
                    .rsplit('.')
                    .next()
                    .unwrap_or("")
                    .to_string();
                if !text.is_empty() {
                    out.push(text);
                }
            }
            _ => {}
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Check if a file is JS/TS
// ---------------------------------------------------------------------------

fn is_js_ts_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("ts") | Some("tsx") | Some("js") | Some("jsx") | Some("mjs") | Some("svelte")
    )
}

fn parse_js_ts(path: &Path, source: &[u8]) -> Option<tree_sitter::Tree> {
    let mut parser = tree_sitter::Parser::new();
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let lang: tree_sitter::Language = match ext {
        "ts" | "tsx" => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        _ => tree_sitter_javascript::LANGUAGE.into(),
    };
    parser.set_language(&lang).ok()?;
    parser.parse(source, None)
}

// ---------------------------------------------------------------------------
// make_node helper
// ---------------------------------------------------------------------------

fn make_node(id: String, label: String, source_file: String, line: Option<usize>) -> Node {
    let source_location = line.map(|l| format!("{}:{}", source_file, l + 1));
    Node {
        id,
        label,
        file_type: "code".to_string(),
        source_file,
        source_location,
        community: None,
        rationale: None,
        docstring: None,
        metadata: std::collections::HashMap::new(),
    }
}

fn make_edge(
    source: String,
    target: String,
    relation: String,
    source_file: Option<String>,
) -> Edge {
    Edge {
        source,
        target,
        relation,
        confidence: "EXTRACTED".to_string(),
        source_file,
        weight: 1.0,
        context: None,
    }
}

// ---------------------------------------------------------------------------
// Phase 1: extract basic file/symbol/method nodes + file-level import edges
// ---------------------------------------------------------------------------

fn extract_js_ts_file_nodes(
    abs_path: &Path,
    rel_path: &Path,
    source: &[u8],
    nodes: &mut Vec<Node>,
    edges: &mut Vec<Edge>,
    edge_set: &mut HashSet<(String, String, String)>,
) {
    let rel_str = rel_path.to_string_lossy().to_string();
    let file_id = make_id(&[&rel_str]);
    let file_label = abs_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(&rel_str)
        .to_string();

    // File node
    nodes.push(make_node(
        file_id.clone(),
        file_label,
        rel_str.clone(),
        None,
    ));

    let tree = match parse_js_ts(abs_path, source) {
        Some(t) => t,
        None => return,
    };
    let root = tree.root_node();
    let file_stem = js_file_stem(&rel_str);

    // Walk top-level children for exports, functions, consts, classes
    for i in 0..root.child_count() {
        let child = match root.child(i) {
            Some(c) => c,
            None => continue,
        };

        match child.kind() {
            "export_statement" => {
                // export { ... } from or export * from (no declaration node)
                if child.child_by_field_name("declaration").is_none() {
                    let source_field = child.child_by_field_name("source");
                    if source_field.is_some() || js_export_is_star(child) {
                        // re-export statement handled in facts phase
                    } else {
                        // export clause (local re-exports), no declaration
                    }
                    continue;
                }
                // export function/class/const/type ...
                let names = js_exported_declaration_names(child, source);
                let line = child.start_position().row;
                for name in names {
                    let sym_id = make_id(&[&file_stem, &name]);
                    nodes.push(make_node(sym_id.clone(), name, rel_str.clone(), Some(line)));
                    let ek = (file_id.clone(), sym_id.clone(), "contains".to_string());
                    if edge_set.insert(ek) {
                        edges.push(make_edge(
                            file_id.clone(),
                            sym_id,
                            "contains".to_string(),
                            Some(rel_str.clone()),
                        ));
                    }
                    // Check for methods in class declarations inside export
                    if let Some(decl) = child.child_by_field_name("declaration") {
                        let decl_kind = decl.kind();
                        if decl_kind == "class_declaration"
                            || decl_kind == "abstract_class_declaration"
                        {
                            if let Some(name_node) = decl.child_by_field_name("name") {
                                let class_name = node_text(name_node, source).to_string();
                                let class_nid = make_id(&[&file_stem, &class_name]);
                                extract_class_methods(
                                    decl, source, &class_nid, &rel_str, nodes, edges, edge_set,
                                );
                            }
                        }
                    }
                }
            }
            "function_declaration" => {
                let line = child.start_position().row;
                if let Some(name_node) = child.child_by_field_name("name") {
                    let name = node_text(name_node, source).to_string();
                    if !name.is_empty() {
                        let sym_id = make_id(&[&file_stem, &name]);
                        if !nodes.iter().any(|n| n.id == sym_id) {
                            nodes.push(make_node(
                                sym_id.clone(),
                                name,
                                rel_str.clone(),
                                Some(line),
                            ));
                            let ek = (file_id.clone(), sym_id.clone(), "contains".to_string());
                            if edge_set.insert(ek) {
                                edges.push(make_edge(
                                    file_id.clone(),
                                    sym_id,
                                    "contains".to_string(),
                                    Some(rel_str.clone()),
                                ));
                            }
                        }
                    }
                }
            }
            "lexical_declaration" => {
                let line = child.start_position().row;
                for i2 in 0..child.child_count() {
                    if let Some(declarator) = child.child(i2) {
                        if declarator.kind() != "variable_declarator" {
                            continue;
                        }
                        let name_node = declarator.child_by_field_name("name");
                        let value_node = declarator.child_by_field_name("value");
                        if let (Some(nn), Some(vn)) = (name_node, value_node) {
                            if vn.kind() == "arrow_function" || vn.kind() == "function" {
                                let name = node_text(nn, source).to_string();
                                if !name.is_empty() {
                                    let sym_id = make_id(&[&file_stem, &name]);
                                    if !nodes.iter().any(|n| n.id == sym_id) {
                                        nodes.push(make_node(
                                            sym_id.clone(),
                                            name,
                                            rel_str.clone(),
                                            Some(line),
                                        ));
                                        let ek = (
                                            file_id.clone(),
                                            sym_id.clone(),
                                            "contains".to_string(),
                                        );
                                        if edge_set.insert(ek) {
                                            edges.push(make_edge(
                                                file_id.clone(),
                                                sym_id,
                                                "contains".to_string(),
                                                Some(rel_str.clone()),
                                            ));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            "class_declaration" | "abstract_class_declaration" => {
                let line = child.start_position().row;
                if let Some(name_node) = child.child_by_field_name("name") {
                    let name = node_text(name_node, source).to_string();
                    if !name.is_empty() {
                        let sym_id = make_id(&[&file_stem, &name]);
                        if !nodes.iter().any(|n| n.id == sym_id) {
                            nodes.push(make_node(
                                sym_id.clone(),
                                name,
                                rel_str.clone(),
                                Some(line),
                            ));
                            let ek = (file_id.clone(), sym_id.clone(), "contains".to_string());
                            if edge_set.insert(ek) {
                                edges.push(make_edge(
                                    file_id.clone(),
                                    sym_id.clone(),
                                    "contains".to_string(),
                                    Some(rel_str.clone()),
                                ));
                            }
                        }
                        extract_class_methods(
                            child, source, &sym_id, &rel_str, nodes, edges, edge_set,
                        );
                    }
                }
            }
            _ => {}
        }
    }
}

fn extract_class_methods(
    class_node: tree_sitter::Node<'_>,
    source: &[u8],
    class_nid: &str,
    rel_str: &str,
    nodes: &mut Vec<Node>,
    edges: &mut Vec<Edge>,
    edge_set: &mut HashSet<(String, String, String)>,
) {
    // Find class body
    let body = match class_node.child_by_field_name("body") {
        Some(b) => b,
        None => return,
    };
    for i in 0..body.child_count() {
        let member = match body.child(i) {
            Some(m) => m,
            None => continue,
        };
        match member.kind() {
            "method_definition" | "method_signature" | "abstract_method_signature" => {
                let line = member.start_position().row;
                if let Some(name_node) = member.child_by_field_name("name") {
                    let method_name = node_text(name_node, source).to_string();
                    if method_name.is_empty() || method_name == "constructor" {
                        continue;
                    }
                    let method_nid = make_id(&[class_nid, &method_name]);
                    if !nodes.iter().any(|n| n.id == method_nid) {
                        nodes.push(make_node(
                            method_nid.clone(),
                            method_name,
                            rel_str.to_string(),
                            Some(line),
                        ));
                        let ek = (
                            class_nid.to_string(),
                            method_nid.clone(),
                            "contains".to_string(),
                        );
                        if edge_set.insert(ek) {
                            edges.push(make_edge(
                                class_nid.to_string(),
                                method_nid,
                                "contains".to_string(),
                                Some(rel_str.to_string()),
                            ));
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Phase 2: collect symbol resolution facts
// ---------------------------------------------------------------------------

fn collect_js_symbol_resolution_facts(
    abs_path: &Path,
    rel_path: &Path,
    source: &[u8],
    facts: &mut SymbolResolutionFacts,
    canon_to_rel: &HashMap<PathBuf, PathBuf>,
) {
    let rel_str = rel_path.to_string_lossy().to_string();
    let file_stem = js_file_stem(&rel_str);
    let file_dir = abs_path.parent().unwrap_or(Path::new("."));

    let tree = match parse_js_ts(abs_path, source) {
        Some(t) => t,
        None => return,
    };
    let root = tree.root_node();

    // resolve a module specifier to a canonical path
    let resolve_module = |raw: &str| -> Option<PathBuf> {
        let resolved = resolve_js_module_path(raw, file_dir)?;
        std::fs::canonicalize(&resolved).ok().or(Some(resolved))
    };

    // Pass 1: declarations, imports, aliases
    for node in walk_nodes(root) {
        match node.kind() {
            "export_statement"
                // Only collect declarations here (no source = local export)
                if node.child_by_field_name("source").is_none() && !js_export_is_star(node) => {
                    for name in js_exported_declaration_names(node, source) {
                        facts.declarations.push(SymbolDeclarationFact {
                            file_path: abs_path.to_path_buf(),
                            name,
                            line: node.start_position().row,
                        });
                    }
                }
            "function_declaration"
                // top-level function declarations (non-exported)
                if node.parent().map(|p| p.kind()) == Some("program") => {
                    if let Some(name_node) = node.child_by_field_name("name") {
                        let name = node_text(name_node, source).to_string();
                        if !name.is_empty() {
                            facts.declarations.push(SymbolDeclarationFact {
                                file_path: abs_path.to_path_buf(),
                                name,
                                line: node.start_position().row,
                            });
                        }
                    }
                }
            "lexical_declaration"
                if node.parent().map(|p| p.kind()) == Some("program") => {
                    // const X = Y style aliases
                    for (alias, target) in js_lexical_aliases(node, source) {
                        facts.aliases.push(SymbolAliasFact {
                            file_path: abs_path.to_path_buf(),
                            alias,
                            target_name: target,
                            line: node.start_position().row,
                        });
                    }
                }
            "import_statement" => {
                let line = node.start_position().row;
                if let Some(raw) = js_module_specifier(node, source) {
                    if let Some(target_path) = resolve_module(&raw) {
                        // named imports
                        let specifiers = js_named_specifiers(node, source, "import_specifier");
                        for (imported_name, local_name) in specifiers {
                            facts.imports.push(SymbolImportFact {
                                file_path: abs_path.to_path_buf(),
                                local_name,
                                target_path: target_path.clone(),
                                imported_name,
                                line,
                            });
                        }
                    }
                }
            }
            _ => {}
        }
    }

    // Pass 2: exports (re-exports with source or export clause)
    for node in walk_nodes(root) {
        if node.kind() != "export_statement" {
            continue;
        }
        let line = node.start_position().row;
        let source_field = node.child_by_field_name("source");

        if let Some(src_node) = source_field {
            let raw = match string_content(src_node, source) {
                Some(s) => s,
                None => continue,
            };
            let target_path = match resolve_module(&raw) {
                Some(p) => p,
                None => continue,
            };

            if js_export_is_star(node) {
                facts.star_exports.push(StarExportFact {
                    file_path: abs_path.to_path_buf(),
                    target_path,
                    line,
                });
            } else {
                // export { X as Y } from '...'
                let specifiers = js_named_specifiers(node, source, "export_specifier");
                for (orig_name, exported_name) in specifiers {
                    facts.exports.push(SymbolExportFact {
                        file_path: abs_path.to_path_buf(),
                        exported_name,
                        line,
                        local_name: None,
                        target_path: Some(target_path.clone()),
                        target_name: Some(orig_name),
                    });
                }
            }
        } else if node.child_by_field_name("declaration").is_none() {
            // export { X } or export { X as Y } (local)
            let specifiers = js_named_specifiers(node, source, "export_specifier");
            for (local_name, exported_name) in specifiers {
                facts.exports.push(SymbolExportFact {
                    file_path: abs_path.to_path_buf(),
                    exported_name,
                    line,
                    local_name: Some(local_name),
                    target_path: None,
                    target_name: None,
                });
            }
        } else {
            // export function/class/const ... (already handled in pass 1, but
            // also add an export fact so other files can import from here)
            for name in js_exported_declaration_names(node, source) {
                facts.exports.push(SymbolExportFact {
                    file_path: abs_path.to_path_buf(),
                    exported_name: name.clone(),
                    line,
                    local_name: Some(name),
                    target_path: None,
                    target_name: None,
                });
            }
        }
    }

    // Pass 3: function call uses (top-level function bodies & arrow functions)
    let mut bodies: Vec<tree_sitter::Node> = Vec::new();
    for i in 0..root.child_count() {
        let child = match root.child(i) {
            Some(c) => c,
            None => continue,
        };
        match child.kind() {
            "function_declaration" => {
                if let Some(body) = child.child_by_field_name("body") {
                    bodies.push(body);
                }
            }
            "lexical_declaration" => {
                for j in 0..child.child_count() {
                    if let Some(declarator) = child.child(j) {
                        if declarator.kind() != "variable_declarator" {
                            continue;
                        }
                        if let Some(value) = declarator.child_by_field_name("value") {
                            if value.kind() == "arrow_function" {
                                // source_id is the const name
                                if let Some(name_n) = declarator.child_by_field_name("name") {
                                    let const_name = node_text(name_n, source).to_string();
                                    let source_id = make_id(&[&file_stem, &const_name]);
                                    // collect calls from the body of this arrow function
                                    let arrow_body = value.child_by_field_name("body");
                                    let body_node = arrow_body.unwrap_or(value);
                                    collect_call_uses(
                                        body_node, source, &source_id, abs_path, &rel_str, facts,
                                    );
                                }
                                continue;
                            }
                            if value.kind() == "function" {
                                if let Some(name_n) = declarator.child_by_field_name("name") {
                                    let const_name = node_text(name_n, source).to_string();
                                    let source_id = make_id(&[&file_stem, &const_name]);
                                    if let Some(body) = value.child_by_field_name("body") {
                                        collect_call_uses(
                                            body, source, &source_id, abs_path, &rel_str, facts,
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }
            "export_statement" => {
                if let Some(decl) = child.child_by_field_name("declaration") {
                    match decl.kind() {
                        "function_declaration" => {
                            if let Some(name_n) = decl.child_by_field_name("name") {
                                let fn_name = node_text(name_n, source).to_string();
                                let source_id = make_id(&[&file_stem, &fn_name]);
                                if let Some(body) = decl.child_by_field_name("body") {
                                    collect_call_uses(
                                        body, source, &source_id, abs_path, &rel_str, facts,
                                    );
                                }
                            }
                        }
                        "lexical_declaration" => {
                            for j in 0..decl.child_count() {
                                if let Some(declarator) = decl.child(j) {
                                    if declarator.kind() != "variable_declarator" {
                                        continue;
                                    }
                                    if let Some(value) = declarator.child_by_field_name("value") {
                                        if value.kind() == "arrow_function"
                                            || value.kind() == "function"
                                        {
                                            if let Some(name_n) =
                                                declarator.child_by_field_name("name")
                                            {
                                                let const_name =
                                                    node_text(name_n, source).to_string();
                                                let source_id = make_id(&[&file_stem, &const_name]);
                                                let body_node = value
                                                    .child_by_field_name("body")
                                                    .unwrap_or(value);
                                                collect_call_uses(
                                                    body_node, source, &source_id, abs_path,
                                                    &rel_str, facts,
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    // Pass 4: class member type uses
    for i in 0..root.child_count() {
        let child = match root.child(i) {
            Some(c) => c,
            None => continue,
        };
        let class_decl = match child.kind() {
            "class_declaration" | "abstract_class_declaration" | "interface_declaration" => {
                Some(child)
            }
            "export_statement" => {
                let decl = child.child_by_field_name("declaration");
                decl.filter(|d| {
                    matches!(
                        d.kind(),
                        "class_declaration"
                            | "abstract_class_declaration"
                            | "interface_declaration"
                    )
                })
            }
            _ => None,
        };
        if let Some(class_node) = class_decl {
            let class_name = class_node
                .child_by_field_name("name")
                .map(|n| node_text(n, source).to_string())
                .unwrap_or_default();
            if class_name.is_empty() {
                continue;
            }
            let class_nid = make_id(&[&file_stem, &class_name]);

            // Heritage: extends / implements
            for j in 0..class_node.child_count() {
                if let Some(heritage_node) = class_node.child(j) {
                    match heritage_node.kind() {
                        "class_heritage" => {
                            // Look for extends_clause and implements_clause
                            for k in 0..heritage_node.child_count() {
                                if let Some(clause) = heritage_node.child(k) {
                                    match clause.kind() {
                                        "extends_clause" => {
                                            for name in ts_heritage_clause_entries(clause, source) {
                                                facts.uses.push(SymbolUseFact {
                                                    file_path: abs_path.to_path_buf(),
                                                    source_id: class_nid.clone(),
                                                    local_name: name,
                                                    relation: "inherits".to_string(),
                                                    context: "extends".to_string(),
                                                    line: clause.start_position().row,
                                                });
                                            }
                                        }
                                        "implements_clause" => {
                                            for name in ts_heritage_clause_entries(clause, source) {
                                                facts.uses.push(SymbolUseFact {
                                                    file_path: abs_path.to_path_buf(),
                                                    source_id: class_nid.clone(),
                                                    local_name: name,
                                                    relation: "implements".to_string(),
                                                    context: "implements".to_string(),
                                                    line: clause.start_position().row,
                                                });
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                        "extends_clause" => {
                            for name in ts_heritage_clause_entries(heritage_node, source) {
                                facts.uses.push(SymbolUseFact {
                                    file_path: abs_path.to_path_buf(),
                                    source_id: class_nid.clone(),
                                    local_name: name,
                                    relation: "inherits".to_string(),
                                    context: "extends".to_string(),
                                    line: heritage_node.start_position().row,
                                });
                            }
                        }
                        "implements_clause" => {
                            for name in ts_heritage_clause_entries(heritage_node, source) {
                                facts.uses.push(SymbolUseFact {
                                    file_path: abs_path.to_path_buf(),
                                    source_id: class_nid.clone(),
                                    local_name: name,
                                    relation: "implements".to_string(),
                                    context: "implements".to_string(),
                                    line: heritage_node.start_position().row,
                                });
                            }
                        }
                        _ => {}
                    }
                }
            }

            // Methods: type annotations
            if let Some(body) = class_node.child_by_field_name("body") {
                for k in 0..body.child_count() {
                    let member = match body.child(k) {
                        Some(m) => m,
                        None => continue,
                    };
                    match member.kind() {
                        "method_definition" | "method_signature" | "abstract_method_signature" => {
                            let method_name = member
                                .child_by_field_name("name")
                                .map(|n| node_text(n, source).to_string())
                                .unwrap_or_default();
                            if method_name.is_empty() || method_name == "constructor" {
                                continue;
                            }
                            let method_nid = make_id(&[&class_nid, &method_name]);

                            // Parameters
                            if let Some(params) = member.child_by_field_name("parameters") {
                                for m in walk_nodes(params) {
                                    match m.kind() {
                                        "required_parameter" | "optional_parameter" => {
                                            if let Some(type_ann) = m.child_by_field_name("type") {
                                                let mut refs = Vec::new();
                                                ts_collect_type_refs(
                                                    type_ann, source, false, &mut refs,
                                                );
                                                for (type_name, context) in refs {
                                                    facts.uses.push(SymbolUseFact {
                                                        file_path: abs_path.to_path_buf(),
                                                        source_id: method_nid.clone(),
                                                        local_name: type_name,
                                                        relation: "references".to_string(),
                                                        context: if context == "generic_arg" {
                                                            "generic_arg".to_string()
                                                        } else {
                                                            "parameter_type".to_string()
                                                        },
                                                        line: m.start_position().row,
                                                    });
                                                }
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                            }

                            // Return type
                            if let Some(ret_type) = member.child_by_field_name("return_type") {
                                let mut refs = Vec::new();
                                ts_collect_type_refs(ret_type, source, false, &mut refs);
                                for (type_name, context) in refs {
                                    facts.uses.push(SymbolUseFact {
                                        file_path: abs_path.to_path_buf(),
                                        source_id: method_nid.clone(),
                                        local_name: type_name,
                                        relation: "references".to_string(),
                                        context: if context == "generic_arg" {
                                            "generic_arg".to_string()
                                        } else {
                                            "return_type".to_string()
                                        },
                                        line: member.start_position().row,
                                    });
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    let _ = canon_to_rel; // suppress warning
}

fn collect_call_uses(
    body: tree_sitter::Node<'_>,
    source: &[u8],
    source_id: &str,
    file_path: &Path,
    _rel_str: &str,
    facts: &mut SymbolResolutionFacts,
) {
    for node in walk_nodes(body) {
        if node.kind() != "call_expression" {
            continue;
        }
        if let Some(callee) = node.child_by_field_name("function") {
            if callee.kind() == "identifier" {
                let name = node_text(callee, source).to_string();
                if !name.is_empty() {
                    facts.uses.push(SymbolUseFact {
                        file_path: file_path.to_path_buf(),
                        source_id: source_id.to_string(),
                        local_name: name,
                        relation: "calls".to_string(),
                        context: "call".to_string(),
                        line: node.start_position().row,
                    });
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// resolve_exported_origin
// ---------------------------------------------------------------------------

fn resolve_exported_origin(
    target_path: &Path,
    imported_name: &str,
    named_exports: &HashMap<PathBuf, HashMap<String, (PathBuf, String)>>,
    star_exports: &HashMap<PathBuf, Vec<PathBuf>>,
    symbol_nodes: &HashMap<(PathBuf, String), String>,
    seen: &mut HashSet<(PathBuf, String)>,
) -> (PathBuf, String) {
    let key = (target_path.to_path_buf(), imported_name.to_string());
    if seen.contains(&key) {
        return key;
    }
    seen.insert(key.clone());

    if let Some(exports) = named_exports.get(target_path) {
        if let Some((origin_path, origin_name)) = exports.get(imported_name) {
            return resolve_exported_origin(
                origin_path,
                origin_name,
                named_exports,
                star_exports,
                symbol_nodes,
                seen,
            );
        }
    }

    let empty_vec: Vec<PathBuf> = Vec::new();
    for star_target in star_exports.get(target_path).unwrap_or(&empty_vec) {
        let star_key = (star_target.clone(), imported_name.to_string());
        if symbol_nodes.contains_key(&star_key) {
            return star_key;
        }
        let resolved = resolve_exported_origin(
            star_target,
            imported_name,
            named_exports,
            star_exports,
            symbol_nodes,
            seen,
        );
        if symbol_nodes.contains_key(&resolved) {
            return resolved;
        }
    }

    key
}

// ---------------------------------------------------------------------------
// Phase 3: apply facts → emit cross-file edges
// ---------------------------------------------------------------------------

fn apply_js_symbol_resolution_facts(
    facts: &SymbolResolutionFacts,
    nodes: &mut Vec<Node>,
    edges: &mut Vec<Edge>,
    edge_set: &mut HashSet<(String, String, String)>,
    canon_to_rel: &HashMap<PathBuf, PathBuf>,
) {
    // Build symbol_nodes: (canonical_path, name) → node_id
    let mut symbol_nodes: HashMap<(PathBuf, String), String> = HashMap::new();
    for node in nodes.iter() {
        if node.file_type != "code" {
            continue;
        }
        // The source_file of a symbol node is the rel_path string
        // We need to find the canonical path for that rel_path
        // Look for canonical path by matching rel_path string in canon_to_rel values
        let source_file_path = PathBuf::from(&node.source_file);
        // try to find canonical path that maps to this rel_path
        let canon_path = canon_to_rel
            .iter()
            .find(|(_, rel)| *rel == &source_file_path)
            .map(|(canon, _)| canon.clone());
        if let Some(canon) = canon_path {
            // strip "()" and leading "." from label
            let label = node
                .label
                .trim_start_matches('.')
                .trim_end_matches("()")
                .to_string();
            if !label.is_empty() {
                symbol_nodes.insert((canon, label), node.id.clone());
            }
        }
    }

    // Build file_id_by_canon: canonical_path → file node id
    let mut file_id_by_canon: HashMap<PathBuf, String> = HashMap::new();
    for (canon, rel) in canon_to_rel.iter() {
        let rel_str = rel.to_string_lossy().to_string();
        let file_id = make_id(&[&rel_str]);
        file_id_by_canon.insert(canon.clone(), file_id);
    }

    // ensure_symbol_node: create a symbol node if not present
    // returns the node id
    let ensure_symbol_node = |canon_path: &PathBuf,
                              name: &str,
                              line: usize,
                              symbol_nodes: &mut HashMap<(PathBuf, String), String>,
                              nodes: &mut Vec<Node>| {
        let key = (canon_path.clone(), name.to_string());
        if let Some(id) = symbol_nodes.get(&key) {
            return id.clone();
        }
        let rel_path = match canon_to_rel.get(canon_path) {
            Some(r) => r,
            None => return String::new(),
        };
        let rel_str = rel_path.to_string_lossy().to_string();
        let stem = js_file_stem(&rel_str);
        let nid = make_id(&[&stem, name]);
        if !nodes.iter().any(|n| n.id == nid) {
            nodes.push(make_node(
                nid.clone(),
                name.to_string(),
                rel_str,
                Some(line),
            ));
        }
        symbol_nodes.insert(key, nid.clone());
        nid
    };

    // Ensure all declarations
    for decl in &facts.declarations {
        ensure_symbol_node(
            &decl.file_path,
            &decl.name,
            decl.line,
            &mut symbol_nodes,
            nodes,
        );
    }

    // Build local_aliases_by_file: canon_path → { local_name → (target_canon_path, imported_name) }
    let mut local_aliases_by_file: HashMap<PathBuf, HashMap<String, (PathBuf, String)>> =
        HashMap::new();
    for imp in &facts.imports {
        local_aliases_by_file
            .entry(imp.file_path.clone())
            .or_default()
            .insert(
                imp.local_name.clone(),
                (imp.target_path.clone(), imp.imported_name.clone()),
            );
    }

    // Resolve const aliases (const X = Y where Y is an imported symbol)
    for alias in &facts.aliases {
        let local_aliases = match local_aliases_by_file.get(&alias.file_path) {
            Some(m) => m,
            None => continue,
        };
        if let Some(origin) = local_aliases.get(&alias.target_name).cloned() {
            local_aliases_by_file
                .entry(alias.file_path.clone())
                .or_default()
                .insert(alias.alias.clone(), origin);
        }
    }

    // Build named_exports_by_file: canon_path → { exported_name → (origin_canon_path, origin_name) }
    let mut named_exports_by_file: HashMap<PathBuf, HashMap<String, (PathBuf, String)>> =
        HashMap::new();
    for exp in &facts.exports {
        let origin =
            if let (Some(target_path), Some(target_name)) = (&exp.target_path, &exp.target_name) {
                // Re-export from another file
                (target_path.clone(), target_name.clone())
            } else if let Some(local_name) = &exp.local_name {
                // Local export
                // Check if local_name is actually an import (from local_aliases)
                let local_aliases = local_aliases_by_file.get(&exp.file_path);
                if let Some(aliases) = local_aliases {
                    if let Some((origin_path, origin_name)) = aliases.get(local_name) {
                        (origin_path.clone(), origin_name.clone())
                    } else {
                        // It's a locally defined symbol
                        (exp.file_path.clone(), local_name.clone())
                    }
                } else {
                    (exp.file_path.clone(), local_name.clone())
                }
            } else {
                continue;
            };

        named_exports_by_file
            .entry(exp.file_path.clone())
            .or_default()
            .insert(exp.exported_name.clone(), origin);
    }

    // Build star_exports_by_file
    let mut star_exports_by_file: HashMap<PathBuf, Vec<PathBuf>> = HashMap::new();
    for se in &facts.star_exports {
        star_exports_by_file
            .entry(se.file_path.clone())
            .or_default()
            .push(se.target_path.clone());
    }

    // Emit re_exports edges
    // star exports: file_id → target_file_id
    for se in &facts.star_exports {
        if let (Some(src_id), Some(tgt_id)) = (
            file_id_by_canon.get(&se.file_path),
            file_id_by_canon.get(&se.target_path),
        ) {
            let ek = (src_id.clone(), tgt_id.clone(), "re_exports".to_string());
            if edge_set.insert(ek) {
                edges.push(make_edge(
                    src_id.clone(),
                    tgt_id.clone(),
                    "re_exports".to_string(),
                    None,
                ));
            }
        }
    }

    // named exports from a different file → re_exports edge
    for (file_path, exports) in &named_exports_by_file {
        let src_id = match file_id_by_canon.get(file_path) {
            Some(id) => id.clone(),
            None => continue,
        };
        for (origin_path, _) in exports.values() {
            if origin_path != file_path {
                if let Some(tgt_id) = file_id_by_canon.get(origin_path) {
                    let ek = (src_id.clone(), tgt_id.clone(), "re_exports".to_string());
                    if edge_set.insert(ek) {
                        edges.push(make_edge(
                            src_id.clone(),
                            tgt_id.clone(),
                            "re_exports".to_string(),
                            None,
                        ));
                    }
                }
            }
        }
    }

    // Emit imports edges by resolving each import fact
    for imp in &facts.imports {
        let src_file_id = match file_id_by_canon.get(&imp.file_path) {
            Some(id) => id.clone(),
            None => continue,
        };
        let mut seen = HashSet::new();
        let (origin_path, origin_name) = resolve_exported_origin(
            &imp.target_path,
            &imp.imported_name,
            &named_exports_by_file,
            &star_exports_by_file,
            &symbol_nodes,
            &mut seen,
        );

        // ensure the origin symbol node exists
        ensure_symbol_node(
            &origin_path,
            &origin_name,
            imp.line,
            &mut symbol_nodes,
            nodes,
        );

        if let Some(tgt_sym_id) = symbol_nodes.get(&(origin_path.clone(), origin_name.clone())) {
            let tgt_id = tgt_sym_id.clone();
            let ek = (src_file_id.clone(), tgt_id.clone(), "imports".to_string());
            if edge_set.insert(ek) {
                edges.push(make_edge(
                    src_file_id.clone(),
                    tgt_id,
                    "imports".to_string(),
                    None,
                ));
            }
        }
    }

    // Emit uses edges
    for use_fact in &facts.uses {
        let src_id = use_fact.source_id.clone();
        let local_name = &use_fact.local_name;

        // try to resolve local_name through local_aliases
        let (origin_path, origin_name) =
            if let Some(aliases) = local_aliases_by_file.get(&use_fact.file_path) {
                if let Some((target_path, imported_name)) = aliases.get(local_name) {
                    let mut seen = HashSet::new();
                    resolve_exported_origin(
                        target_path,
                        imported_name,
                        &named_exports_by_file,
                        &star_exports_by_file,
                        &symbol_nodes,
                        &mut seen,
                    )
                } else {
                    // Local symbol
                    (use_fact.file_path.clone(), local_name.clone())
                }
            } else {
                (use_fact.file_path.clone(), local_name.clone())
            };

        // Ensure the origin symbol node exists
        ensure_symbol_node(
            &origin_path,
            &origin_name,
            use_fact.line,
            &mut symbol_nodes,
            nodes,
        );

        if let Some(tgt_id) = symbol_nodes.get(&(origin_path, origin_name)) {
            let tgt = tgt_id.clone();
            // Don't add self-loops
            if src_id == tgt {
                continue;
            }
            let ek = (src_id.clone(), tgt.clone(), use_fact.relation.clone());
            if edge_set.insert(ek) {
                edges.push(make_edge(src_id, tgt, use_fact.relation.clone(), None));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

pub fn extract_js_files(paths: &[PathBuf], root: &Path) -> (Vec<Node>, Vec<Edge>) {
    // Build canon_to_rel map
    let mut canon_to_rel: HashMap<PathBuf, PathBuf> = HashMap::new();
    for path in paths {
        if !is_js_ts_file(path) {
            continue;
        }
        let canon = std::fs::canonicalize(path)
            .ok()
            .unwrap_or_else(|| path.clone());
        let rel = path
            .strip_prefix(root)
            .map(|r| r.to_path_buf())
            .unwrap_or_else(|_| path.clone());
        canon_to_rel.insert(canon, rel);
    }

    let mut nodes: Vec<Node> = Vec::new();
    let mut edges: Vec<Edge> = Vec::new();
    let mut edge_set: HashSet<(String, String, String)> = HashSet::new();

    // Phase 1: basic nodes per file
    for (canon, rel) in &canon_to_rel {
        let source = match std::fs::read(canon) {
            Ok(s) => s,
            Err(_) => continue,
        };
        extract_js_ts_file_nodes(canon, rel, &source, &mut nodes, &mut edges, &mut edge_set);
    }

    // Phase 2: collect facts
    let mut facts = SymbolResolutionFacts::default();
    for (canon, rel) in &canon_to_rel {
        let source = match std::fs::read(canon) {
            Ok(s) => s,
            Err(_) => continue,
        };
        collect_js_symbol_resolution_facts(canon, rel, &source, &mut facts, &canon_to_rel);
    }

    // Phase 3: apply facts
    apply_js_symbol_resolution_facts(&facts, &mut nodes, &mut edges, &mut edge_set, &canon_to_rel);

    (nodes, edges)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn tmp() -> TempDir {
        tempfile::TempDir::new().unwrap()
    }

    fn write(dir: &Path, name: &str, content: &str) -> PathBuf {
        let path = dir.join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, content).unwrap();
        path
    }

    fn run(dir: &Path, paths: &[PathBuf]) -> (Vec<Node>, Vec<Edge>) {
        extract_js_files(paths, dir)
    }

    fn make_test_id(s: &str) -> String {
        s.chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect::<String>()
            .to_lowercase()
    }

    fn test_file_stem(rel_path: &str) -> String {
        js_file_stem(rel_path)
    }

    fn has_node(nodes: &[Node], id: &str) -> bool {
        let nid = make_test_id(id);
        nodes.iter().any(|n| n.id == nid)
    }

    fn has_edge(edges: &[Edge], source: &str, target: &str, relation: &str) -> bool {
        let src = make_test_id(source);
        let tgt = make_test_id(target);
        edges
            .iter()
            .any(|e| e.source == src && e.target == tgt && e.relation == relation)
    }

    // has_symbol_edge: source is a file (path string → file node id), target is a symbol node
    fn has_symbol_edge(
        edges: &[Edge],
        source_file: &str,
        target_file: &str,
        symbol: &str,
        relation: &str,
    ) -> bool {
        let src = make_test_id(source_file);
        let stem = test_file_stem(target_file);
        let tgt = make_test_id(&format!("{}_{}", stem, symbol));
        edges
            .iter()
            .any(|e| e.source == src && e.target == tgt && e.relation == relation)
    }

    fn has_symbol_to_symbol_edge(
        edges: &[Edge],
        src_file: &str,
        src_sym: &str,
        tgt_file: &str,
        tgt_sym: &str,
        relation: &str,
    ) -> bool {
        let src_stem = test_file_stem(src_file);
        let tgt_stem = test_file_stem(tgt_file);
        let src = make_test_id(&format!("{}_{}", src_stem, src_sym));
        let tgt = make_test_id(&format!("{}_{}", tgt_stem, tgt_sym));
        edges
            .iter()
            .any(|e| e.source == src && e.target == tgt && e.relation == relation)
    }

    // Test 1: simple export function creates file node + symbol node
    #[test]
    fn test_ts_exported_function_node() {
        let dir = tmp();
        write(
            dir.path(),
            "src/lib/foo.ts",
            "export function Foo() { return 42; }",
        );
        let paths = vec![dir.path().join("src/lib/foo.ts")];
        let (nodes, _edges) = run(dir.path(), &paths);
        // file node id = make_id("src/lib/foo.ts")
        assert!(
            has_node(&nodes, "src/lib/foo.ts"),
            "file node missing; nodes: {:?}",
            nodes.iter().map(|n| &n.id).collect::<Vec<_>>()
        );
        // symbol node id = make_id("lib.foo_Foo")
        let sym_id = make_test_id(&format!("{}_Foo", test_file_stem("src/lib/foo.ts")));
        assert!(
            nodes.iter().any(|n| n.id == sym_id),
            "symbol node missing: {}; nodes: {:?}",
            sym_id,
            nodes.iter().map(|n| &n.id).collect::<Vec<_>>()
        );
    }

    // Test 2: file_stem helper
    #[test]
    fn test_js_file_stem_with_parent() {
        assert_eq!(test_file_stem("src/lib/foo.ts"), "lib.foo");
        assert_eq!(test_file_stem("src/routes/page.ts"), "routes.page");
        assert_eq!(test_file_stem("foo.ts"), "foo");
    }

    // Test 3: named import creates imports edge pointing to origin symbol
    #[test]
    fn test_ts_named_import_to_origin() {
        let dir = tmp();
        write(dir.path(), "src/lib/foo.ts", "export function Foo() {}");
        write(
            dir.path(),
            "src/routes/page.ts",
            "import { Foo } from '../lib/foo';\nexport function handler() { return Foo(); }",
        );
        let paths = vec![
            dir.path().join("src/lib/foo.ts"),
            dir.path().join("src/routes/page.ts"),
        ];
        let (_, edges) = run(dir.path(), &paths);
        assert!(
            has_symbol_edge(
                &edges,
                "src/routes/page.ts",
                "src/lib/foo.ts",
                "Foo",
                "imports"
            ),
            "imports edge missing; edges: {:?}",
            edges
                .iter()
                .map(|e| (&e.source, &e.target, &e.relation))
                .collect::<Vec<_>>()
        );
    }

    // Test 4: barrel re-export (named) - import through barrel resolves to origin
    #[test]
    fn test_ts_named_import_through_barrel() {
        let dir = tmp();
        write(dir.path(), "src/lib/foo.ts", "export function Foo() {}");
        write(
            dir.path(),
            "src/lib/index.ts",
            "export { Foo } from './foo';",
        );
        write(
            dir.path(),
            "src/routes/page.ts",
            "import { Foo } from '../lib/index';",
        );
        let paths = vec![
            dir.path().join("src/lib/foo.ts"),
            dir.path().join("src/lib/index.ts"),
            dir.path().join("src/routes/page.ts"),
        ];
        let (_, edges) = run(dir.path(), &paths);
        assert!(
            has_symbol_edge(
                &edges,
                "src/routes/page.ts",
                "src/lib/foo.ts",
                "Foo",
                "imports"
            ),
            "barrel import edge missing; edges: {:?}",
            edges
                .iter()
                .map(|e| (&e.source, &e.target, &e.relation))
                .collect::<Vec<_>>()
        );
    }

    // Test 5: star re-export barrel
    #[test]
    fn test_ts_star_export_barrel() {
        let dir = tmp();
        write(dir.path(), "src/lib/foo.ts", "export function Foo() {}");
        write(dir.path(), "src/lib/index.ts", "export * from './foo';");
        write(
            dir.path(),
            "src/routes/page.ts",
            "import { Foo } from '../lib/index';",
        );
        let paths = vec![
            dir.path().join("src/lib/foo.ts"),
            dir.path().join("src/lib/index.ts"),
            dir.path().join("src/routes/page.ts"),
        ];
        let (_, edges) = run(dir.path(), &paths);
        assert!(
            has_symbol_edge(
                &edges,
                "src/routes/page.ts",
                "src/lib/foo.ts",
                "Foo",
                "imports"
            ),
            "star barrel import edge missing; edges: {:?}",
            edges
                .iter()
                .map(|e| (&e.source, &e.target, &e.relation))
                .collect::<Vec<_>>()
        );
    }

    // Test 6: re_exports edge emitted for barrel
    #[test]
    fn test_ts_barrel_re_exports_edge() {
        let dir = tmp();
        write(dir.path(), "src/lib/foo.ts", "export function Foo() {}");
        write(
            dir.path(),
            "src/lib/index.ts",
            "export { Foo } from './foo';",
        );
        let paths = vec![
            dir.path().join("src/lib/foo.ts"),
            dir.path().join("src/lib/index.ts"),
        ];
        let (_, edges) = run(dir.path(), &paths);
        assert!(
            has_edge(&edges, "src/lib/index.ts", "src/lib/foo.ts", "re_exports"),
            "re_exports edge missing; edges: {:?}",
            edges
                .iter()
                .map(|e| (&e.source, &e.target, &e.relation))
                .collect::<Vec<_>>()
        );
    }

    // Test 7: star re_exports edge
    #[test]
    fn test_ts_star_re_exports_edge() {
        let dir = tmp();
        write(dir.path(), "src/lib/foo.ts", "export function Foo() {}");
        write(dir.path(), "src/lib/index.ts", "export * from './foo';");
        let paths = vec![
            dir.path().join("src/lib/foo.ts"),
            dir.path().join("src/lib/index.ts"),
        ];
        let (_, edges) = run(dir.path(), &paths);
        assert!(
            has_edge(&edges, "src/lib/index.ts", "src/lib/foo.ts", "re_exports"),
            "star re_exports edge missing; edges: {:?}",
            edges
                .iter()
                .map(|e| (&e.source, &e.target, &e.relation))
                .collect::<Vec<_>>()
        );
    }

    // Test 8: TypeScript class with inherits creates symbol + edge
    #[test]
    fn test_ts_class_inherits() {
        let dir = tmp();
        write(dir.path(), "src/lib/base.ts", "export class Base {}");
        write(
            dir.path(),
            "src/lib/impl.ts",
            "import { Base } from './base';\nexport class DataProcessor extends Base {}",
        );
        let paths = vec![
            dir.path().join("src/lib/base.ts"),
            dir.path().join("src/lib/impl.ts"),
        ];
        let (_, edges) = run(dir.path(), &paths);
        assert!(
            has_symbol_to_symbol_edge(
                &edges,
                "src/lib/impl.ts",
                "DataProcessor",
                "src/lib/base.ts",
                "Base",
                "inherits"
            ),
            "inherits edge missing; edges: {:?}",
            edges
                .iter()
                .map(|e| (&e.source, &e.target, &e.relation))
                .collect::<Vec<_>>()
        );
    }

    // Test 9: TypeScript class with implements creates edge
    #[test]
    fn test_ts_class_implements() {
        let dir = tmp();
        write(
            dir.path(),
            "src/lib/iface.ts",
            "export interface IProcessor {}",
        );
        write(
            dir.path(),
            "src/lib/impl.ts",
            "import { IProcessor } from './iface';\nexport class DataProcessor implements IProcessor {}",
        );
        let paths = vec![
            dir.path().join("src/lib/iface.ts"),
            dir.path().join("src/lib/impl.ts"),
        ];
        let (_, edges) = run(dir.path(), &paths);
        assert!(
            has_symbol_to_symbol_edge(
                &edges,
                "src/lib/impl.ts",
                "DataProcessor",
                "src/lib/iface.ts",
                "IProcessor",
                "implements"
            ),
            "implements edge missing; edges: {:?}",
            edges
                .iter()
                .map(|e| (&e.source, &e.target, &e.relation))
                .collect::<Vec<_>>()
        );
    }

    // Test 10: method parameter type annotation creates references edge
    #[test]
    fn test_ts_method_parameter_type_ref() {
        let dir = tmp();
        write(dir.path(), "src/lib/types.ts", "export interface Config {}");
        write(
            dir.path(),
            "src/lib/impl.ts",
            "import { Config } from './types';\nexport class DataProcessor {\n  run(cfg: Config): void {}\n}",
        );
        let paths = vec![
            dir.path().join("src/lib/types.ts"),
            dir.path().join("src/lib/impl.ts"),
        ];
        let (_, edges) = run(dir.path(), &paths);
        // method node id = make_id("lib.impl_DataProcessor_run")
        let method_id = make_test_id("lib_impl_DataProcessor_run");
        let cfg_id = make_test_id(&format!("{}_Config", test_file_stem("src/lib/types.ts")));
        assert!(
            edges
                .iter()
                .any(|e| e.source == method_id && e.target == cfg_id && e.relation == "references"),
            "references edge missing; edges: {:?}",
            edges
                .iter()
                .map(|e| (&e.source, &e.target, &e.relation))
                .collect::<Vec<_>>()
        );
    }

    // Test 11: method return type creates references edge
    #[test]
    fn test_ts_method_return_type_ref() {
        let dir = tmp();
        write(dir.path(), "src/lib/types.ts", "export interface Result {}");
        write(
            dir.path(),
            "src/lib/impl.ts",
            "import { Result } from './types';\nexport class DataProcessor {\n  run(): Result {}\n}",
        );
        let paths = vec![
            dir.path().join("src/lib/types.ts"),
            dir.path().join("src/lib/impl.ts"),
        ];
        let (_, edges) = run(dir.path(), &paths);
        let method_id = make_test_id("lib_impl_DataProcessor_run");
        let result_id = make_test_id(&format!("{}_Result", test_file_stem("src/lib/types.ts")));
        assert!(
            edges.iter().any(|e| e.source == method_id
                && e.target == result_id
                && e.relation == "references"),
            "return type references edge missing; edges: {:?}",
            edges
                .iter()
                .map(|e| (&e.source, &e.target, &e.relation))
                .collect::<Vec<_>>()
        );
    }

    // Test 12: arrow function call through barrel resolves to origin symbol
    #[test]
    fn test_ts_arrow_function_call_through_barrel_targets_origin_symbol() {
        let dir = tmp();
        write(dir.path(), "src/lib/foo.ts", "export function Foo() {}");
        write(
            dir.path(),
            "src/lib/index.ts",
            "export { Foo } from './foo';",
        );
        write(
            dir.path(),
            "src/routes/page.ts",
            "import { Foo } from '../lib/index';\nconst X = () => Foo();",
        );
        let paths = vec![
            dir.path().join("src/lib/foo.ts"),
            dir.path().join("src/lib/index.ts"),
            dir.path().join("src/routes/page.ts"),
        ];
        let (_, edges) = run(dir.path(), &paths);
        assert!(
            has_symbol_to_symbol_edge(
                &edges,
                "src/routes/page.ts",
                "X",
                "src/lib/foo.ts",
                "Foo",
                "calls"
            ),
            "calls edge missing; edges: {:?}",
            edges
                .iter()
                .map(|e| (&e.source, &e.target, &e.relation))
                .collect::<Vec<_>>()
        );
    }

    // Test 13: regular function call through direct import
    #[test]
    fn test_ts_function_call_direct_import() {
        let dir = tmp();
        write(dir.path(), "src/lib/foo.ts", "export function Foo() {}");
        write(
            dir.path(),
            "src/routes/page.ts",
            "import { Foo } from '../lib/foo';\nexport function handler() { return Foo(); }",
        );
        let paths = vec![
            dir.path().join("src/lib/foo.ts"),
            dir.path().join("src/routes/page.ts"),
        ];
        let (_, edges) = run(dir.path(), &paths);
        assert!(
            has_symbol_to_symbol_edge(
                &edges,
                "src/routes/page.ts",
                "handler",
                "src/lib/foo.ts",
                "Foo",
                "calls"
            ),
            "calls edge missing; edges: {:?}",
            edges
                .iter()
                .map(|e| (&e.source, &e.target, &e.relation))
                .collect::<Vec<_>>()
        );
    }

    // Test 14: star barrel call resolves to origin
    #[test]
    fn test_ts_function_call_through_star_barrel() {
        let dir = tmp();
        write(dir.path(), "src/lib/foo.ts", "export function Foo() {}");
        write(dir.path(), "src/lib/index.ts", "export * from './foo';");
        write(
            dir.path(),
            "src/routes/page.ts",
            "import { Foo } from '../lib/index';\nexport function handler() { return Foo(); }",
        );
        let paths = vec![
            dir.path().join("src/lib/foo.ts"),
            dir.path().join("src/lib/index.ts"),
            dir.path().join("src/routes/page.ts"),
        ];
        let (_, edges) = run(dir.path(), &paths);
        assert!(
            has_symbol_to_symbol_edge(
                &edges,
                "src/routes/page.ts",
                "handler",
                "src/lib/foo.ts",
                "Foo",
                "calls"
            ),
            "calls edge missing; edges: {:?}",
            edges
                .iter()
                .map(|e| (&e.source, &e.target, &e.relation))
                .collect::<Vec<_>>()
        );
    }

    // Test 15: svelte.ts file import (tests path resolution with .svelte.ts extension)
    #[test]
    fn test_ts_svelte_ts_import() {
        let dir = tmp();
        write(
            dir.path(),
            "src/lib/hooks/is-mobile.svelte.ts",
            "export function isMobile() { return false; }",
        );
        write(
            dir.path(),
            "src/routes/page.ts",
            "import { isMobile } from '../lib/hooks/is-mobile.svelte';",
        );
        let paths = vec![
            dir.path().join("src/lib/hooks/is-mobile.svelte.ts"),
            dir.path().join("src/routes/page.ts"),
        ];
        let (_, edges) = run(dir.path(), &paths);
        // The import edge: src/routes/page.ts → isMobile in src/lib/hooks/is-mobile.svelte.ts
        // target file stem: "hooks.is-mobile.svelte" (parent=hooks, stem=is-mobile.svelte)
        // Actually file_stem("src/lib/hooks/is-mobile.svelte.ts"):
        //   stem = "is-mobile.svelte" (file_stem strips last extension only)
        //   parent.name = "hooks"
        //   result = "hooks.is-mobile.svelte"
        // symbol id = make_id("hooks.is-mobile.svelte_isMobile") = "hooks_is_mobile_svelte_ismobile"
        let stem = test_file_stem("src/lib/hooks/is-mobile.svelte.ts");
        let sym_id = make_test_id(&format!("{}_isMobile", stem));
        let src_id = make_test_id("src/routes/page.ts");
        assert!(
            edges
                .iter()
                .any(|e| e.source == src_id && e.target == sym_id && e.relation == "imports"),
            "svelte.ts import edge missing; stem={}, sym_id={}, edges: {:?}",
            stem,
            sym_id,
            edges
                .iter()
                .map(|e| (&e.source, &e.target, &e.relation))
                .collect::<Vec<_>>()
        );
    }

    // Test 16: multiple exports from same file
    #[test]
    fn test_ts_multiple_exports() {
        let dir = tmp();
        write(
            dir.path(),
            "src/lib/foo.ts",
            "export function Foo() {}\nexport function Bar() {}",
        );
        let paths = vec![dir.path().join("src/lib/foo.ts")];
        let (nodes, _) = run(dir.path(), &paths);
        let foo_id = make_test_id(&format!("{}_Foo", test_file_stem("src/lib/foo.ts")));
        let bar_id = make_test_id(&format!("{}_Bar", test_file_stem("src/lib/foo.ts")));
        assert!(nodes.iter().any(|n| n.id == foo_id), "Foo node missing");
        assert!(nodes.iter().any(|n| n.id == bar_id), "Bar node missing");
    }

    // Test 17: deep chain barrel (A → B → C)
    #[test]
    fn test_ts_deep_chain_barrel() {
        let dir = tmp();
        write(dir.path(), "src/lib/foo.ts", "export function Foo() {}");
        write(
            dir.path(),
            "src/lib/index.ts",
            "export { Foo } from './foo';",
        );
        write(
            dir.path(),
            "src/routes/index.ts",
            "export { Foo } from '../lib/index';",
        );
        write(
            dir.path(),
            "src/app/main.ts",
            "import { Foo } from '../routes/index';",
        );
        let paths = vec![
            dir.path().join("src/lib/foo.ts"),
            dir.path().join("src/lib/index.ts"),
            dir.path().join("src/routes/index.ts"),
            dir.path().join("src/app/main.ts"),
        ];
        let (_, edges) = run(dir.path(), &paths);
        assert!(
            has_symbol_edge(
                &edges,
                "src/app/main.ts",
                "src/lib/foo.ts",
                "Foo",
                "imports"
            ),
            "deep chain barrel imports edge missing; edges: {:?}",
            edges
                .iter()
                .map(|e| (&e.source, &e.target, &e.relation))
                .collect::<Vec<_>>()
        );
    }

    // Test 18: interface declaration creates symbol node
    #[test]
    fn test_ts_interface_declaration_node() {
        let dir = tmp();
        write(
            dir.path(),
            "src/lib/types.ts",
            "export interface Config { key: string; }",
        );
        let paths = vec![dir.path().join("src/lib/types.ts")];
        let (nodes, _) = run(dir.path(), &paths);
        let sym_id = make_test_id(&format!("{}_Config", test_file_stem("src/lib/types.ts")));
        assert!(
            nodes.iter().any(|n| n.id == sym_id),
            "Config interface node missing; nodes: {:?}",
            nodes.iter().map(|n| &n.id).collect::<Vec<_>>()
        );
    }

    // Test 19: type alias creates symbol node
    #[test]
    fn test_ts_type_alias_node() {
        let dir = tmp();
        write(
            dir.path(),
            "src/lib/types.ts",
            "export type MyType = string | number;",
        );
        let paths = vec![dir.path().join("src/lib/types.ts")];
        let (nodes, _) = run(dir.path(), &paths);
        let sym_id = make_test_id(&format!("{}_MyType", test_file_stem("src/lib/types.ts")));
        assert!(
            nodes.iter().any(|n| n.id == sym_id),
            "MyType alias node missing; nodes: {:?}",
            nodes.iter().map(|n| &n.id).collect::<Vec<_>>()
        );
    }

    // Test 20: JS file (not TS) import resolution
    #[test]
    fn test_js_import_resolution() {
        let dir = tmp();
        write(dir.path(), "src/lib/foo.js", "export function Foo() {}");
        write(
            dir.path(),
            "src/routes/page.js",
            "import { Foo } from '../lib/foo.js';\nexport function handler() { return Foo(); }",
        );
        let paths = vec![
            dir.path().join("src/lib/foo.js"),
            dir.path().join("src/routes/page.js"),
        ];
        let (_, edges) = run(dir.path(), &paths);
        assert!(
            has_symbol_edge(
                &edges,
                "src/routes/page.js",
                "src/lib/foo.js",
                "Foo",
                "imports"
            ),
            "JS imports edge missing; edges: {:?}",
            edges
                .iter()
                .map(|e| (&e.source, &e.target, &e.relation))
                .collect::<Vec<_>>()
        );
    }

    // Test 21: method nodes are created for class methods
    #[test]
    fn test_ts_method_nodes_created() {
        let dir = tmp();
        write(
            dir.path(),
            "src/lib/impl.ts",
            "export class DataProcessor {\n  run(): void {}\n  stop(): void {}\n}",
        );
        let paths = vec![dir.path().join("src/lib/impl.ts")];
        let (nodes, _) = run(dir.path(), &paths);
        let class_nid = make_test_id(&format!(
            "{}_DataProcessor",
            test_file_stem("src/lib/impl.ts")
        ));
        let run_id = make_test_id(&format!("{}_run", class_nid));
        let stop_id = make_test_id(&format!("{}_stop", class_nid));
        assert!(
            nodes.iter().any(|n| n.id == run_id),
            "run method node missing; nodes: {:?}",
            nodes.iter().map(|n| &n.id).collect::<Vec<_>>()
        );
        assert!(
            nodes.iter().any(|n| n.id == stop_id),
            "stop method node missing; nodes: {:?}",
            nodes.iter().map(|n| &n.id).collect::<Vec<_>>()
        );
    }
}
