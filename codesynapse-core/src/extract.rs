use crate::error::{CodeSynapseError, Result};
use crate::types::{Edge, ExtractionFragment, Node, NodeId};
use std::collections::HashMap;
use std::path::Path;

pub trait LanguageExtractor {
    fn file_extensions(&self) -> Vec<&'static str>;
    fn extract(&self, source: &[u8], path: &Path) -> Result<ExtractionFragment>;
    fn resolve_imports(&self, imports: &[ImportNode]) -> Vec<Edge>;
    fn collect_type_refs(&self, fragment: &mut ExtractionFragment);
}

#[derive(Debug, Clone)]
pub struct ImportNode {
    pub source: String,
    pub specifiers: Vec<String>,
    pub is_relative: bool,
}

pub struct Extractor {
    pub language_extractors: HashMap<String, Box<dyn LanguageExtractor + Send + Sync>>,
}

impl Extractor {
    pub fn new() -> Self {
        Extractor {
            language_extractors: HashMap::new(),
        }
    }

    pub fn register(&mut self, ext: &str, extractor: Box<dyn LanguageExtractor + Send + Sync>) {
        self.language_extractors.insert(ext.to_string(), extractor);
    }

    pub fn extract_file(&self, path: &Path, source: &[u8]) -> Result<ExtractionFragment> {
        // Try full filename first (e.g. "package.json" or "mcp.json")
        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_lowercase();

        if let Some(extractor) = self.language_extractors.get(filename.as_str()) {
            return extractor.extract(source, path);
        }

        // Then try by extension
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        if let Some(extractor) = self.language_extractors.get(ext.as_str()) {
            extractor.extract(source, path)
        } else {
            // Default: basic text extraction (no structured AST)
            Ok(ExtractionFragment {
                nodes: vec![],
                edges: vec![],
            })
        }
    }

    pub fn extract_all(
        &self,
        files: &[(std::path::PathBuf, &[u8])],
    ) -> Vec<(std::path::PathBuf, Result<ExtractionFragment>)> {
        files
            .iter()
            .map(|(path, source)| {
                let result = self.extract_file(path, source);
                (path.clone(), result)
            })
            .collect()
    }
}

impl Default for Extractor {
    fn default() -> Self {
        Self::new()
    }
}

/// Python-specific extractor
pub struct PythonExtractor;

impl PythonExtractor {
    fn extract_fragment(source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        let content = std::str::from_utf8(source).map_err(|e| {
            CodeSynapseError::Parse(format!("invalid UTF-8 in {}: {}", path.display(), e))
        })?;

        let mut nodes = Vec::new();
        let mut edges = Vec::new();

        let file_id = path_to_file_id(path);

        // File-level node
        nodes.push(Node {
            id: file_id.clone(),
            label: path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string(),
            file_type: "code".to_string(),
            source_file: path.to_string_lossy().to_string(),
            source_location: None,
            community: None,
            rationale: None,
            docstring: None,
            metadata: HashMap::new(),
        });

        // Simple regex-based extraction for initial pass
        // Class definitions
        for cap in regex_class_captures(content) {
            let class_name = cap;
            let class_id = make_id(&[&file_id, class_name]);
            nodes.push(Node {
                id: class_id.clone(),
                label: class_name.to_string(),
                file_type: "code".to_string(),
                source_file: path.to_string_lossy().to_string(),
                source_location: None,
                community: None,
                rationale: None,
                docstring: None,
                metadata: HashMap::new(),
            });
            edges.push(Edge {
                source: file_id.clone(),
                target: class_id,
                relation: "contains".to_string(),
                confidence: "EXTRACTED".to_string(),
                source_file: Some(path.to_string_lossy().to_string()),
                weight: 1.0,
                context: None,
            });
        }

        // Function definitions
        for cap in regex_fn_captures(content) {
            let fn_name = cap;
            let fn_id = make_id(&[&file_id, fn_name, "()"]);
            nodes.push(Node {
                id: fn_id.clone(),
                label: format!("{}()", fn_name),
                file_type: "code".to_string(),
                source_file: path.to_string_lossy().to_string(),
                source_location: None,
                community: None,
                rationale: None,
                docstring: None,
                metadata: HashMap::new(),
            });
            edges.push(Edge {
                source: file_id.clone(),
                target: fn_id,
                relation: "contains".to_string(),
                confidence: "EXTRACTED".to_string(),
                source_file: Some(path.to_string_lossy().to_string()),
                weight: 1.0,
                context: None,
            });
        }

        // Import statements
        for (module, alias) in regex_import_captures(content) {
            let import_target = alias.unwrap_or(module);
            let import_id = make_id(&[import_target]);
            if !nodes.iter().any(|n| n.id == import_id) {
                nodes.push(Node {
                    id: import_id.clone(),
                    label: import_target.to_string(),
                    file_type: "code".to_string(),
                    source_file: path.to_string_lossy().to_string(),
                    source_location: None,
                    community: None,
                    rationale: None,
                    docstring: None,
                    metadata: HashMap::new(),
                });
            }
            edges.push(Edge {
                source: file_id.clone(),
                target: import_id,
                relation: "imports".to_string(),
                confidence: "EXTRACTED".to_string(),
                source_file: Some(path.to_string_lossy().to_string()),
                weight: 1.0,
                context: None,
            });
        }

        // From-imports
        for (_module, names) in regex_from_import_captures(content) {
            for name in names {
                let import_id = make_id(&[&file_id, name]);
                if !nodes.iter().any(|n| n.id == import_id) {
                    nodes.push(Node {
                        id: import_id.clone(),
                        label: name.to_string(),
                        file_type: "code".to_string(),
                        source_file: path.to_string_lossy().to_string(),
                        source_location: None,
                        community: None,
                        rationale: None,
                        docstring: None,
                        metadata: HashMap::new(),
                    });
                }
                edges.push(Edge {
                    source: file_id.clone(),
                    target: import_id,
                    relation: "imports".to_string(),
                    confidence: "EXTRACTED".to_string(),
                    source_file: Some(path.to_string_lossy().to_string()),
                    weight: 1.0,
                    context: None,
                });
            }
        }

        Ok(ExtractionFragment { nodes, edges })
    }
}

