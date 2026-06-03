use super::make_file_node;
use crate::error::Result;
use crate::extract::{make_id, ImportNode, LanguageExtractor};
use crate::types::{Edge, ExtractionFragment, Node};
use std::collections::HashMap;
use std::path::Path;

pub struct CsprojExtractor;

impl CsprojExtractor {
    pub fn extract(source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        let (file_id, _, file_node) = make_file_node(path);
        let mut fragment = ExtractionFragment {
            nodes: vec![file_node],
            edges: vec![],
        };

        let content = std::str::from_utf8(source).unwrap_or("");

        // SDK attribute: <Project Sdk="Microsoft.NET.Sdk.Web">
        let sdk_re = regex::Regex::new(r#"<Project\s[^>]*Sdk\s*=\s*"([^"]+)""#);
        if let Ok(r) = sdk_re {
            for cap in r.captures_iter(content) {
                let sdk = cap.get(1).map(|m| m.as_str()).unwrap_or("");
                if !sdk.is_empty() {
                    let id = make_id(&[&file_id, sdk]);
                    fragment.nodes.push(Node {
                        id: id.clone(),
                        label: sdk.to_string(),
                        file_type: "code".to_string(),
                        source_file: path.to_string_lossy().to_string(),
                        source_location: None,
                        community: None,
                        rationale: None,
                        docstring: None,
                        metadata: HashMap::new(),
                    });
                    fragment.edges.push(Edge {
                        source: file_id.clone(),
                        target: id,
                        relation: "contains".to_string(),
                        confidence: "EXTRACTED".to_string(),
                        source_file: Some(path.to_string_lossy().to_string()),
                        weight: 1.0,
                        context: None,
                    });
                }
            }
        }

        // TargetFramework
        let tf_re = regex::Regex::new(r#"<TargetFramework>([^<]+)</TargetFramework>"#);
        if let Ok(r) = tf_re {
            for cap in r.captures_iter(content) {
                let tf = cap.get(1).map(|m| m.as_str()).unwrap_or("").trim();
                if !tf.is_empty() {
                    let id = make_id(&[&file_id, tf]);
                    fragment.nodes.push(Node {
                        id: id.clone(),
                        label: tf.to_string(),
                        file_type: "code".to_string(),
                        source_file: path.to_string_lossy().to_string(),
                        source_location: None,
                        community: None,
                        rationale: None,
                        docstring: None,
                        metadata: HashMap::new(),
                    });
                    fragment.edges.push(Edge {
                        source: file_id.clone(),
                        target: id,
                        relation: "contains".to_string(),
                        confidence: "EXTRACTED".to_string(),
                        source_file: Some(path.to_string_lossy().to_string()),
                        weight: 1.0,
                        context: None,
                    });
                }
            }
        }

        // PackageReference Include="..." Version="..."
        let pkg_re = regex::Regex::new(r#"<PackageReference\s[^>]*Include\s*=\s*"([^"]+)""#);
        if let Ok(r) = pkg_re {
            for cap in r.captures_iter(content) {
                let pkg = cap.get(1).map(|m| m.as_str()).unwrap_or("");
                if !pkg.is_empty() {
                    let id = make_id(&[&file_id, pkg]);
                    fragment.nodes.push(Node {
                        id: id.clone(),
                        label: pkg.to_string(),
                        file_type: "code".to_string(),
                        source_file: path.to_string_lossy().to_string(),
                        source_location: None,
                        community: None,
                        rationale: None,
                        docstring: None,
                        metadata: HashMap::new(),
                    });
                    fragment.edges.push(Edge {
                        source: file_id.clone(),
                        target: id,
                        relation: "imports".to_string(),
                        confidence: "EXTRACTED".to_string(),
                        source_file: Some(path.to_string_lossy().to_string()),
                        weight: 1.0,
                        context: Some("import".to_string()),
                    });
                }
            }
        }

        // ProjectReference Include="..."
        let proj_re = regex::Regex::new(r#"<ProjectReference\s[^>]*Include\s*=\s*"([^"]+)""#);
        if let Ok(r) = proj_re {
            for cap in r.captures_iter(content) {
                let proj_path_str = cap.get(1).map(|m| m.as_str()).unwrap_or("");
                if !proj_path_str.is_empty() {
                    let file_name = proj_path_str
                        .replace('\\', "/")
                        .split('/')
                        .next_back()
                        .unwrap_or(proj_path_str)
                        .to_string();
                    let id = make_id(&[&file_id, &file_name]);
                    fragment.nodes.push(Node {
                        id: id.clone(),
                        label: file_name,
                        file_type: "code".to_string(),
                        source_file: path.to_string_lossy().to_string(),
                        source_location: None,
                        community: None,
                        rationale: None,
                        docstring: None,
                        metadata: HashMap::new(),
                    });
                    fragment.edges.push(Edge {
                        source: file_id.clone(),
                        target: id,
                        relation: "imports".to_string(),
                        confidence: "EXTRACTED".to_string(),
                        source_file: Some(path.to_string_lossy().to_string()),
                        weight: 1.0,
                        context: Some("import".to_string()),
                    });
                }
            }
        }

        Ok(fragment)
    }
}

impl LanguageExtractor for CsprojExtractor {
    fn file_extensions(&self) -> Vec<&'static str> {
        vec!["csproj"]
    }
    fn extract(&self, source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        Self::extract(source, path)
    }
    fn resolve_imports(&self, _imports: &[ImportNode]) -> Vec<Edge> {
        vec![]
    }
    fn collect_type_refs(&self, _fragment: &mut ExtractionFragment) {}
}
