use serde_json::{json, Value};
use std::path::{Path, PathBuf};

use crate::error::{CodeSynapseError, Result};

// ─── constants ────────────────────────────────────────────────────────────────

const CHARS_PER_TOKEN: usize = 4;
const FILE_CHAR_CAP: usize = 20_000;

const CONTEXT_EXCEEDED_MARKERS: &[&str] = &[
    "context size",
    "context length",
    "context_length",
    "context window",
    "n_keep",
    "exceeds the available",
    "n_ctx",
    "maximum context",
    "too many tokens",
    "prompt is too long",
    "context_length_exceeded",
];

// ─── result type ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct LlmResult {
    pub nodes: Vec<Value>,
    pub edges: Vec<Value>,
    pub hyperedges: Vec<Value>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub model: Option<String>,
    pub finish_reason: String,
}

impl LlmResult {
    pub fn empty(model: Option<String>) -> Self {
        Self {
            nodes: vec![],
            edges: vec![],
            hyperedges: vec![],
            input_tokens: 0,
            output_tokens: 0,
            model,
            finish_reason: "stop".to_string(),
        }
    }

    fn merge(mut self, other: Self) -> Self {
        self.nodes.extend(other.nodes);
        self.edges.extend(other.edges);
        self.hyperedges.extend(other.hyperedges);
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
        self.finish_reason = "stop".to_string();
        self
    }
}

// ─── backend registry ────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct BackendConfig {
    pub base_url: String,
    pub default_model: String,
    pub env_keys: Vec<String>,
    pub model_env_key: Option<String>,
    pub temperature: Option<f64>,
    pub reasoning_effort: Option<String>,
    pub max_completion_tokens: usize,
}

fn all_backends() -> Vec<(&'static str, BackendConfig)> {
    vec![
        (
            "gemini",
            BackendConfig {
                base_url: "https://generativelanguage.googleapis.com/v1beta/openai/".into(),
                default_model: "gemini-3-flash-preview".into(),
                env_keys: vec!["GEMINI_API_KEY".into(), "GOOGLE_API_KEY".into()],
                model_env_key: Some("CODESYNAPSE_GEMINI_MODEL".into()),
                temperature: Some(0.0),
                reasoning_effort: Some("low".into()),
                max_completion_tokens: 16384,
            },
        ),
        (
            "kimi",
            BackendConfig {
                base_url: "https://api.moonshot.ai/v1".into(),
                default_model: "kimi-k2.6".into(),
                env_keys: vec!["MOONSHOT_API_KEY".into()],
                model_env_key: None,
                temperature: None,
                reasoning_effort: None,
                max_completion_tokens: 16384,
            },
        ),
        (
            "claude",
            BackendConfig {
                base_url: "https://api.anthropic.com".into(),
                default_model: "claude-sonnet-4-6".into(),
                env_keys: vec!["ANTHROPIC_API_KEY".into()],
                model_env_key: None,
                temperature: Some(0.0),
                reasoning_effort: None,
                max_completion_tokens: 16384,
            },
        ),
        (
            "openai",
            BackendConfig {
                base_url: "https://api.openai.com/v1".into(),
                default_model: "gpt-4.1-mini".into(),
                env_keys: vec!["OPENAI_API_KEY".into()],
                model_env_key: Some("CODESYNAPSE_OPENAI_MODEL".into()),
                temperature: Some(0.0),
                reasoning_effort: None,
                max_completion_tokens: 8192,
            },
        ),
        (
            "deepseek",
            BackendConfig {
                base_url: "https://api.deepseek.com".into(),
                default_model: "deepseek-v4-flash".into(),
                env_keys: vec!["DEEPSEEK_API_KEY".into()],
                model_env_key: Some("CODESYNAPSE_DEEPSEEK_MODEL".into()),
                temperature: Some(0.0),
                reasoning_effort: None,
                max_completion_tokens: 16384,
            },
        ),
        (
            "ollama",
            BackendConfig {
                base_url: "http://localhost:11434/v1".into(),
                default_model: "qwen2.5-coder:7b".into(),
                env_keys: vec!["OLLAMA_API_KEY".into()],
                model_env_key: None,
                temperature: Some(0.0),
                reasoning_effort: None,
                max_completion_tokens: 16384,
            },
        ),
    ]
}