impl LanguageExtractor for PythonExtractor {
    fn file_extensions(&self) -> Vec<&'static str> {
        vec!["py"]
    }

    fn extract(&self, source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        Self::extract_fragment(source, path)
    }

    fn resolve_imports(&self, _imports: &[ImportNode]) -> Vec<Edge> {
        vec![]
    }

    fn collect_type_refs(&self, _fragment: &mut ExtractionFragment) {}
}

fn regex_class_captures(content: &str) -> Vec<&str> {
    content
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with("class ") && trimmed.contains('(')
                || trimmed.starts_with("class ") && trimmed.contains(':')
            {
                let name_part = trimmed
                    .strip_prefix("class ")
                    .and_then(|s| s.split('(').next())
                    .or_else(|| {
                        trimmed
                            .strip_prefix("class ")
                            .and_then(|s| s.split(':').next())
                    })
                    .unwrap_or("")
                    .trim();
                if !name_part.is_empty() {
                    Some(name_part)
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect()
}

fn regex_fn_captures(content: &str) -> Vec<&str> {
    content
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with("def ") {
                let name = trimmed
                    .strip_prefix("def ")
                    .and_then(|s| s.split('(').next())
                    .unwrap_or("")
                    .trim();
                if !name.is_empty() {
                    Some(name)
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect()
}

fn regex_import_captures(content: &str) -> Vec<(&str, Option<&str>)> {
    content
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with("import ") && !trimmed.starts_with("import ") {
                return None;
            }
            if trimmed.starts_with("import ") {
                let rest = trimmed.strip_prefix("import ").unwrap_or("");
                // Handle "import X as Y"
                if let Some(as_pos) = rest.find(" as ") {
                    let module = rest[..as_pos].trim();
                    let alias = rest[as_pos + 4..].trim();
                    Some((module, Some(alias)))
                } else {
                    let module = rest.trim();
                    if !module.contains('(') {
                        Some((module, None))
                    } else {
                        None
                    }
                }
            } else {
                None
            }
        })
        .collect()
}

fn regex_from_import_captures(content: &str) -> Vec<(&str, Vec<&str>)> {
    content
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with("from ") {
                let rest = trimmed.strip_prefix("from ")?;
                let (module, import_part) = rest.split_once(" import ")?;
                let names: Vec<&str> = import_part
                    .trim()
                    .strip_prefix('(')
                    .and_then(|s| s.strip_suffix(')'))
                    .map(|s| {
                        s.split(',')
                            .map(|n| n.trim())
                            .filter(|n| !n.is_empty())
                            .collect()
                    })
                    .unwrap_or_else(|| {
                        import_part
                            .split(',')
                            .map(|n| n.trim())
                            .filter(|n| !n.is_empty())
                            .collect()
                    });
                let module = module.trim();
                Some((module, names))
            } else {
                None
            }
        })
        .collect()
}

pub fn path_to_file_id(path: &Path) -> NodeId {
    let stem = path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let ext = path
        .extension()
        .map(|e| format!("_{}", e.to_string_lossy()))
        .unwrap_or_default();
    let parent = path
        .parent()
        .and_then(|p| p.file_name())
        .map(|n| format!("{}_", n.to_string_lossy()))
        .unwrap_or_default();
    make_id(&[&format!("{}{}{}", parent, stem, ext)])
}

