use crate::config::LlmConfig;
use crate::error::{CodeSynapseError, Result};
use crate::extract::make_id;
use crate::types::{Edge, ExtractionFragment, Node};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

pub trait LlmExtractor: Send + Sync {
    fn extract(&self, source: &[u8], path: &Path) -> Result<ExtractionFragment>;
}

const SYSTEM_PROMPT: &str = "You are a knowledge graph extractor. Given text, extract entities as nodes and relationships as edges. Respond with ONLY a JSON object — no prose, no markdown fences. Schema: {\"nodes\":[{\"id\":\"slug\",\"label\":\"Human Label\"}],\"edges\":[{\"source\":\"id1\",\"target\":\"id2\",\"relation\":\"relationship_type\"}]}";

fn extraction_user_prompt(text: &str) -> String {
    format!(
        "Extract knowledge graph from:\n\n{}",
        &text[..text.len().min(8000)]
    )
}

#[derive(Deserialize)]
struct LlmNode {
    id: String,
    label: String,
}

#[derive(Deserialize)]
struct LlmEdge {
    source: String,
    target: String,
    relation: String,
}

#[derive(Deserialize)]
struct LlmResponse {
    nodes: Vec<LlmNode>,
    edges: Vec<LlmEdge>,
}

pub fn parse_llm_response(text: &str, path: &Path) -> Result<ExtractionFragment> {
    let json_str = strip_fences(text);
    let resp: LlmResponse = serde_json::from_str(json_str)
        .map_err(|e| CodeSynapseError::Parse(format!("LLM response parse error: {e}")))?;

    let source_file = path.to_string_lossy().to_string();
    let nodes = resp
        .nodes
        .into_iter()
        .map(|n| Node {
            id: n.id,
            label: n.label,
            file_type: "llm".to_string(),
            source_file: source_file.clone(),
            source_location: None,
            community: None,
            rationale: None,
            docstring: None,
            metadata: HashMap::new(),
        })
        .collect();

    let edges = resp
        .edges
        .into_iter()
        .map(|e| Edge {
            source: e.source,
            target: e.target,
            relation: e.relation,
            confidence: "high".to_string(),
            source_file: Some(source_file.clone()),
            weight: 1.0,
            context: None,
        })
        .collect();

    Ok(ExtractionFragment { nodes, edges })
}

fn strip_fences(text: &str) -> &str {
    let trimmed = text.trim();
    if let Some(inner) = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
    {
        if let Some(end) = inner.rfind("```") {
            return inner[..end].trim();
        }
    }
    trimmed
}

fn fallback_fragment(path: &Path) -> ExtractionFragment {
    let file_id = make_id(&[path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .as_ref()]);
    ExtractionFragment {
        nodes: vec![Node {
            id: file_id,
            label: path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string(),
            file_type: "llm".to_string(),
            source_file: path.to_string_lossy().to_string(),
            source_location: None,
            community: None,
            rationale: None,
            docstring: None,
            metadata: HashMap::new(),
        }],
        edges: vec![],
    }
}

#[allow(clippy::result_large_err)]
fn send_json(
    req: ureq::Request,
    body: serde_json::Value,
) -> std::result::Result<ureq::Response, ureq::Error> {
    let s = body.to_string();
    req.send_string(&s)
}

pub struct AnthropicLlmExtractor {
    pub api_key: String,
    pub model: String,
}

impl LlmExtractor for AnthropicLlmExtractor {
    fn extract(&self, source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        let text = String::from_utf8_lossy(source);
        let body = serde_json::json!({
            "model": self.model,
            "max_tokens": 1024,
            "system": SYSTEM_PROMPT,
            "messages": [{"role": "user", "content": extraction_user_prompt(&text)}]
        });
        let req = ureq::post("https://api.anthropic.com/v1/messages")
            .set("x-api-key", &self.api_key)
            .set("anthropic-version", "2023-06-01")
            .set("content-type", "application/json");
        let response = send_json(req, body)
            .map_err(|e| CodeSynapseError::Other(format!("Anthropic API error: {e}")))?;
        let body = response
            .into_string()
            .map_err(|e| CodeSynapseError::Other(format!("Anthropic response read error: {e}")))?;
        let json: serde_json::Value = serde_json::from_str(&body)
            .map_err(|e| CodeSynapseError::Other(format!("Anthropic response parse error: {e}")))?;
        let content = json["content"][0]["text"]
            .as_str()
            .unwrap_or("")
            .to_string();
        parse_llm_response(&content, path).or_else(|_| Ok(fallback_fragment(path)))
    }
}

pub struct OpenAiLlmExtractor {
    pub api_key: String,
    pub model: String,
    pub base_url: String,
}