fn get_backend_config(backend: &str) -> Option<BackendConfig> {
    all_backends()
        .into_iter()
        .find(|(k, _)| *k == backend)
        .map(|(_, v)| v)
}

// ─── pure functions ───────────────────────────────────────────────────────────

pub fn get_backend_api_key(backend: &str) -> String {
    if let Some(cfg) = get_backend_config(backend) {
        for key in &cfg.env_keys {
            if let Ok(val) = std::env::var(key) {
                if !val.is_empty() {
                    return val;
                }
            }
        }
    }
    String::new()
}

pub fn detect_backend() -> Option<String> {
    for backend in &["gemini", "kimi", "claude", "openai", "deepseek"] {
        if !get_backend_api_key(backend).is_empty() {
            return Some((*backend).to_string());
        }
    }
    for key in &["AWS_PROFILE", "AWS_REGION", "AWS_DEFAULT_REGION"] {
        if std::env::var(key).map(|v| !v.is_empty()).unwrap_or(false) {
            return Some("bedrock".to_string());
        }
    }
    if let Ok(url) = std::env::var("OLLAMA_BASE_URL") {
        if !url.is_empty() {
            return Some("ollama".to_string());
        }
    }
    None
}

pub fn looks_like_context_exceeded(msg: &str) -> bool {
    let lower = msg.to_lowercase();
    CONTEXT_EXCEEDED_MARKERS.iter().any(|m| lower.contains(m))
}

pub fn response_is_hollow(raw_content: Option<&str>, parsed: &Value) -> bool {
    match raw_content {
        None => return true,
        Some(s) if s.trim().is_empty() => return true,
        _ => {}
    }
    let nodes_empty = parsed
        .get("nodes")
        .and_then(Value::as_array)
        .map(|v| v.is_empty())
        .unwrap_or(true);
    let edges_empty = parsed
        .get("edges")
        .and_then(Value::as_array)
        .map(|v| v.is_empty())
        .unwrap_or(true);
    let hyper_empty = parsed
        .get("hyperedges")
        .and_then(Value::as_array)
        .map(|v| v.is_empty())
        .unwrap_or(true);
    nodes_empty && edges_empty && hyper_empty
}

// ─── request building ────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct OpenAiCompatRequest {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub user_message: String,
    pub temperature: Option<f64>,
    pub reasoning_effort: Option<String>,
    pub max_completion_tokens: usize,
    pub backend: String,
}

pub fn read_files(files: &[PathBuf], root: &Path) -> String {
    let parts: Vec<String> = files
        .iter()
        .filter_map(|p| {
            let rel = p.strip_prefix(root).unwrap_or(p);
            let content = std::fs::read_to_string(p).ok()?;
            let cap = content.len().min(FILE_CHAR_CAP);
            Some(format!("=== {} ===\n{}", rel.display(), &content[..cap]))
        })
        .collect();
    parts.join("\n\n")
}

fn format_env_keys(env_keys: &[String]) -> String {
    env_keys.join(" or ")
}

fn default_model_for(cfg: &BackendConfig) -> String {
    if let Some(ref key) = cfg.model_env_key {
        if let Ok(val) = std::env::var(key) {
            if !val.is_empty() {
                return val;
            }
        }
    }
    cfg.default_model.clone()
}

pub fn build_extract_request(
    files: &[PathBuf],
    backend: &str,
    root: &Path,
    api_key: Option<&str>,
    model: Option<&str>,
) -> Result<OpenAiCompatRequest> {
    let cfg = get_backend_config(backend)
        .ok_or_else(|| CodeSynapseError::Validation(format!("Unknown backend: {backend}")))?;

    let key = match api_key {
        Some(k) if !k.is_empty() => k.to_string(),
        _ => get_backend_api_key(backend),
    };

    if key.is_empty() {
        let key_names = format_env_keys(&cfg.env_keys);
        return Err(CodeSynapseError::Validation(format!(
            "No API key for backend '{backend}'. Set {key_names} or pass api_key="
        )));
    }

    let mdl = model
        .map(|s| s.to_string())
        .unwrap_or_else(|| default_model_for(&cfg));
    let user_message = read_files(files, root);

    Ok(OpenAiCompatRequest {
        base_url: cfg.base_url.clone(),
        api_key: key,
        model: mdl,
        user_message,
        temperature: cfg.temperature,
        reasoning_effort: cfg.reasoning_effort.clone(),
        max_completion_tokens: cfg.max_completion_tokens,
        backend: backend.to_string(),
    })
}

