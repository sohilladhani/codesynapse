use crate::extract::make_id;
use crate::types::{Edge, Node};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

struct FileFact {
    rel_path: String,
    file_id: String,
}

struct DeclFact {
    file_rel: String,
    name: String,
    node_id: String,
}

struct ImportFact {
    importer_rel: String,
    module: String,
    imported_name: String,
    local_name: String,
    is_relative: bool,
    dot_level: usize,
}

struct TypeAnnotFact {
    fn_rel: String,
    fn_name: String,
    type_name: String,
    context: String,
}

struct CallFact {
    caller_rel: String,
    caller_fn: String,
    callee_name: String,
}

fn ntext<'a>(source: &'a [u8], node: &tree_sitter::Node) -> &'a str {
    std::str::from_utf8(&source[node.start_byte()..node.end_byte()]).unwrap_or("")
}

fn make_file_id(rel_path: &str) -> String {
    make_id(&[rel_path])
}

fn make_sym_id(file_rel: &str, name: &str, is_fn: bool) -> String {
    if is_fn {
        make_id(&[file_rel, name, "()"])
    } else {
        make_id(&[file_rel, name])
    }
}

fn walk_all(node: tree_sitter::Node) -> Vec<tree_sitter::Node> {
    let mut stack = vec![node];
    let mut out = Vec::new();
    while let Some(n) = stack.pop() {
        out.push(n);
        let mut c = n.walk();
        for child in n.children(&mut c) {
            stack.push(child);
        }
    }
    out
}

fn make_code_node(id: String, label: String, rel_path: &str) -> Node {
    Node {
        id,
        label,
        file_type: "code".to_string(),
        source_file: rel_path.to_string(),
        source_location: None,
        community: None,
        rationale: None,
        docstring: None,
        metadata: HashMap::new(),
    }
}