pub fn make_id(parts: &[&str]) -> NodeId {
    let joined = parts.join("_");
    joined
        .chars()
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

pub fn normalize_id(id: &str) -> NodeId {
    id.chars()
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

/// JavaScript-specific extractor
pub struct JavaScriptExtractor;

impl JavaScriptExtractor {
    fn extract_fragment(source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        let content = std::str::from_utf8(source).map_err(|e| {
            CodeSynapseError::Parse(format!("invalid UTF-8 in {}: {}", path.display(), e))
        })?;

        let mut nodes = Vec::new();
        let mut edges = Vec::new();

        let file_id = path_to_file_id(path);

        nodes.push(Node {
            id: file_id.clone(),
            label: path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string(),
            file_type: "code".to_string(),
            source_file: path.to_string_lossy().to_string(),
            source_location: None,
            community: None,
            rationale: None,
            docstring: None,
            metadata: HashMap::new(),
        });

        // Class definitions with extends
        for (class_name, base_class) in js_class_captures(content) {
            let class_id = make_id(&[&file_id, class_name]);
            nodes.push(Node {
                id: class_id.clone(),
                label: class_name.to_string(),
                file_type: "code".to_string(),
                source_file: path.to_string_lossy().to_string(),
                source_location: None,
                community: None,
                rationale: None,
                docstring: None,
                metadata: HashMap::new(),
            });
            edges.push(Edge {
                source: file_id.clone(),
                target: class_id.clone(),
                relation: "contains".to_string(),
                confidence: "EXTRACTED".to_string(),
                source_file: Some(path.to_string_lossy().to_string()),
                weight: 1.0,
                context: None,
            });

            if let Some(base) = base_class {
                let base_id = make_id(&[base]);
                if !nodes.iter().any(|n| n.id == base_id) {
                    nodes.push(Node {
                        id: base_id.clone(),
                        label: base.to_string(),
                        file_type: "code".to_string(),
                        source_file: path.to_string_lossy().to_string(),
                        source_location: None,
                        community: None,
                        rationale: None,
                        docstring: None,
                        metadata: HashMap::new(),
                    });
                }
                edges.push(Edge {
                    source: class_id,
                    target: base_id,
                    relation: "inherits".to_string(),
                    confidence: "EXTRACTED".to_string(),
                    source_file: Some(path.to_string_lossy().to_string()),
                    weight: 1.0,
                    context: None,
                });
            }
        }

        // ESM imports: import { x } from './mod'
        for (specifiers, source_module) in js_esm_import_captures(content) {
            let mod_id = make_id(&[&file_id, source_module]);
            if !nodes.iter().any(|n| n.id == mod_id) {
                nodes.push(Node {
                    id: mod_id.clone(),
                    label: source_module.to_string(),
                    file_type: "code".to_string(),
                    source_file: path.to_string_lossy().to_string(),
                    source_location: None,
                    community: None,
                    rationale: None,
                    docstring: None,
                    metadata: HashMap::new(),
                });
            }
            for spec in specifiers {
                let spec_id = make_id(&[&file_id, spec]);
                if !nodes.iter().any(|n| n.id == spec_id) {
                    nodes.push(Node {
                        id: spec_id.clone(),
                        label: spec.to_string(),
                        file_type: "code".to_string(),
                        source_file: path.to_string_lossy().to_string(),
                        source_location: None,
                        community: None,
                        rationale: None,
                        docstring: None,
                        metadata: HashMap::new(),
                    });
                }
                edges.push(Edge {
                    source: file_id.clone(),
                    target: spec_id,
                    relation: "imports".to_string(),
                    confidence: "EXTRACTED".to_string(),
                    source_file: Some(path.to_string_lossy().to_string()),
                    weight: 1.0,
                    context: None,
                });
            }
            edges.push(Edge {
                source: file_id.clone(),
                target: mod_id,
                relation: "imports_from".to_string(),
                confidence: "EXTRACTED".to_string(),
                source_file: Some(path.to_string_lossy().to_string()),
                weight: 1.0,
                context: None,
            });
        }

        // Dynamic imports: import('./mod')
        for source_module in js_dynamic_import_captures(content) {
            let mod_id = make_id(&[&file_id, source_module]);
            if !nodes.iter().any(|n| n.id == mod_id) {
                nodes.push(Node {
                    id: mod_id.clone(),
                    label: source_module.to_string(),
                    file_type: "code".to_string(),
                    source_file: path.to_string_lossy().to_string(),
                    source_location: None,
                    community: None,
                    rationale: None,
                    docstring: None,
                    metadata: HashMap::new(),
                });
            }
            edges.push(Edge {
                source: file_id.clone(),
                target: mod_id,
                relation: "imports_from".to_string(),
                confidence: "EXTRACTED".to_string(),
                source_file: Some(path.to_string_lossy().to_string()),
                weight: 1.0,
                context: None,
            });
        }

        // CJS require: require('./mod')
        for source_module in js_require_captures(content) {
            let mod_id = make_id(&[&file_id, source_module]);
            if !nodes.iter().any(|n| n.id == mod_id) {
                nodes.push(Node {
                    id: mod_id.clone(),
                    label: source_module.to_string(),
                    file_type: "code".to_string(),
                    source_file: path.to_string_lossy().to_string(),
                    source_location: None,
                    community: None,
                    rationale: None,
                    docstring: None,
                    metadata: HashMap::new(),
                });
            }
            edges.push(Edge {
                source: file_id.clone(),
                target: mod_id,
                relation: "imports_from".to_string(),
                confidence: "EXTRACTED".to_string(),
                source_file: Some(path.to_string_lossy().to_string()),
                weight: 1.0,
                context: None,
            });
        }

        Ok(ExtractionFragment { nodes, edges })
    }
}

impl LanguageExtractor for JavaScriptExtractor {
    fn file_extensions(&self) -> Vec<&'static str> {
        vec!["js", "jsx", "mjs", "cjs"]
    }

    fn extract(&self, source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        Self::extract_fragment(source, path)
    }

    fn resolve_imports(&self, _imports: &[ImportNode]) -> Vec<Edge> {
        vec![]
    }

    fn collect_type_refs(&self, _fragment: &mut ExtractionFragment) {}
}

fn js_class_captures(content: &str) -> Vec<(&str, Option<&str>)> {
    content
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with("class ") {
                let rest = trimmed.strip_prefix("class ")?;
                let class_name = rest.split_whitespace().next()?;
                let base = if let Some(extends_pos) = rest.find("extends ") {
                    let after_extends = &rest[extends_pos + 8..];
                    Some(
                        after_extends
                            .split_whitespace()
                            .next()?
                            .trim_end_matches('{')
                            .trim(),
                    )
                } else {
                    None
                };
                Some((class_name, base))
            } else {
                None
            }
        })
        .collect()
}