// ─── Ollama extra body ────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct OllamaExtraBody {
    pub num_ctx: usize,
    pub keep_alive: String,
}

pub fn compute_ollama_num_ctx(user_message_len: usize, max_completion_tokens: usize) -> usize {
    if let Ok(raw) = std::env::var("CODESYNAPSE_OLLAMA_NUM_CTX") {
        if let Ok(v) = raw.trim().parse::<usize>() {
            return v;
        }
    }
    let estimated_input = user_message_len / CHARS_PER_TOKEN + 400;
    let auto = (estimated_input + max_completion_tokens + 2000).min(131_072);
    auto.max(8192)
}

pub fn build_extra_body(
    backend: &str,
    user_message: &str,
    max_completion_tokens: usize,
) -> Option<OllamaExtraBody> {
    if backend != "ollama" {
        return None;
    }
    let num_ctx = compute_ollama_num_ctx(user_message.len(), max_completion_tokens);
    let keep_alive =
        std::env::var("CODESYNAPSE_OLLAMA_KEEP_ALIVE").unwrap_or_else(|_| "30m".to_string());
    Some(OllamaExtraBody {
        num_ctx,
        keep_alive,
    })
}

// ─── response processing ─────────────────────────────────────────────────────

fn parse_llm_json(raw: &str) -> Value {
    let stripped = if raw.starts_with("```") {
        let after_fence = raw.split("```").nth(1).unwrap_or("");
        let after_lang = after_fence.strip_prefix("json").unwrap_or(after_fence);
        after_lang.rsplit("```").last().unwrap_or(after_lang).trim()
    } else {
        raw.trim()
    };
    serde_json::from_str(stripped)
        .unwrap_or_else(|_| json!({"nodes": [], "edges": [], "hyperedges": []}))
}

pub fn process_openai_compat_response(
    raw_content: Option<&str>,
    finish_reason: &str,
    prompt_tokens: u64,
    completion_tokens: u64,
    model: &str,
    backend: &str,
) -> LlmResult {
    let parsed = match raw_content {
        Some(s) if !s.trim().is_empty() => parse_llm_json(s),
        _ => json!({"nodes": [], "edges": [], "hyperedges": []}),
    };

    let hollow = response_is_hollow(raw_content, &parsed);
    let effective_finish_reason = if hollow && finish_reason != "length" {
        let _ = backend;
        "length".to_string()
    } else {
        finish_reason.to_string()
    };

    LlmResult {
        nodes: parsed
            .get("nodes")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default(),
        edges: parsed
            .get("edges")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default(),
        hyperedges: parsed
            .get("hyperedges")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default(),
        input_tokens: prompt_tokens,
        output_tokens: completion_tokens,
        model: Some(model.to_string()),
        finish_reason: effective_finish_reason,
    }
}

// ─── adaptive retry ───────────────────────────────────────────────────────────

#[allow(clippy::only_used_in_recursion)]
pub fn extract_with_adaptive_retry<F>(
    chunk: &[PathBuf],
    backend: &str,
    max_depth: usize,
    depth: usize,
    extractor: &F,
) -> Result<LlmResult>
where
    F: Fn(&[PathBuf]) -> Result<LlmResult>,
{
    let result = match extractor(chunk) {
        Ok(r) => r,
        Err(e) => {
            let msg = e.to_string();
            if !looks_like_context_exceeded(&msg) {
                return Err(e);
            }
            if chunk.len() <= 1 {
                return Ok(LlmResult::empty(None));
            }
            if depth >= max_depth {
                return Ok(LlmResult::empty(None));
            }
            let mid = chunk.len() / 2;
            let left = extract_with_adaptive_retry(
                &chunk[..mid],
                backend,
                max_depth,
                depth + 1,
                extractor,
            )?;
            let right = extract_with_adaptive_retry(
                &chunk[mid..],
                backend,
                max_depth,
                depth + 1,
                extractor,
            )?;
            return Ok(left.merge(right));
        }
    };

    if result.finish_reason != "length" {
        return Ok(result);
    }

    if chunk.len() <= 1 || depth >= max_depth {
        return Ok(result);
    }

    let mid = chunk.len() / 2;
    let left =
        extract_with_adaptive_retry(&chunk[..mid], backend, max_depth, depth + 1, extractor)?;
    let right =
        extract_with_adaptive_retry(&chunk[mid..], backend, max_depth, depth + 1, extractor)?;
    Ok(left.merge(right))
}