#[allow(clippy::too_many_arguments)]
fn parse_file(
    source: &[u8],
    rel_path: &str,
    file_facts: &mut Vec<FileFact>,
    decl_facts: &mut Vec<DeclFact>,
    import_facts: &mut Vec<ImportFact>,
    type_annot_facts: &mut Vec<TypeAnnotFact>,
    call_facts: &mut Vec<CallFact>,
    all_nodes: &mut Vec<Node>,
) {
    let mut parser = tree_sitter::Parser::new();
    let lang: tree_sitter::Language = tree_sitter_python::LANGUAGE.into();
    if parser.set_language(&lang).is_err() {
        return;
    }
    let tree = match parser.parse(source, None) {
        Some(t) => t,
        None => return,
    };

    let file_id = make_file_id(rel_path);
    let file_label = Path::new(rel_path)
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    file_facts.push(FileFact {
        rel_path: rel_path.to_string(),
        file_id: file_id.clone(),
    });
    all_nodes.push(make_code_node(file_id.clone(), file_label, rel_path));

    let root = tree.root_node();

    for node in walk_all(root) {
        match node.kind() {
            "function_definition" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = ntext(source, &name_node).to_string();
                    let node_id = make_sym_id(rel_path, &name, true);
                    decl_facts.push(DeclFact {
                        file_rel: rel_path.to_string(),
                        name: name.clone(),
                        node_id: node_id.clone(),
                    });
                    all_nodes.push(make_code_node(node_id, format!("{}()", name), rel_path));

                    // Collect type annotations from parameters
                    if let Some(params) = node.child_by_field_name("parameters") {
                        let mut pc = params.walk();
                        for param in params.children(&mut pc) {
                            if param.kind() == "typed_parameter"
                                || param.kind() == "typed_default_parameter"
                            {
                                if let Some(type_node) = param.child_by_field_name("type") {
                                    collect_type_refs(
                                        source,
                                        type_node,
                                        &name,
                                        rel_path,
                                        "parameter_type",
                                        "generic_arg",
                                        type_annot_facts,
                                    );
                                }
                            }
                        }
                    }

                    // Collect return type
                    if let Some(ret_node) = node.child_by_field_name("return_type") {
                        collect_type_refs(
                            source,
                            ret_node,
                            &name,
                            rel_path,
                            "return_type",
                            "return_generic_arg",
                            type_annot_facts,
                        );
                    }

                    // Collect calls in body
                    if let Some(body) = node.child_by_field_name("body") {
                        for bn in walk_all(body) {
                            if bn.kind() == "call" {
                                if let Some(fn_node) = bn.child_by_field_name("function") {
                                    if fn_node.kind() == "identifier" {
                                        let callee = ntext(source, &fn_node).to_string();
                                        call_facts.push(CallFact {
                                            caller_rel: rel_path.to_string(),
                                            caller_fn: name.clone(),
                                            callee_name: callee,
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
            }
            "class_definition" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = ntext(source, &name_node).to_string();
                    let node_id = make_sym_id(rel_path, &name, false);
                    decl_facts.push(DeclFact {
                        file_rel: rel_path.to_string(),
                        name: name.clone(),
                        node_id: node_id.clone(),
                    });
                    all_nodes.push(make_code_node(node_id, name, rel_path));
                }
            }
            "import_from_statement" => {
                let module_node = match node.child_by_field_name("module_name") {
                    Some(n) => n,
                    None => continue,
                };
                let is_relative = module_node.kind() == "relative_import";
                let (dot_level, module_name) = if is_relative {
                    let mut dots = 0usize;
                    let mut mod_name = String::new();
                    let mut mc = module_node.walk();
                    for child in module_node.children(&mut mc) {
                        match child.kind() {
                            "import_prefix" => {
                                dots = ntext(source, &child).chars().filter(|&c| c == '.').count();
                            }
                            "dotted_name" => {
                                mod_name = ntext(source, &child).to_string();
                            }
                            _ => {}
                        }
                    }
                    (dots, mod_name)
                } else {
                    (0, ntext(source, &module_node).to_string())
                };

                let module_id = module_node.id();
                let mut nc = node.walk();
                for child in node.children(&mut nc) {
                    if child.id() == module_id {
                        continue;
                    }
                    match child.kind() {
                        "dotted_name" => {
                            let imported = ntext(source, &child).to_string();
                            import_facts.push(ImportFact {
                                importer_rel: rel_path.to_string(),
                                module: module_name.clone(),
                                imported_name: imported.clone(),
                                local_name: imported,
                                is_relative,
                                dot_level,
                            });
                        }
                        "aliased_import" => {
                            let orig = child
                                .child_by_field_name("name")
                                .map(|n| ntext(source, &n).to_string())
                                .unwrap_or_default();
                            // name field on aliased_import is dotted_name; text is the full thing
                            // Get just the last identifier
                            let orig_simple =
                                orig.split('.').next_back().unwrap_or(&orig).to_string();
                            let alias = child
                                .child_by_field_name("alias")
                                .map(|n| ntext(source, &n).to_string())
                                .unwrap_or_else(|| orig_simple.clone());
                            import_facts.push(ImportFact {
                                importer_rel: rel_path.to_string(),
                                module: module_name.clone(),
                                imported_name: orig_simple,
                                local_name: alias,
                                is_relative,
                                dot_level,
                            });
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
}

fn collect_type_refs(
    source: &[u8],
    type_node: tree_sitter::Node,
    fn_name: &str,
    rel_path: &str,
    simple_ctx: &str,
    generic_ctx: &str,
    facts: &mut Vec<TypeAnnotFact>,
) {
    let mut tc = type_node.walk();
    for child in type_node.children(&mut tc) {
        if !child.is_named() {
            continue;
        }
        match child.kind() {
            "identifier" => {
                let name = ntext(source, &child);
                if !is_builtin(name) {
                    facts.push(TypeAnnotFact {
                        fn_rel: rel_path.to_string(),
                        fn_name: fn_name.to_string(),
                        type_name: name.to_string(),
                        context: simple_ctx.to_string(),
                    });
                }
            }
            "generic_type" => {
                // generic_type: identifier (outer) + type_parameter (args)
                let mut gc = child.walk();
                for gchild in child.children(&mut gc) {
                    if gchild.kind() == "type_parameter" {
                        let mut tpc = gchild.walk();
                        for tnode in gchild.children(&mut tpc) {
                            if tnode.kind() == "type" {
                                collect_type_refs(
                                    source,
                                    tnode,
                                    fn_name,
                                    rel_path,
                                    generic_ctx,
                                    generic_ctx,
                                    facts,
                                );
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

fn is_builtin(name: &str) -> bool {
    matches!(
        name,
        "int"
            | "str"
            | "float"
            | "bool"
            | "bytes"
            | "None"
            | "list"
            | "dict"
            | "set"
            | "tuple"
            | "Any"
            | "Optional"
            | "Union"
            | "List"
            | "Dict"
            | "Set"
            | "Tuple"
            | "Type"
            | "Callable"
            | "Iterable"
            | "Iterator"
    )
}

fn resolve_relative(importer_rel: &str, dots: usize, module: &str) -> String {
    let path = Path::new(importer_rel);
    let mut dir = path.parent().unwrap_or(Path::new("")).to_path_buf();
    for _ in 1..dots {
        dir = dir.parent().unwrap_or(Path::new("")).to_path_buf();
    }
    if module.is_empty() {
        dir.join("__init__.py").to_string_lossy().replace('\\', "/")
    } else {
        dir.join(format!("{}.py", module.replace('.', "/")))
            .to_string_lossy()
            .replace('\\', "/")
    }
}

pub fn extract_python_files(paths: &[PathBuf], root: &Path) -> (Vec<Node>, Vec<Edge>) {
    let mut file_facts: Vec<FileFact> = Vec::new();
    let mut decl_facts: Vec<DeclFact> = Vec::new();
    let mut import_facts: Vec<ImportFact> = Vec::new();
    let mut type_annot_facts: Vec<TypeAnnotFact> = Vec::new();
    let mut call_facts: Vec<CallFact> = Vec::new();
    let mut all_nodes: Vec<Node> = Vec::new();

    for path in paths {
        let source = match std::fs::read(path) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let rel = path.strip_prefix(root).unwrap_or(path);
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        parse_file(
            &source,
            &rel_str,
            &mut file_facts,
            &mut decl_facts,
            &mut import_facts,
            &mut type_annot_facts,
            &mut call_facts,
            &mut all_nodes,
        );
    }

    let file_id_map: HashMap<String, String> = file_facts
        .iter()
        .map(|f| (f.rel_path.clone(), f.file_id.clone()))
        .collect();

    // (file_rel, symbol_name) → node_id
    let mut symbol_map: HashMap<(String, String), String> = HashMap::new();
    for d in &decl_facts {
        symbol_map.insert((d.file_rel.clone(), d.name.clone()), d.node_id.clone());
    }

    let known_files: Vec<String> = file_facts.iter().map(|f| f.rel_path.clone()).collect();

    let mut edges: Vec<Edge> = Vec::new();

    // (barrel_file_rel, local_name) → (origin_file_rel, origin_name)
    let mut re_export_map: HashMap<(String, String), (String, String)> = HashMap::new();

    // Pass 1: handle relative imports from __init__.py (re-exports)
    for imp in &import_facts {
        if !imp.is_relative {
            continue;
        }
        let target_file = resolve_relative(&imp.importer_rel, imp.dot_level, &imp.module);
        let is_barrel = imp.importer_rel.ends_with("__init__.py");

        if is_barrel {
            if let (Some(barrel_id), Some(origin_id)) = (
                file_id_map.get(&imp.importer_rel),
                file_id_map.get(&target_file),
            ) {
                // Only emit re_exports once per barrel→target pair
                let already = edges.iter().any(|e| {
                    e.source == *barrel_id && e.target == *origin_id && e.relation == "re_exports"
                });
                if !already {
                    edges.push(Edge {
                        source: barrel_id.clone(),
                        target: origin_id.clone(),
                        relation: "re_exports".to_string(),
                        confidence: "EXTRACTED".to_string(),
                        source_file: Some(imp.importer_rel.clone()),
                        weight: 1.0,
                        context: None,
                    });
                }
            }
            re_export_map.insert(
                (imp.importer_rel.clone(), imp.local_name.clone()),
                (target_file, imp.imported_name.clone()),
            );
        }
    }

    // Pass 2: build import_resolution (all imports → origin symbol)
    // (importer_rel, local_name) → (origin_file_rel, origin_name)
    let mut import_resolution: HashMap<(String, String), (String, String)> = HashMap::new();

    for imp in &import_facts {
        if imp.is_relative {
            let target_file = resolve_relative(&imp.importer_rel, imp.dot_level, &imp.module);
            import_resolution.insert(
                (imp.importer_rel.clone(), imp.local_name.clone()),
                (target_file, imp.imported_name.clone()),
            );
        } else {
            // Absolute: from pkg import X → look in pkg/__init__.py
            let pkg_init = format!("{}/__init__.py", imp.module.replace('.', "/"));
            if let Some(barrel_rel) = known_files.iter().find(|f| **f == pkg_init) {
                let key = (barrel_rel.clone(), imp.imported_name.clone());
                if let Some((origin_file, origin_name)) = re_export_map.get(&key) {
                    // consumer imports origin_symbol
                    if let (Some(consumer_id), Some(sym_id)) = (
                        file_id_map.get(&imp.importer_rel),
                        symbol_map.get(&(origin_file.clone(), origin_name.clone())),
                    ) {
                        edges.push(Edge {
                            source: consumer_id.clone(),
                            target: sym_id.clone(),
                            relation: "imports".to_string(),
                            confidence: "EXTRACTED".to_string(),
                            source_file: Some(imp.importer_rel.clone()),
                            weight: 1.0,
                            context: None,
                        });
                    }
                    import_resolution.insert(
                        (imp.importer_rel.clone(), imp.local_name.clone()),
                        (origin_file.clone(), origin_name.clone()),
                    );
                }
            }
        }
    }

    // Pass 3: calls
    for call in &call_facts {
        let caller_id = symbol_map.get(&(call.caller_rel.clone(), call.caller_fn.clone()));
        let key = (call.caller_rel.clone(), call.callee_name.clone());
        if let Some((origin_file, origin_name)) = import_resolution.get(&key) {
            let callee_id = symbol_map.get(&(origin_file.clone(), origin_name.clone()));
            if let (Some(cid), Some(eid)) = (caller_id, callee_id) {
                edges.push(Edge {
                    source: cid.clone(),
                    target: eid.clone(),
                    relation: "calls".to_string(),
                    confidence: "EXTRACTED".to_string(),
                    source_file: Some(call.caller_rel.clone()),
                    weight: 1.0,
                    context: None,
                });
            }
        }
    }

    // Pass 4: type annotation references
    for annot in &type_annot_facts {
        let fn_id = symbol_map.get(&(annot.fn_rel.clone(), annot.fn_name.clone()));
        let imp_key = (annot.fn_rel.clone(), annot.type_name.clone());
        let type_id = if let Some((origin_file, origin_name)) = import_resolution.get(&imp_key) {
            symbol_map
                .get(&(origin_file.clone(), origin_name.clone()))
                .cloned()
        } else {
            // Fallback: find any symbol with this name
            symbol_map
                .iter()
                .find(|((_, name), _)| name == &annot.type_name)
                .map(|(_, id)| id.clone())
        };

        if let (Some(fid), Some(tid)) = (fn_id, type_id) {
            edges.push(Edge {
                source: fid.clone(),
                target: tid.clone(),
                relation: "references".to_string(),
                confidence: "EXTRACTED".to_string(),
                source_file: Some(annot.fn_rel.clone()),
                weight: 1.0,
                context: Some(annot.context.clone()),
            });
        }
    }

    (all_nodes, edges)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn write(root: &Path, rel: &str, content: &str) -> PathBuf {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, content).unwrap();
        p
    }

    fn node_id_by(nodes: &[Node], label: &str, source_file: &str) -> String {
        nodes
            .iter()
            .find(|n| n.label == label && n.source_file == source_file)
            .unwrap_or_else(|| {
                panic!(
                    "node not found: label={:?} source_file={:?}\nnodes: {:?}",
                    label,
                    source_file,
                    nodes
                        .iter()
                        .map(|n| (&n.label, &n.source_file))
                        .collect::<Vec<_>>()
                )
            })
            .id
            .clone()
    }

    fn has_edge(edges: &[Edge], src: &str, tgt: &str, rel: &str) -> bool {
        edges
            .iter()
            .any(|e| e.source == src && e.target == tgt && e.relation == rel)
    }

    #[test]
    fn test_python_package_reexport_resolves_import_and_call_to_origin_symbol() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        write(root, "pkg/foo.py", "def Foo():\n    return 1\n");
        write(
            root,
            "pkg/__init__.py",
            "from .foo import Foo as PublicFoo\n",
        );
        write(
            root,
            "app.py",
            "from pkg import PublicFoo\n\ndef X():\n    return PublicFoo()\n",
        );

        let paths = vec![
            root.join("pkg/foo.py"),
            root.join("pkg/__init__.py"),
            root.join("app.py"),
        ];
        let (nodes, edges) = extract_python_files(&paths, root);

        let origin_file = node_id_by(&nodes, "foo.py", "pkg/foo.py");
        let barrel_file = node_id_by(&nodes, "__init__.py", "pkg/__init__.py");
        let consumer_file = node_id_by(&nodes, "app.py", "app.py");
        let origin_symbol = node_id_by(&nodes, "Foo()", "pkg/foo.py");
        let consumer_symbol = node_id_by(&nodes, "X()", "app.py");

        assert!(
            has_edge(&edges, &barrel_file, &origin_file, "re_exports"),
            "barrel→origin re_exports missing; edges: {:?}",
            edges
                .iter()
                .map(|e| (&e.source, &e.target, &e.relation))
                .collect::<Vec<_>>()
        );
        assert!(
            has_edge(&edges, &consumer_file, &origin_symbol, "imports"),
            "consumer_file→origin_symbol imports missing; edges: {:?}",
            edges
                .iter()
                .map(|e| (&e.source, &e.target, &e.relation))
                .collect::<Vec<_>>()
        );
        assert!(
            has_edge(&edges, &consumer_symbol, &origin_symbol, "calls"),
            "consumer_symbol→origin_symbol calls missing; edges: {:?}",
            edges
                .iter()
                .map(|e| (&e.source, &e.target, &e.relation))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_python_parameter_return_and_generic_contexts() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        write(
            root,
            "pkg/model.py",
            "class Payload:\n    pass\n\nclass Result:\n    pass\n",
        );
        write(
            root,
            "pkg/service.py",
            "from .model import Payload, Result\n\n\
             def process(item: Payload) -> Result:\n    return Result()\n\n\
             def process_many(items: list[Payload]) -> Result:\n    return Result()\n",
        );

        let paths = vec![root.join("pkg/model.py"), root.join("pkg/service.py")];
        let (nodes, edges) = extract_python_files(&paths, root);

        let labels: HashMap<String, String> = nodes
            .iter()
            .map(|n| (n.id.clone(), n.label.clone()))
            .collect();

        let pairs: HashSet<(String, String, Option<String>)> = edges
            .iter()
            .filter(|e| e.relation == "references")
            .map(|e| {
                (
                    labels
                        .get(&e.source)
                        .cloned()
                        .unwrap_or_else(|| e.source.clone()),
                    labels
                        .get(&e.target)
                        .cloned()
                        .unwrap_or_else(|| e.target.clone()),
                    e.context.clone(),
                )
            })
            .collect();

        assert!(
            pairs.contains(&(
                "process()".to_string(),
                "Payload".to_string(),
                Some("parameter_type".to_string())
            )),
            "process()→Payload(parameter_type) missing; pairs={:?}",
            pairs
        );
        assert!(
            pairs.contains(&(
                "process()".to_string(),
                "Result".to_string(),
                Some("return_type".to_string())
            )),
            "process()→Result(return_type) missing; pairs={:?}",
            pairs
        );
        assert!(
            pairs.contains(&(
                "process_many()".to_string(),
                "Payload".to_string(),
                Some("generic_arg".to_string())
            )),
            "process_many()→Payload(generic_arg) missing; pairs={:?}",
            pairs
        );
    }
}
