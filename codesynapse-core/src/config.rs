use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CodeSynapseConfig {
    pub output: Option<String>,
    #[serde(default)]
    pub no_llm: bool,
    #[serde(default)]
    pub code_only: bool,
    pub formats: Option<Vec<String>>,
    pub llm: Option<LlmConfig>,
    pub embeddings: Option<EmbeddingsConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EmbeddingsConfig {
    /// Path to a directory containing `tokenizer.json` + `model.safetensors`.
    /// Example: `./potion-code-16M`
    pub model_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    pub provider: Option<String>,
    pub model: Option<String>,
    pub api_key: Option<String>,
    pub base_url: Option<String>,
}

impl CodeSynapseConfig {
    pub fn from_file(path: &Path) -> Result<Self, String> {
        let content =
            std::fs::read_to_string(path).map_err(|e| format!("Failed to read config: {}", e))?;
        toml::from_str(&content).map_err(|e| format!("Failed to parse config: {}", e))
    }

    pub fn discover(start: &Path) -> Result<Self, String> {
        let candidates = [
            start.join("codesynapse.toml"),
            start.join("codesynapse.toml"),
            start.join(".codesynapse.toml"),
            start.join(".codesynapse.toml"),
        ];
        for candidate in &candidates {
            if candidate.exists() {
                return Self::from_file(candidate);
            }
        }
        Err("No codesynapse.toml found".to_string())
    }

    pub fn write_default(path: &Path) -> Result<(), String> {
        let config = CodeSynapseConfig::default();
        let content = toml::to_string_pretty(&config)
            .map_err(|e| format!("Failed to serialize config: {}", e))?;
        std::fs::write(path, content).map_err(|e| format!("Failed to write config: {}", e))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = CodeSynapseConfig::default();
        assert!(!config.no_llm);
        assert!(!config.code_only);
        assert!(config.output.is_none());
        assert!(config.formats.is_none());
        assert!(config.llm.is_none());
    }

    #[test]
    fn test_config_parse_full() {
        let toml_str = r#"
output = "codesynapse-out"
no_llm = true
code_only = true
formats = ["json", "html"]

[llm]
provider = "anthropic"
model = "claude-sonnet-4-20250514"
"#;
        let config: CodeSynapseConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.output, Some("codesynapse-out".to_string()));
        assert!(config.no_llm);
        assert!(config.code_only);
        assert_eq!(
            config.formats,
            Some(vec!["json".to_string(), "html".to_string()])
        );
        let llm = config.llm.unwrap();
        assert_eq!(llm.provider, Some("anthropic".to_string()));
        assert_eq!(llm.model, Some("claude-sonnet-4-20250514".to_string()));
    }

    #[test]
    fn test_config_parse_minimal() {
        let toml_str = r#"no_llm = true"#;
        let config: CodeSynapseConfig = toml::from_str(toml_str).unwrap();
        assert!(!config.code_only);
        assert!(config.no_llm);
        assert!(config.output.is_none());
        assert!(config.formats.is_none());
    }

    #[test]
    fn test_config_write_default() {
        let dir = std::env::temp_dir().join("codesynapse_test_config");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let path = dir.join("codesynapse.toml");
        CodeSynapseConfig::write_default(&path).unwrap();
        assert!(path.exists());

        let loaded = CodeSynapseConfig::from_file(&path).unwrap();
        assert!(!loaded.no_llm);
        assert!(!loaded.code_only);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_config_discover() {
        let dir = std::env::temp_dir().join("codesynapse_test_discover");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let path = dir.join("codesynapse.toml");
        std::fs::write(
            &path,
            r#"no_llm = true
code_only = true
"#,
        )
        .unwrap();

        let config = CodeSynapseConfig::discover(&dir).unwrap();
        assert!(config.no_llm);
        assert!(config.code_only);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_config_discover_not_found() {
        let dir = std::env::temp_dir().join("codesynapse_test_no_config");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let result = CodeSynapseConfig::discover(&dir);
        assert!(result.is_err());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_config_parse_empty() {
        let config: CodeSynapseConfig = toml::from_str("").unwrap();
        assert!(!config.no_llm);
        assert!(!config.code_only);
    }
}