// ─── corpus parallel ──────────────────────────────────────────────────────────

pub fn effective_max_concurrency(backend: &str, requested: usize) -> usize {
    if backend == "ollama" {
        let parallel = std::env::var("CODESYNAPSE_OLLAMA_PARALLEL").unwrap_or_default();
        if parallel.trim() != "1" {
            return 1;
        }
    }
    requested
}

pub fn extract_corpus_parallel_with<F>(
    files: &[PathBuf],
    backend: &str,
    chunk_size: usize,
    max_concurrency: usize,
    max_retry_depth: usize,
    extractor: F,
) -> LlmResult
where
    F: Fn(&[PathBuf]) -> Result<LlmResult> + Send + Sync + 'static,
{
    let chunks: Vec<Vec<PathBuf>> = files
        .chunks(chunk_size.max(1))
        .map(|c| c.to_vec())
        .collect();
    let total = chunks.len();
    let workers = effective_max_concurrency(backend, max_concurrency.max(1).min(total.max(1)));

    let mut merged = LlmResult {
        finish_reason: "stop".to_string(),
        ..Default::default()
    };

    let accumulate = |acc: &mut LlmResult, r: LlmResult| {
        acc.nodes.extend(r.nodes);
        acc.edges.extend(r.edges);
        acc.hyperedges.extend(r.hyperedges);
        acc.input_tokens += r.input_tokens;
        acc.output_tokens += r.output_tokens;
    };

    if workers <= 1 {
        for (idx, chunk) in chunks.iter().enumerate() {
            match extract_with_adaptive_retry(chunk, backend, max_retry_depth, 0, &extractor) {
                Ok(r) => accumulate(&mut merged, r),
                Err(e) => eprintln!("[codesynapse] chunk {}/{total} failed: {e}", idx + 1),
            }
        }
    } else {
        use std::sync::{Arc, Mutex};
        let extractor = Arc::new(extractor);
        let acc: Arc<Mutex<LlmResult>> = Arc::new(Mutex::new(LlmResult {
            finish_reason: "stop".to_string(),
            ..Default::default()
        }));
        let mut handles = vec![];

        for (idx, chunk) in chunks.into_iter().enumerate() {
            let ext = extractor.clone();
            let acc2 = acc.clone();
            let be = backend.to_string();
            let handle = std::thread::spawn(move || {
                match extract_with_adaptive_retry(&chunk, &be, max_retry_depth, 0, &*ext) {
                    Ok(r) => {
                        let mut guard = acc2.lock().unwrap();
                        guard.nodes.extend(r.nodes);
                        guard.edges.extend(r.edges);
                        guard.hyperedges.extend(r.hyperedges);
                        guard.input_tokens += r.input_tokens;
                        guard.output_tokens += r.output_tokens;
                    }
                    Err(e) => eprintln!("[codesynapse] chunk {}/{total} failed: {e}", idx + 1),
                }
            });
            handles.push(handle);
        }
        for h in handles {
            let _ = h.join();
        }
        let inner = Arc::try_unwrap(acc).unwrap().into_inner().unwrap();
        merged = inner;
        merged.finish_reason = "stop".to_string();
    }

    merged
}