impl LlmExtractor for OpenAiLlmExtractor {
    fn extract(&self, source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        let text = String::from_utf8_lossy(source);
        let body = serde_json::json!({
            "model": self.model,
            "max_tokens": 1024,
            "messages": [
                {"role": "system", "content": SYSTEM_PROMPT},
                {"role": "user", "content": extraction_user_prompt(&text)}
            ]
        });
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let mut req = ureq::post(&url).set("content-type", "application/json");
        if !self.api_key.is_empty() {
            req = req.set("authorization", &format!("Bearer {}", self.api_key));
        }
        let response = send_json(req, body)
            .map_err(|e| CodeSynapseError::Other(format!("OpenAI API error: {e}")))?;
        let raw = response
            .into_string()
            .map_err(|e| CodeSynapseError::Other(format!("OpenAI response read error: {e}")))?;
        let json: serde_json::Value = serde_json::from_str(&raw)
            .map_err(|e| CodeSynapseError::Other(format!("OpenAI response parse error: {e}")))?;
        let content = json["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();
        parse_llm_response(&content, path).or_else(|_| Ok(fallback_fragment(path)))
    }
}

pub struct OllamaLlmExtractor {
    pub base_url: String,
    pub model: String,
}

impl LlmExtractor for OllamaLlmExtractor {
    fn extract(&self, source: &[u8], path: &Path) -> Result<ExtractionFragment> {
        let text = String::from_utf8_lossy(source);
        let body = serde_json::json!({
            "model": self.model,
            "stream": false,
            "messages": [
                {"role": "system", "content": SYSTEM_PROMPT},
                {"role": "user", "content": extraction_user_prompt(&text)}
            ]
        });
        let url = format!("{}/api/chat", self.base_url.trim_end_matches('/'));
        let req = ureq::post(&url).set("content-type", "application/json");
        let response = send_json(req, body)
            .map_err(|e| CodeSynapseError::Other(format!("Ollama API error: {e}")))?;
        let raw = response
            .into_string()
            .map_err(|e| CodeSynapseError::Other(format!("Ollama response read error: {e}")))?;
        let json: serde_json::Value = serde_json::from_str(&raw)
            .map_err(|e| CodeSynapseError::Other(format!("Ollama response parse error: {e}")))?;
        let content = json["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();
        parse_llm_response(&content, path).or_else(|_| Ok(fallback_fragment(path)))
    }
}

pub fn build_extractor(config: &LlmConfig) -> Result<Box<dyn LlmExtractor>> {
    match config.provider.as_deref().unwrap_or("anthropic") {
        "anthropic" => {
            let api_key = config
                .api_key
                .clone()
                .or_else(|| std::env::var("ANTHROPIC_API_KEY").ok())
                .unwrap_or_default();
            Ok(Box::new(AnthropicLlmExtractor {
                api_key,
                model: config
                    .model
                    .clone()
                    .unwrap_or_else(|| "claude-haiku-4-5-20251001".to_string()),
            }))
        }
        "openai" => {
            let api_key = config
                .api_key
                .clone()
                .or_else(|| std::env::var("OPENAI_API_KEY").ok())
                .unwrap_or_default();
            Ok(Box::new(OpenAiLlmExtractor {
                api_key,
                model: config
                    .model
                    .clone()
                    .unwrap_or_else(|| "gpt-4o-mini".to_string()),
                base_url: config
                    .base_url
                    .clone()
                    .unwrap_or_else(|| "https://api.openai.com/v1".to_string()),
            }))
        }
        "ollama" => Ok(Box::new(OllamaLlmExtractor {
            model: config.model.clone().unwrap_or_else(|| "llama3".to_string()),
            base_url: config
                .base_url
                .clone()
                .unwrap_or_else(|| "http://localhost:11434".to_string()),
        })),
        "openai-compat" => Ok(Box::new(OpenAiLlmExtractor {
            api_key: config.api_key.clone().unwrap_or_default(),
            model: config.model.clone().unwrap_or_default(),
            base_url: config.base_url.clone().unwrap_or_default(),
        })),
        other => Err(CodeSynapseError::Other(format!(
            "Unknown LLM provider: {other}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn test_path() -> PathBuf {
        PathBuf::from("test/doc.md")
    }

    #[test]
    fn test_strip_fences_plain_json() {
        let input = r#"{"nodes":[],"edges":[]}"#;
        assert_eq!(strip_fences(input), input);
    }

    #[test]
    fn test_strip_fences_json_block() {
        let input = "```json\n{\"nodes\":[],\"edges\":[]}\n```";
        assert_eq!(strip_fences(input), "{\"nodes\":[],\"edges\":[]}");
    }

    #[test]
    fn test_strip_fences_plain_block() {
        let input = "```\n{\"nodes\":[],\"edges\":[]}\n```";
        assert_eq!(strip_fences(input), "{\"nodes\":[],\"edges\":[]}");
    }

    #[test]
    fn test_parse_llm_response_valid() {
        let json = r#"{"nodes":[{"id":"foo","label":"Foo"}],"edges":[{"source":"foo","target":"bar","relation":"uses"}]}"#;
        let fragment = parse_llm_response(json, &test_path()).unwrap();
        assert_eq!(fragment.nodes.len(), 1);
        assert_eq!(fragment.nodes[0].id, "foo");
        assert_eq!(fragment.nodes[0].label, "Foo");
        assert_eq!(fragment.nodes[0].file_type, "llm");
        assert_eq!(fragment.edges.len(), 1);
        assert_eq!(fragment.edges[0].relation, "uses");
    }

    #[test]
    fn test_parse_llm_response_fenced() {
        let json = "```json\n{\"nodes\":[{\"id\":\"a\",\"label\":\"A\"}],\"edges\":[]}\n```";
        let fragment = parse_llm_response(json, &test_path()).unwrap();
        assert_eq!(fragment.nodes.len(), 1);
        assert_eq!(fragment.nodes[0].id, "a");
    }

    #[test]
    fn test_parse_llm_response_sets_source_file() {
        let json = r#"{"nodes":[{"id":"n","label":"N"}],"edges":[]}"#;
        let path = PathBuf::from("/some/path/doc.txt");
        let fragment = parse_llm_response(json, &path).unwrap();
        assert_eq!(fragment.nodes[0].source_file, "/some/path/doc.txt");
    }

    #[test]
    fn test_parse_llm_response_empty() {
        let json = r#"{"nodes":[],"edges":[]}"#;
        let fragment = parse_llm_response(json, &test_path()).unwrap();
        assert!(fragment.nodes.is_empty());
        assert!(fragment.edges.is_empty());
    }

    #[test]
    fn test_parse_llm_response_invalid_json() {
        let result = parse_llm_response("not json at all", &test_path());
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("parse error") || msg.contains("Parse error"));
    }

    #[test]
    fn test_parse_llm_response_edge_has_source_file() {
        let json = r#"{"nodes":[],"edges":[{"source":"a","target":"b","relation":"calls"}]}"#;
        let path = PathBuf::from("/tmp/note.md");
        let fragment = parse_llm_response(json, &path).unwrap();
        assert_eq!(
            fragment.edges[0].source_file,
            Some("/tmp/note.md".to_string())
        );
        assert_eq!(fragment.edges[0].confidence, "high");
    }

    #[test]
    fn test_build_extractor_anthropic() {
        let config = LlmConfig {
            provider: Some("anthropic".to_string()),
            model: Some("claude-haiku-4-5-20251001".to_string()),
            api_key: Some("test-key".to_string()),
            base_url: None,
        };
        assert!(build_extractor(&config).is_ok());
    }

    #[test]
    fn test_build_extractor_anthropic_default() {
        let config = LlmConfig {
            provider: None,
            model: None,
            api_key: None,
            base_url: None,
        };
        assert!(build_extractor(&config).is_ok());
    }

    #[test]
    fn test_build_extractor_openai() {
        let config = LlmConfig {
            provider: Some("openai".to_string()),
            model: Some("gpt-4o-mini".to_string()),
            api_key: Some("sk-test".to_string()),
            base_url: None,
        };
        assert!(build_extractor(&config).is_ok());
    }

    #[test]
    fn test_build_extractor_openai_compat() {
        let config = LlmConfig {
            provider: Some("openai-compat".to_string()),
            model: Some("custom-model".to_string()),
            api_key: Some("key".to_string()),
            base_url: Some("http://localhost:8080/v1".to_string()),
        };
        assert!(build_extractor(&config).is_ok());
    }

    #[test]
    fn test_build_extractor_ollama() {
        let config = LlmConfig {
            provider: Some("ollama".to_string()),
            model: Some("llama3".to_string()),
            api_key: None,
            base_url: Some("http://localhost:11434".to_string()),
        };
        assert!(build_extractor(&config).is_ok());
    }

    #[test]
    fn test_build_extractor_unknown_provider() {
        let config = LlmConfig {
            provider: Some("fakeprovider".to_string()),
            model: None,
            api_key: None,
            base_url: None,
        };
        let result = build_extractor(&config);
        assert!(result.is_err());
        let err = result.err().unwrap();
        let msg = err.to_string();
        assert!(msg.contains("Unknown LLM provider: fakeprovider"));
    }

    #[test]
    fn test_fallback_fragment_has_one_node() {
        let path = PathBuf::from("/tmp/readme.md");
        let fragment = fallback_fragment(&path);
        assert_eq!(fragment.nodes.len(), 1);
        assert_eq!(fragment.edges.len(), 0);
        assert_eq!(fragment.nodes[0].file_type, "llm");
    }
}