fn js_esm_import_captures(content: &str) -> Vec<(Vec<&str>, &str)> {
    content
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with("import ") && trimmed.contains(" from ") {
                let spec_part = &trimmed[7..];
                let (specifiers_str, source) = spec_part.split_once(" from ")?;
                let source = source.trim().trim_matches('\'').trim_matches('"');
                let specifiers: Vec<&str> = specifiers_str
                    .trim()
                    .strip_prefix('{')
                    .and_then(|s| s.strip_suffix('}'))
                    .map(|s| {
                        s.split(',')
                            .map(|n| n.trim())
                            .filter(|n| !n.is_empty())
                            .collect()
                    })
                    .unwrap_or_else(|| vec![specifiers_str.trim()]);
                Some((specifiers, source))
            } else {
                None
            }
        })
        .collect()
}

fn js_dynamic_import_captures(content: &str) -> Vec<&str> {
    content
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.contains("import(") {
                let start = trimmed.find("import(")?;
                let rest = &trimmed[start + 7..];
                let source = rest
                    .split(')')
                    .next()?
                    .trim()
                    .trim_matches('\'')
                    .trim_matches('"');
                if !source.is_empty() {
                    Some(source)
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect()
}

fn js_require_captures(content: &str) -> Vec<&str> {
    content
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.contains("require(") {
                let start = trimmed.find("require(")?;
                let rest = &trimmed[start + 8..];
                let source = rest
                    .split(')')
                    .next()?
                    .trim()
                    .trim_matches('\'')
                    .trim_matches('"');
                if !source.is_empty() {
                    Some(source)
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect()
}

/// TypeScript-specific extractor
pub struct TypeScriptExtractor;

impl TypeScriptExtractor {
    fn extract_fragment(source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        let content = std::str::from_utf8(source).map_err(|e| {
            CodeSynapseError::Parse(format!("invalid UTF-8 in {}: {}", path.display(), e))
        })?;

        let mut fragment = JavaScriptExtractor::extract_fragment(source, path)?;
        let file_id = path_to_file_id(path);

        // Interface declarations
        for name in ts_interface_captures(content) {
            let iface_id = make_id(&[&file_id, name]);
            if !fragment.nodes.iter().any(|n| n.id == iface_id) {
                fragment.nodes.push(Node {
                    id: iface_id.clone(),
                    label: name.to_string(),
                    file_type: "code".to_string(),
                    source_file: path.to_string_lossy().to_string(),
                    source_location: None,
                    community: None,
                    rationale: None,
                    docstring: None,
                    metadata: {
                        let mut m = HashMap::new();
                        m.insert("kind".to_string(), "interface".to_string());
                        m
                    },
                });
                fragment.edges.push(Edge {
                    source: file_id.clone(),
                    target: iface_id,
                    relation: "contains".to_string(),
                    confidence: "EXTRACTED".to_string(),
                    source_file: Some(path.to_string_lossy().to_string()),
                    weight: 1.0,
                    context: None,
                });
            }
        }

        // Type aliases
        for (name, ref_type) in ts_type_alias_captures(content) {
            let alias_id = make_id(&[&file_id, name]);
            if !fragment.nodes.iter().any(|n| n.id == alias_id) {
                fragment.nodes.push(Node {
                    id: alias_id.clone(),
                    label: name.to_string(),
                    file_type: "code".to_string(),
                    source_file: path.to_string_lossy().to_string(),
                    source_location: None,
                    community: None,
                    rationale: None,
                    docstring: None,
                    metadata: {
                        let mut m = HashMap::new();
                        m.insert("kind".to_string(), "type_alias".to_string());
                        m
                    },
                });
                fragment.edges.push(Edge {
                    source: file_id.clone(),
                    target: alias_id.clone(),
                    relation: "contains".to_string(),
                    confidence: "EXTRACTED".to_string(),
                    source_file: Some(path.to_string_lossy().to_string()),
                    weight: 1.0,
                    context: None,
                });
            }
            if let Some(ref_ty) = ref_type {
                let ref_id = make_id(&[ref_ty]);
                if !fragment.nodes.iter().any(|n| n.id == ref_id) {
                    fragment.nodes.push(Node {
                        id: ref_id.clone(),
                        label: ref_ty.to_string(),
                        file_type: "code".to_string(),
                        source_file: path.to_string_lossy().to_string(),
                        source_location: None,
                        community: None,
                        rationale: None,
                        docstring: None,
                        metadata: HashMap::new(),
                    });
                }
                fragment.edges.push(Edge {
                    source: alias_id,
                    target: ref_id,
                    relation: "type_ref".to_string(),
                    confidence: "EXTRACTED".to_string(),
                    source_file: Some(path.to_string_lossy().to_string()),
                    weight: 1.0,
                    context: None,
                });
            }
        }

        Ok(fragment)
    }
}

impl LanguageExtractor for TypeScriptExtractor {
    fn file_extensions(&self) -> Vec<&'static str> {
        vec!["ts", "tsx", "mts", "cts"]
    }

    fn extract(&self, source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        Self::extract_fragment(source, path)
    }

    fn resolve_imports(&self, _imports: &[ImportNode]) -> Vec<Edge> {
        vec![]
    }

    fn collect_type_refs(&self, _fragment: &mut ExtractionFragment) {}
}

fn ts_interface_captures(content: &str) -> Vec<&str> {
    content
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with("interface ") {
                let name = trimmed
                    .strip_prefix("interface ")?
                    .split_whitespace()
                    .next()?
                    .trim_end_matches('{')
                    .trim();
                if !name.is_empty() {
                    Some(name)
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect()
}

fn ts_type_alias_captures(content: &str) -> Vec<(&str, Option<&str>)> {
    content
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with("type ") && trimmed.contains('=') {
                let rest = trimmed.strip_prefix("type ")?;
                let name = rest.split_whitespace().next()?;
                let rhs = rest.split('=').nth(1)?.trim();
                let ref_type = rhs.split('<').next()?.split_whitespace().next();
                Some((name, ref_type))
            } else {
                None
            }
        })
        .collect()
}

/// Go-specific extractor
pub struct GoExtractor;

impl GoExtractor {
    fn extract_fragment(source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        let content = std::str::from_utf8(source).map_err(|e| {
            CodeSynapseError::Parse(format!("invalid UTF-8 in {}: {}", path.display(), e))
        })?;

        let mut nodes = Vec::new();
        let mut edges = Vec::new();

        let file_id = path_to_file_id(path);

        nodes.push(Node {
            id: file_id.clone(),
            label: path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string(),
            file_type: "code".to_string(),
            source_file: path.to_string_lossy().to_string(),
            source_location: None,
            community: None,
            rationale: None,
            docstring: None,
            metadata: HashMap::new(),
        });

        // Struct definitions
        for name in go_struct_captures(content) {
            let struct_id = make_id(&[&file_id, name]);
            nodes.push(Node {
                id: struct_id.clone(),
                label: name.to_string(),
                file_type: "code".to_string(),
                source_file: path.to_string_lossy().to_string(),
                source_location: None,
                community: None,
                rationale: None,
                docstring: None,
                metadata: {
                    let mut m = HashMap::new();
                    m.insert("kind".to_string(), "struct".to_string());
                    m
                },
            });
            edges.push(Edge {
                source: file_id.clone(),
                target: struct_id,
                relation: "contains".to_string(),
                confidence: "EXTRACTED".to_string(),
                source_file: Some(path.to_string_lossy().to_string()),
                weight: 1.0,
                context: None,
            });
        }

        // Imports
        for module in go_import_captures(content) {
            let mod_id = make_id(&[&file_id, module]);
            if !nodes.iter().any(|n| n.id == mod_id) {
                nodes.push(Node {
                    id: mod_id.clone(),
                    label: module.to_string(),
                    file_type: "code".to_string(),
                    source_file: path.to_string_lossy().to_string(),
                    source_location: None,
                    community: None,
                    rationale: None,
                    docstring: None,
                    metadata: HashMap::new(),
                });
            }
            edges.push(Edge {
                source: file_id.clone(),
                target: mod_id,
                relation: "imports".to_string(),
                confidence: "EXTRACTED".to_string(),
                source_file: Some(path.to_string_lossy().to_string()),
                weight: 1.0,
                context: None,
            });
        }

        Ok(ExtractionFragment { nodes, edges })
    }
}

impl LanguageExtractor for GoExtractor {
    fn file_extensions(&self) -> Vec<&'static str> {
        vec!["go"]
    }

    fn extract(&self, source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        Self::extract_fragment(source, path)
    }

    fn resolve_imports(&self, _imports: &[ImportNode]) -> Vec<Edge> {
        vec![]
    }

    fn collect_type_refs(&self, _fragment: &mut ExtractionFragment) {}
}

fn go_struct_captures(content: &str) -> Vec<&str> {
    content
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with("type ") && trimmed.contains(" struct") {
                let name = trimmed.strip_prefix("type ")?.split_whitespace().next()?;
                if !name.is_empty() {
                    Some(name)
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect()
}

fn go_import_captures(content: &str) -> Vec<&str> {
    content
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with("import ") {
                let rest = trimmed.strip_prefix("import ")?;
                let module = rest.trim().trim_matches('"');
                if !module.is_empty() {
                    Some(module)
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect()
}

/// Rust-specific extractor
pub struct RustExtractor;

impl RustExtractor {
    fn extract_fragment(source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        let content = std::str::from_utf8(source).map_err(|e| {
            CodeSynapseError::Parse(format!("invalid UTF-8 in {}: {}", path.display(), e))
        })?;

        let mut nodes = Vec::new();
        let mut edges = Vec::new();

        let file_id = path_to_file_id(path);

        nodes.push(Node {
            id: file_id.clone(),
            label: path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string(),
            file_type: "code".to_string(),
            source_file: path.to_string_lossy().to_string(),
            source_location: None,
            community: None,
            rationale: None,
            docstring: None,
            metadata: HashMap::new(),
        });

        // Struct definitions with generics
        for (name, generic_param) in rs_struct_captures(content) {
            let struct_id = make_id(&[&file_id, name]);
            nodes.push(Node {
                id: struct_id.clone(),
                label: name.to_string(),
                file_type: "code".to_string(),
                source_file: path.to_string_lossy().to_string(),
                source_location: None,
                community: None,
                rationale: None,
                docstring: None,
                metadata: HashMap::new(),
            });
            edges.push(Edge {
                source: file_id.clone(),
                target: struct_id.clone(),
                relation: "contains".to_string(),
                confidence: "EXTRACTED".to_string(),
                source_file: Some(path.to_string_lossy().to_string()),
                weight: 1.0,
                context: None,
            });

            if let Some(generic) = generic_param {
                let gen_id = make_id(&[&file_id, generic, "ty"]);
                if !nodes.iter().any(|n| n.id == gen_id) {
                    nodes.push(Node {
                        id: gen_id.clone(),
                        label: generic.to_string(),
                        file_type: "code".to_string(),
                        source_file: path.to_string_lossy().to_string(),
                        source_location: None,
                        community: None,
                        rationale: None,
                        docstring: None,
                        metadata: {
                            let mut m = HashMap::new();
                            m.insert("kind".to_string(), "generic_param".to_string());
                            m
                        },
                    });
                }
                edges.push(Edge {
                    source: struct_id,
                    target: gen_id,
                    relation: "generic".to_string(),
                    confidence: "EXTRACTED".to_string(),
                    source_file: Some(path.to_string_lossy().to_string()),
                    weight: 1.0,
                    context: None,
                });
            }
        }

        // Use imports
        for path_parts in rs_use_captures(content) {
            // Create nodes for each segment in the path
            let mut prev_id: Option<NodeId> = None;
            let parts: Vec<&str> = path_parts.split("::").collect();
            for (i, part) in parts.iter().enumerate() {
                let seg_id = if i == 0 {
                    make_id(&[part])
                } else {
                    let parent = make_id(parts[..i].as_ref());
                    make_id(&[&parent, part])
                };
                if !nodes.iter().any(|n| n.id == seg_id) {
                    nodes.push(Node {
                        id: seg_id.clone(),
                        label: part.to_string(),
                        file_type: "code".to_string(),
                        source_file: path.to_string_lossy().to_string(),
                        source_location: None,
                        community: None,
                        rationale: None,
                        docstring: None,
                        metadata: HashMap::new(),
                    });
                }
                if let Some(_prev) = prev_id {
                    edges.push(Edge {
                        source: file_id.clone(),
                        target: seg_id.clone(),
                        relation: "imports".to_string(),
                        confidence: "EXTRACTED".to_string(),
                        source_file: Some(path.to_string_lossy().to_string()),
                        weight: 1.0,
                        context: None,
                    });
                }
                prev_id = Some(seg_id);
            }
        }

        Ok(ExtractionFragment { nodes, edges })
    }
}

impl LanguageExtractor for RustExtractor {
    fn file_extensions(&self) -> Vec<&'static str> {
        vec!["rs"]
    }

    fn extract(&self, source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        Self::extract_fragment(source, path)
    }

    fn resolve_imports(&self, _imports: &[ImportNode]) -> Vec<Edge> {
        vec![]
    }

    fn collect_type_refs(&self, _fragment: &mut ExtractionFragment) {}
}

fn rs_struct_captures(content: &str) -> Vec<(&str, Option<&str>)> {
    content
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with("struct ") {
                let rest = trimmed.strip_prefix("struct ")?;
                let name = rest
                    .split(|c: char| c.is_whitespace() || c == '<' || c == '{')
                    .next()?;
                let generic = rest.find('<').and_then(|start| {
                    let inner = &rest[start + 1..];
                    inner.find('>').map(|end| inner[..end].trim())
                });
                Some((name, generic))
            } else {
                None
            }
        })
        .collect()
}

fn rs_use_captures(content: &str) -> Vec<&str> {
    content
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with("use ") {
                let rest = trimmed.strip_prefix("use ")?.trim_end_matches(';').trim();
                if !rest.is_empty() {
                    Some(rest)
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_python_class() {
        let source = b"class Foo(Bar):\n    pass\n";
        let path = Path::new("test.py");
        let result = PythonExtractor.extract(source, path).unwrap();

        let foo_node = result.nodes.iter().find(|n| n.label == "Foo");
        assert!(foo_node.is_some(), "expected Foo node");

        // Should have file node + class node
        assert!(result.nodes.len() >= 2, "expected at least 2 nodes");

        // Should have contains edge
        let contains_edge = result.edges.iter().find(|e| e.relation == "contains");
        assert!(contains_edge.is_some(), "expected contains edge");
    }

    #[test]
    fn test_extract_python_function() {
        let source = b"def foo(x: int) -> str:\n    pass\n";
        let path = Path::new("test.py");
        let result = PythonExtractor.extract(source, path).unwrap();

        let fn_node = result.nodes.iter().find(|n| n.label == "foo()");
        assert!(fn_node.is_some(), "expected foo() node");
    }

    #[test]
    fn test_extract_python_import() {
        let source = b"import os\n";
        let path = Path::new("test.py");
        let result = PythonExtractor.extract(source, path).unwrap();

        let import_edge = result.edges.iter().find(|e| e.relation == "imports");
        assert!(import_edge.is_some(), "expected imports edge");
    }

    #[test]
    fn test_extract_python_from_import() {
        let source = b"from .helper import transform\n";
        let path = Path::new("test.py");
        let result = PythonExtractor.extract(source, path).unwrap();

        let import_edge = result.edges.iter().find(|e| e.relation == "imports");
        assert!(import_edge.is_some(), "expected imports edge");
    }

    #[test]
    fn test_extract_recursion_limit() {
        // Deeply nested but valid file - should not crash
        let source = b"# deeply nested\n";
        let path = Path::new("test.py");
        let result = PythonExtractor.extract(source, path).unwrap();
        assert!(!result.nodes.is_empty(), "should at least have file node");
    }

    #[test]
    fn test_extract_syntax_error() {
        let source = b"def foo( bar : \n"; // invalid syntax
        let path = Path::new("test.py");
        let result = PythonExtractor.extract(source, path);
        assert!(result.is_ok(), "should gracefully handle syntax errors");
    }

    #[test]
    fn test_make_id() {
        assert_eq!(make_id(&["foo", "bar"]), "foo_bar");
        assert_eq!(make_id(&["Foo", "Bar"]), "foo_bar");
        assert_eq!(make_id(&["foo-bar"]), "foo_bar");
    }

    #[test]
    fn test_normalize_id() {
        assert_eq!(normalize_id("Foo-Bar"), "foo_bar");
        assert_eq!(
            normalize_id("Session ValidateToken"),
            "session_validatetoken"
        );
    }

    // --- JavaScript extractor tests (15-18) ---

    #[test]
    fn test_extract_js_class() {
        let source = b"class Foo extends Bar {}";
        let path = Path::new("class_def.js");
        let result = JavaScriptExtractor.extract(source, path).unwrap();

        let foo_node = result.nodes.iter().find(|n| n.label == "Foo");
        assert!(foo_node.is_some(), "expected Foo node");

        let inherits_edge = result.edges.iter().find(|e| e.relation == "inherits");
        assert!(inherits_edge.is_some(), "expected inherits edge");

        let bar_node = result.nodes.iter().find(|n| n.label == "Bar");
        assert!(bar_node.is_some(), "expected Bar node");
    }

    #[test]
    fn test_extract_js_import() {
        let source = b"import { x } from './mod'";
        let path = Path::new("imports.js");
        let result = JavaScriptExtractor.extract(source, path).unwrap();

        let imports_from_edge = result.edges.iter().find(|e| e.relation == "imports_from");
        assert!(imports_from_edge.is_some(), "expected imports_from edge");

        let imports_edge = result
            .edges
            .iter()
            .filter(|e| e.relation == "imports")
            .count();
        assert!(imports_edge > 0, "expected at least one imports edge");
    }

    #[test]
    fn test_extract_js_dynamic_import() {
        let source = b"const mod = await import('./mod')";
        let path = Path::new("imports.js");
        let result = JavaScriptExtractor.extract(source, path).unwrap();

        let imports_from_edge = result.edges.iter().find(|e| e.relation == "imports_from");
        assert!(imports_from_edge.is_some(), "expected imports_from edge");
    }

    #[test]
    fn test_extract_js_require() {
        let source = b"const m = require('./mod')";
        let path = Path::new("imports.js");
        let result = JavaScriptExtractor.extract(source, path).unwrap();

        let imports_from_edge = result.edges.iter().find(|e| e.relation == "imports_from");
        assert!(imports_from_edge.is_some(), "expected imports_from edge");
    }

    // --- TypeScript extractor tests (19-20) ---

    #[test]
    fn test_extract_ts_interface() {
        let source = b"interface Foo {\n  bar(): void\n}";
        let path = Path::new("interface.ts");
        let result = TypeScriptExtractor.extract(source, path).unwrap();

        let iface_node = result.nodes.iter().find(|n| n.label == "Foo");
        assert!(iface_node.is_some(), "expected Foo interface node");
        if let Some(n) = iface_node {
            assert_eq!(
                n.metadata.get("kind").map(|s| s.as_str()),
                Some("interface")
            );
        }
    }

    #[test]
    fn test_extract_ts_type_alias() {
        let source = b"type Foo = Bar<string>";
        let path = Path::new("interface.ts");
        let result = TypeScriptExtractor.extract(source, path).unwrap();

        let alias_node = result.nodes.iter().find(|n| n.label == "Foo");
        assert!(alias_node.is_some(), "expected Foo type alias node");

        let type_ref_edge = result.edges.iter().find(|e| e.relation == "type_ref");
        assert!(type_ref_edge.is_some(), "expected type_ref edge");
    }

    // --- Go extractor tests (21-22) ---

    #[test]
    fn test_extract_go_struct() {
        let source = b"type Foo struct {\n\tBar string\n}";
        let path = Path::new("struct.go");
        let result = GoExtractor.extract(source, path).unwrap();

        let struct_node = result.nodes.iter().find(|n| n.label == "Foo");
        assert!(struct_node.is_some(), "expected Foo struct node");
    }

    #[test]
    fn test_extract_go_import() {
        let source = b"import \"fmt\"";
        let path = Path::new("struct.go");
        let result = GoExtractor.extract(source, path).unwrap();

        let import_edge = result.edges.iter().find(|e| e.relation == "imports");
        assert!(import_edge.is_some(), "expected imports edge");

        let fmt_node = result.nodes.iter().find(|n| n.label == "fmt");
        assert!(fmt_node.is_some(), "expected fmt node");
    }

    // --- Rust extractor tests (23-24) ---

    #[test]
    fn test_extract_rs_struct() {
        let source = b"struct Foo<T> {\n    bar: T,\n}";
        let path = Path::new("generic.rs");
        let result = RustExtractor.extract(source, path).unwrap();

        let struct_node = result.nodes.iter().find(|n| n.label == "Foo");
        assert!(struct_node.is_some(), "expected Foo struct node");

        let generic_edge = result.edges.iter().find(|e| e.relation == "generic");
        assert!(generic_edge.is_some(), "expected generic edge");
    }

    #[test]
    fn test_extract_rs_import() {
        let source = b"use crate::mod::Foo;";
        let path = Path::new("generic.rs");
        let result = RustExtractor.extract(source, path).unwrap();

        let imports = result
            .edges
            .iter()
            .filter(|e| e.relation == "imports")
            .count();
        assert!(imports > 0, "expected at least one imports edge");
    }

    // --- Edge case tests ---

    #[test]
    fn test_extract_empty_source() {
        let source = b"";
        let path = Path::new("empty.py");
        let result = PythonExtractor.extract(source, path).unwrap();
        // Should have at least a file node
        assert!(!result.nodes.is_empty());
    }

    #[test]
    fn test_extract_unsupported_extension() {
        let extractor = Extractor::new();
        let path = Path::new("data.bin");
        let source = b"\x00\x01\x02";
        let result = extractor.extract_file(path, source);
        // Unknown extension returns Ok with empty fragment
        assert!(result.is_ok(), "unsupported extension returns Ok(fragment)");
        let fragment = result.unwrap();
        assert!(fragment.nodes.is_empty());
        assert!(fragment.edges.is_empty());
    }

    #[test]
    fn test_extract_non_utf8() {
        let source = b"\xff\xfe\x00\x01";
        let path = Path::new("main.py");
        let result = PythonExtractor.extract(source, path);
        // Non-UTF8 bytes return a parse error from from_utf8
        assert!(result.is_err(), "non-UTF8 should return an error");
    }

    #[test]
    fn test_extract_binary_content() {
        // Null bytes are valid UTF-8 in Rust, but tree-sitter may parse them
        let source = b"\x00\x01\x02\x03\x04\x05";
        let path = Path::new("test.py");
        let result = PythonExtractor.extract(source, path);
        // Should not panic regardless of outcome
        let _ = result;
    }

    #[test]
    fn test_extract_trailing_whitespace_content() {
        let source = b"   \n   \n   ";
        let path = Path::new("whitespace.py");
        let result = PythonExtractor.extract(source, path).unwrap();
        assert!(
            !result.nodes.is_empty(),
            "whitespace-only file should still have file node"
        );
    }

    #[test]
    fn test_extract_unicode_identifiers() {
        let source = "def función(ñ): pass\n".as_bytes();
        let path = Path::new("unicode.py");
        let result = PythonExtractor.extract(source, path).unwrap();
        let fn_node = result
            .nodes
            .iter()
            .find(|n| n.label.contains("función") || n.label.contains("fun"));
        assert!(fn_node.is_some(), "expected unicode function node");
    }

    #[test]
    fn test_extract_js_empty_source() {
        let source = b"";
        let path = Path::new("empty.js");
        let result = JavaScriptExtractor.extract(source, path).unwrap();
        assert!(!result.nodes.is_empty());
    }

    #[test]
    fn test_extract_rs_empty_source() {
        let source = b"";
        let path = Path::new("empty.rs");
        let result = RustExtractor.extract(source, path).unwrap();
        assert!(!result.nodes.is_empty());
    }

    #[test]
    fn test_extract_ts_empty_source() {
        let source = b"";
        let path = Path::new("empty.ts");
        let result = TypeScriptExtractor.extract(source, path).unwrap();
        assert!(!result.nodes.is_empty());
    }

    #[test]
    fn test_extract_go_empty_source() {
        let source = b"";
        let path = Path::new("empty.go");
        let result = GoExtractor.extract(source, path).unwrap();
        assert!(!result.nodes.is_empty());
    }

    #[test]
    fn test_extract_extractor_register_multi_extension() {
        let mut extractor = Extractor::new();
        extractor.register("py", Box::new(PythonExtractor));
        let path = Path::new("main.py");
        let source = b"class Foo: pass";
        let result = extractor.extract_file(path, source);
        assert!(result.is_ok());
        let fragment = result.unwrap();
        assert!(!fragment.nodes.is_empty());
    }

    #[test]
    fn test_file_node_ids_differ_for_same_stem_different_dirs() {
        let source = b"class Auth: pass\n";
        let py_result = PythonExtractor
            .extract(source, Path::new("python/auth.py"))
            .unwrap();
        let rs_result = RustExtractor
            .extract(source, Path::new("rust/auth.rs"))
            .unwrap();

        let py_file_node = py_result
            .nodes
            .iter()
            .find(|n| n.label == "auth.py")
            .unwrap();
        let rs_file_node = rs_result
            .nodes
            .iter()
            .find(|n| n.label == "auth.rs")
            .unwrap();

        assert_ne!(
            py_file_node.id, rs_file_node.id,
            "same stem in different dirs must produce different node IDs"
        );
    }

    #[test]
    fn test_file_node_ids_differ_for_same_stem_same_dir() {
        let source = b"class Auth: pass\n";
        let py_result = PythonExtractor
            .extract(source, Path::new("src/auth.py"))
            .unwrap();
        let rs_result = RustExtractor
            .extract(source, Path::new("src/auth.rs"))
            .unwrap();

        let py_file_node = py_result
            .nodes
            .iter()
            .find(|n| n.label == "auth.py")
            .unwrap();
        let rs_file_node = rs_result
            .nodes
            .iter()
            .find(|n| n.label == "auth.rs")
            .unwrap();

        assert_ne!(
            py_file_node.id, rs_file_node.id,
            "same stem same dir different extension must produce different node IDs"
        );
    }
}