// ─── tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    };

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    const ALL_BACKEND_KEYS: &[&str] = &[
        "GEMINI_API_KEY",
        "GOOGLE_API_KEY",
        "MOONSHOT_API_KEY",
        "ANTHROPIC_API_KEY",
        "OPENAI_API_KEY",
        "DEEPSEEK_API_KEY",
        "AWS_PROFILE",
        "AWS_REGION",
        "AWS_DEFAULT_REGION",
        "OLLAMA_BASE_URL",
    ];

    fn with_env<R, F: FnOnce() -> R>(set: &[(&str, &str)], clear: &[&str], f: F) -> R {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        for &k in clear {
            std::env::remove_var(k);
        }
        for &(k, v) in set {
            std::env::set_var(k, v);
        }
        let r = f();
        for &(k, _) in set {
            std::env::remove_var(k);
        }
        r
    }

    // ── 1. detect_backend: gemini via GEMINI_API_KEY ──────────────────────────

    #[test]
    fn test_gemini_accepts_gemini_api_key() {
        with_env(
            &[("GEMINI_API_KEY", "gemini-key")],
            ALL_BACKEND_KEYS,
            || {
                assert_eq!(detect_backend().as_deref(), Some("gemini"));
                assert_eq!(get_backend_api_key("gemini"), "gemini-key");
            },
        );
    }

    // ── 2. detect_backend: gemini via GOOGLE_API_KEY ──────────────────────────

    #[test]
    fn test_gemini_accepts_google_api_key() {
        with_env(
            &[("GOOGLE_API_KEY", "google-key")],
            ALL_BACKEND_KEYS,
            || {
                assert_eq!(detect_backend().as_deref(), Some("gemini"));
                assert_eq!(get_backend_api_key("gemini"), "google-key");
            },
        );
    }

    // ── 3. detect_backend: gemini wins over all others ────────────────────────

    #[test]
    fn test_backend_detection_prefers_gemini() {
        with_env(
            &[
                ("OPENAI_API_KEY", "openai-key"),
                ("ANTHROPIC_API_KEY", "anthropic-key"),
                ("MOONSHOT_API_KEY", "moonshot-key"),
                ("GEMINI_API_KEY", "gemini-key"),
            ],
            ALL_BACKEND_KEYS,
            || {
                assert_eq!(detect_backend().as_deref(), Some("gemini"));
            },
        );
    }

    // ── 4. detect_backend: openai ─────────────────────────────────────────────

    #[test]
    fn test_openai_backend_detected() {
        with_env(
            &[("OPENAI_API_KEY", "openai-key")],
            ALL_BACKEND_KEYS,
            || {
                assert_eq!(detect_backend().as_deref(), Some("openai"));
                assert_eq!(get_backend_api_key("openai"), "openai-key");
            },
        );
    }

    // ── 5. build_extract_request: gemini routes through openai compat ─────────

    #[test]
    fn test_extract_files_direct_routes_gemini_through_openai_compat() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("note.md");
        std::fs::write(&source, "# Architecture\n\nThe runner emits a snapshot.\n").unwrap();

        with_env(
            &[("GOOGLE_API_KEY", "google-key")],
            ALL_BACKEND_KEYS,
            || {
                let req = build_extract_request(
                    std::slice::from_ref(&source),
                    "gemini",
                    dir.path(),
                    None,
                    None,
                )
                .unwrap();
                assert_eq!(
                    req.base_url,
                    "https://generativelanguage.googleapis.com/v1beta/openai/"
                );
                assert_eq!(req.api_key, "google-key");
                assert_eq!(req.model, "gemini-3-flash-preview");
                assert_eq!(
                    req.user_message,
                    "=== note.md ===\n# Architecture\n\nThe runner emits a snapshot.\n"
                );
                assert_eq!(req.temperature, Some(0.0));
                assert_eq!(req.reasoning_effort.as_deref(), Some("low"));
                assert_eq!(req.max_completion_tokens, 16384);
            },
        );
    }

    // ── 6. CODESYNAPSE_GEMINI_MODEL overrides default model ─────────────────────

    #[test]
    fn test_gemini_model_can_be_overridden_by_env() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("note.md");
        std::fs::write(&source, "# Architecture\n").unwrap();

        with_env(
            &[
                ("GOOGLE_API_KEY", "google-key"),
                ("CODESYNAPSE_GEMINI_MODEL", "gemini-3.1-pro-preview"),
            ],
            ALL_BACKEND_KEYS,
            || {
                let req = build_extract_request(
                    std::slice::from_ref(&source),
                    "gemini",
                    dir.path(),
                    None,
                    None,
                )
                .unwrap();
                assert_eq!(req.model, "gemini-3.1-pro-preview");
            },
        );
    }

    // ── 7. missing gemini key names both env vars in error message ─────────────

    #[test]
    fn test_missing_gemini_key_names_both_supported_env_vars() {
        with_env(&[], ALL_BACKEND_KEYS, || {
            let err = build_extract_request(&[], "gemini", Path::new("."), None, None)
                .unwrap_err()
                .to_string();
            assert!(
                err.contains("GEMINI_API_KEY") && err.contains("GOOGLE_API_KEY"),
                "error should name both keys, got: {err}"
            );
        });
    }

    // ── 8. looks_like_context_exceeded: matches known messages ────────────────

    #[test]
    fn test_looks_like_context_exceeded_matches_common_messages() {
        let msgs = [
            "Error code: 400 - {'error': 'Context size has been exceeded.'}",
            "n_keep: 22374 >= n_ctx: 4096",
            "context_length_exceeded: This model's maximum context length is 8192 tokens",
            "exceeds the available context size",
            "The prompt is too long for this model.",
        ];
        for m in &msgs {
            assert!(looks_like_context_exceeded(m), "should match: {m}");
        }
    }

    // ── 9. looks_like_context_exceeded: ignores unrelated errors ─────────────

    #[test]
    fn test_looks_like_context_exceeded_ignores_unrelated_errors() {
        let msgs = [
            "timeout",
            "rate limit",
            "401 unauthorized",
            "connection refused",
        ];
        for m in &msgs {
            assert!(!looks_like_context_exceeded(m), "should not match: {m}");
        }
    }

    // ── 10. adaptive_retry: bisects on context exceeded ───────────────────────

    #[test]
    fn test_adaptive_retry_splits_on_context_exceeded() {
        let dir = tempfile::tempdir().unwrap();
        let files: Vec<PathBuf> = (0..4)
            .map(|i| {
                let p = dir.path().join(format!("f{i}.md"));
                std::fs::write(&p, "hello").unwrap();
                p
            })
            .collect();

        let call_count = Arc::new(AtomicUsize::new(0));
        let cc = call_count.clone();

        let extractor = move |chunk: &[PathBuf]| -> Result<LlmResult> {
            cc.fetch_add(1, Ordering::SeqCst);
            if chunk.len() == 4 {
                return Err(CodeSynapseError::Validation(
                    "Error 400: Context size has been exceeded.".into(),
                ));
            }
            Ok(LlmResult {
                nodes: chunk
                    .iter()
                    .map(|f| json!({"id": f.file_stem().unwrap().to_str().unwrap()}))
                    .collect(),
                finish_reason: "stop".to_string(),
                ..Default::default()
            })
        };

        let result = extract_with_adaptive_retry(&files, "kimi", 3, 0, &extractor).unwrap();

        assert_eq!(result.nodes.len(), 4);
        assert_eq!(call_count.load(Ordering::SeqCst), 3);
    }

    // ── 11. adaptive_retry: single-file overflow returns empty fragment ────────

    #[test]
    fn test_adaptive_retry_gives_up_on_single_file_overflow() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("huge.md");
        std::fs::write(&f, "x").unwrap();

        let extractor = |_: &[PathBuf]| -> Result<LlmResult> {
            Err(CodeSynapseError::Validation(
                "context_length_exceeded".into(),
            ))
        };

        let result = extract_with_adaptive_retry(&[f], "kimi", 3, 0, &extractor).unwrap();
        assert_eq!(result.nodes.len(), 0);
        assert_eq!(result.edges.len(), 0);
        assert_eq!(result.finish_reason, "stop");
    }

    // ── 12. adaptive_retry: re-raises unrelated errors ────────────────────────

    #[test]
    fn test_adaptive_retry_re_raises_unrelated_errors() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("f.md");
        std::fs::write(&f, "x").unwrap();

        let extractor = |_: &[PathBuf]| -> Result<LlmResult> {
            Err(CodeSynapseError::Validation("rate limit hit".into()))
        };

        let err = extract_with_adaptive_retry(&[f], "kimi", 3, 0, &extractor).unwrap_err();
        assert!(err.to_string().contains("rate limit"));
    }

    // ── 13. response_is_hollow: empty string ─────────────────────────────────

    #[test]
    fn test_response_is_hollow_flags_empty_string() {
        let parsed = json!({"nodes": [], "edges": [], "hyperedges": []});
        assert!(response_is_hollow(Some(""), &parsed));
    }

    // ── 14. response_is_hollow: None content ─────────────────────────────────

    #[test]
    fn test_response_is_hollow_flags_none_content() {
        let parsed = json!({"nodes": [], "edges": [], "hyperedges": []});
        assert!(response_is_hollow(None, &parsed));
    }

    // ── 15. response_is_hollow: whitespace only ───────────────────────────────

    #[test]
    fn test_response_is_hollow_flags_whitespace_only() {
        let parsed = json!({"nodes": [], "edges": [], "hyperedges": []});
        assert!(response_is_hollow(Some("   \n\t  "), &parsed));
    }

    // ── 16. response_is_hollow: parsed but no nodes/edges ────────────────────

    #[test]
    fn test_response_is_hollow_flags_parsed_but_no_nodes_or_edges() {
        assert!(response_is_hollow(
            Some(r#"{"sorry": "I cannot"}"#),
            &json!({})
        ));
        assert!(response_is_hollow(
            Some("{}"),
            &json!({"nodes": [], "edges": [], "hyperedges": []})
        ));
    }

    // ── 17. response_is_hollow: real extraction is not hollow ────────────────

    #[test]
    fn test_response_is_hollow_accepts_real_extraction() {
        let parsed = json!({"nodes": [{"id": "x"}], "edges": [], "hyperedges": []});
        assert!(!response_is_hollow(
            Some(r#"{"nodes":[{"id":"x"}]}"#),
            &parsed
        ));

        let parsed2 =
            json!({"nodes": [], "edges": [{"source": "a", "target": "b"}], "hyperedges": []});
        assert!(!response_is_hollow(Some(r#"{"edges":[...]}"#), &parsed2));
    }

    // ── 18. process response: empty content → finish_reason = "length" ────────

    #[test]
    fn test_call_openai_compat_relabels_empty_content_as_length() {
        let result =
            process_openai_compat_response(Some(""), "stop", 100, 0, "qwen2.5-coder:7b", "ollama");
        assert_eq!(
            result.finish_reason, "length",
            "empty content from a 'successful' call must be re-labelled to trigger bisection"
        );
    }

    // ── 19. process response: None content → finish_reason = "length" ─────────

    #[test]
    fn test_call_openai_compat_relabels_none_content_as_length() {
        let result =
            process_openai_compat_response(None, "stop", 100, 0, "qwen2.5-coder:7b", "ollama");
        assert_eq!(result.finish_reason, "length");
    }

    // ── 20. process response: unparseable JSON → finish_reason = "length" ─────

    #[test]
    fn test_call_openai_compat_relabels_unparseable_json_as_length() {
        let result = process_openai_compat_response(
            Some(r#"{"nodes": [{"id":"#),
            "stop",
            100,
            20,
            "qwen2.5-coder:7b",
            "ollama",
        );
        assert_eq!(result.finish_reason, "length");
    }

    // ── 21. process response: real extraction preserves finish_reason ──────────

    #[test]
    fn test_call_openai_compat_preserves_real_finish_reason() {
        let result = process_openai_compat_response(
            Some(r#"{"nodes":[{"id":"a"}],"edges":[],"hyperedges":[]}"#),
            "stop",
            100,
            200,
            "m",
            "kimi",
        );
        assert_eq!(result.finish_reason, "stop");
        assert_eq!(result.nodes.len(), 1);
    }

    // ── 22. Ollama extra_body: num_ctx and keep_alive ─────────────────────────

    #[test]
    fn test_ollama_extra_body_sets_num_ctx_and_keep_alive() {
        with_env(
            &[],
            &[
                "CODESYNAPSE_OLLAMA_NUM_CTX",
                "CODESYNAPSE_OLLAMA_KEEP_ALIVE",
            ],
            || {
                let body = build_extra_body("ollama", "user msg", 8192).unwrap();
                assert!(
                    body.num_ctx >= 8192,
                    "num_ctx must be at least the floor value, got {}",
                    body.num_ctx
                );
                assert_eq!(body.keep_alive, "30m");
            },
        );
    }

    // ── 23. Ollama num_ctx scales with small token budget ─────────────────────

    #[test]
    fn test_ollama_num_ctx_scales_with_small_token_budget() {
        with_env(
            &[],
            &[
                "CODESYNAPSE_OLLAMA_NUM_CTX",
                "CODESYNAPSE_OLLAMA_KEEP_ALIVE",
            ],
            || {
                let small_msg = "x".repeat(32_000);
                let num_ctx = compute_ollama_num_ctx(small_msg.len(), 16384);
                assert!(
                    num_ctx < 131_072,
                    "num_ctx={num_ctx} is too large for a small chunk; wastes VRAM (#798)"
                );
                assert!(
                    num_ctx >= 8192,
                    "num_ctx must cover at least the output cap"
                );
            },
        );
    }

    // ── 24. Ollama num_ctx env override ──────────────────────────────────────

    #[test]
    fn test_ollama_num_ctx_env_override() {
        with_env(
            &[("CODESYNAPSE_OLLAMA_NUM_CTX", "65536")],
            &["CODESYNAPSE_OLLAMA_KEEP_ALIVE"],
            || {
                let num_ctx = compute_ollama_num_ctx(100, 8192);
                assert_eq!(num_ctx, 65536);
            },
        );
    }

    // ── 25. non-Ollama backend gets no extra_body ─────────────────────────────

    #[test]
    fn test_non_ollama_backend_gets_no_num_ctx_extra_body() {
        let body = build_extra_body("openai", "u", 8192);
        assert!(
            body.is_none(),
            "non-ollama backends must not get num_ctx injection"
        );
    }

    // ── 26. extract_corpus_parallel: Ollama runs serially ────────────────────

    #[test]
    fn test_extract_corpus_parallel_ollama_runs_serially() {
        with_env(&[], &["CODESYNAPSE_OLLAMA_PARALLEL"], || {
            let dir = tempfile::tempdir().unwrap();
            let files: Vec<PathBuf> = (0..6)
                .map(|i| {
                    let p = dir.path().join(format!("f{i}.md"));
                    std::fs::write(&p, "hello").unwrap();
                    p
                })
                .collect();

            let extractor = |chunk: &[PathBuf]| -> Result<LlmResult> {
                Ok(LlmResult {
                    nodes: chunk
                        .iter()
                        .map(|f| json!({"id": f.file_stem().unwrap().to_str().unwrap()}))
                        .collect(),
                    finish_reason: "stop".to_string(),
                    ..Default::default()
                })
            };

            let result = extract_corpus_parallel_with(&files, "ollama", 2, 4, 3, extractor);
            assert_eq!(result.nodes.len(), 6);
        });
    }

    // ── 27. extract_corpus_parallel: CODESYNAPSE_OLLAMA_PARALLEL=1 uses concurrency

    #[test]
    fn test_extract_corpus_parallel_ollama_parallel_env_restores_concurrency() {
        with_env(&[("CODESYNAPSE_OLLAMA_PARALLEL", "1")], &[], || {
            let workers = effective_max_concurrency("ollama", 4);
            assert_eq!(
                workers, 4,
                "CODESYNAPSE_OLLAMA_PARALLEL=1 should restore concurrency"
            );
        });
    }

    // ── 28. adaptive_retry: bisects on hollow Ollama response ────────────────

    #[test]
    fn test_adaptive_retry_bisects_on_hollow_ollama_response() {
        let dir = tempfile::tempdir().unwrap();
        let files: Vec<PathBuf> = (0..4)
            .map(|i| {
                let p = dir.path().join(format!("f{i}.md"));
                std::fs::write(&p, "hello").unwrap();
                p
            })
            .collect();

        let call_count = Arc::new(AtomicUsize::new(0));
        let cc = call_count.clone();

        let extractor = move |chunk: &[PathBuf]| -> Result<LlmResult> {
            cc.fetch_add(1, Ordering::SeqCst);
            if chunk.len() == 4 {
                return Ok(LlmResult {
                    nodes: vec![],
                    edges: vec![],
                    hyperedges: vec![],
                    input_tokens: 100,
                    output_tokens: 0,
                    model: Some("m".into()),
                    finish_reason: "length".to_string(),
                });
            }
            Ok(LlmResult {
                nodes: chunk
                    .iter()
                    .map(|f| json!({"id": f.file_stem().unwrap().to_str().unwrap()}))
                    .collect(),
                finish_reason: "stop".to_string(),
                ..Default::default()
            })
        };

        let result = extract_with_adaptive_retry(&files, "ollama", 3, 0, &extractor).unwrap();

        assert_eq!(
            result.nodes.len(),
            4,
            "bisection should recover all 4 nodes after hollow response"
        );
        assert_eq!(call_count.load(Ordering::SeqCst), 3);
    }
}
