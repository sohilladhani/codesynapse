use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;

pub const FILE_CHAR_CAP: usize = 20_000;
pub const PER_FILE_OVERHEAD_CHARS: usize = 80;
pub const CHARS_PER_TOKEN: usize = 4;
pub const DEFAULT_TOKEN_BUDGET: usize = 60_000;

#[derive(Debug, Clone)]
pub struct ChunkResult {
    pub nodes: Vec<serde_json::Value>,
    pub edges: Vec<serde_json::Value>,
    pub hyperedges: Vec<serde_json::Value>,
    pub input_tokens: usize,
    pub output_tokens: usize,
    pub finish_reason: String,
    pub model: Option<String>,
    pub failed_chunks: usize,
}

impl ChunkResult {
    pub fn empty() -> Self {
        ChunkResult {
            nodes: vec![],
            edges: vec![],
            hyperedges: vec![],
            input_tokens: 0,
            output_tokens: 0,
            finish_reason: "stop".to_string(),
            model: None,
            failed_chunks: 0,
        }
    }

    pub fn merge(mut self, other: ChunkResult) -> ChunkResult {
        self.nodes.extend(other.nodes);
        self.edges.extend(other.edges);
        self.hyperedges.extend(other.hyperedges);
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
        self.failed_chunks += other.failed_chunks;
        self.finish_reason = "stop".to_string();
        self
    }
}

pub fn read_chunk_content(path: &Path) -> String {
    match std::fs::read(path) {
        Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        Err(_) => String::new(),
    }
}

pub fn estimate_file_tokens(path: &Path) -> usize {
    let size = match path.metadata() {
        Ok(m) => m.len() as usize,
        Err(_) => return 0,
    };
    let chars = size.min(FILE_CHAR_CAP) + PER_FILE_OVERHEAD_CHARS;
    chars / CHARS_PER_TOKEN
}

pub fn pack_chunks_by_tokens(
    files: &[PathBuf],
    token_budget: usize,
) -> Result<Vec<Vec<PathBuf>>, String> {
    if token_budget == 0 {
        return Err(format!("token_budget must be positive, got {token_budget}"));
    }

    let mut by_dir: HashMap<PathBuf, Vec<PathBuf>> = HashMap::new();
    for f in files {
        let parent = f.parent().unwrap_or(Path::new(".")).to_path_buf();
        by_dir.entry(parent).or_default().push(f.clone());
    }

    let mut sorted_dirs: Vec<PathBuf> = by_dir.keys().cloned().collect();
    sorted_dirs.sort();

    let mut chunks: Vec<Vec<PathBuf>> = vec![];
    let mut current: Vec<PathBuf> = vec![];
    let mut current_tokens: usize = 0;

    for dir in &sorted_dirs {
        for path in &by_dir[dir] {
            let cost = estimate_file_tokens(path);
            if !current.is_empty() && current_tokens + cost > token_budget {
                chunks.push(current.clone());
                current = vec![];
                current_tokens = 0;
            }
            current.push(path.clone());
            current_tokens += cost;
        }
    }

    if !current.is_empty() {
        chunks.push(current);
    }
    Ok(chunks)
}

fn looks_like_context_exceeded(msg: &str) -> bool {
    let lower = msg.to_lowercase();
    let markers = [
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
    markers.iter().any(|m| lower.contains(m))
}

pub fn extract_with_adaptive_retry<F>(
    chunk: &[PathBuf],
    extractor: &F,
    max_depth: usize,
    depth: usize,
) -> ChunkResult
where
    F: Fn(&[PathBuf]) -> Result<ChunkResult, String> + Sync,
{
    match extractor(chunk) {
        Err(e) => {
            if !looks_like_context_exceeded(&e) {
                eprintln!("[codesynapse] chunk extraction failed: {e}");
                return ChunkResult {
                    failed_chunks: 1,
                    ..ChunkResult::empty()
                };
            }
            if chunk.len() <= 1 {
                eprintln!(
                    "[codesynapse] single-file chunk {:?} exceeds model context and cannot be split further: {}",
                    chunk.first(),
                    e
                );
                return ChunkResult::empty();
            }
            if depth >= max_depth {
                eprintln!(
                    "[codesynapse] chunk of {} still overflows context at recursion depth {} (max {}) — dropping",
                    chunk.len(),
                    depth,
                    max_depth
                );
                return ChunkResult::empty();
            }
            eprintln!(
                "[codesynapse] chunk of {} exceeded context at depth {} ({}); splitting in half and retrying",
                chunk.len(),
                depth,
                e
            );
            let mid = chunk.len() / 2;
            let left = extract_with_adaptive_retry(&chunk[..mid], extractor, max_depth, depth + 1);
            let right = extract_with_adaptive_retry(&chunk[mid..], extractor, max_depth, depth + 1);
            left.merge(right)
        }
        Ok(result) => {
            if result.finish_reason != "length" {
                return result;
            }
            if chunk.len() <= 1 {
                eprintln!(
                    "[codesynapse] single-file chunk {:?} truncated at max_completion_tokens — partial result kept",
                    chunk.first()
                );
                return result;
            }
            if depth >= max_depth {
                eprintln!(
                    "[codesynapse] chunk of {} still truncated at recursion depth {} (max {}) — partial result kept",
                    chunk.len(),
                    depth,
                    max_depth
                );
                return result;
            }
            eprintln!(
                "[codesynapse] chunk of {} truncated at depth {}, splitting into halves of {} and {}",
                chunk.len(),
                depth,
                chunk.len() / 2,
                chunk.len() - chunk.len() / 2
            );
            let mid = chunk.len() / 2;
            let left = extract_with_adaptive_retry(&chunk[..mid], extractor, max_depth, depth + 1);
            let right = extract_with_adaptive_retry(&chunk[mid..], extractor, max_depth, depth + 1);
            left.merge(right)
        }
    }
}

pub struct CorpusParallelConfig {
    pub chunk_size: usize,
    pub token_budget: Option<usize>,
    pub max_concurrency: usize,
    pub max_retry_depth: usize,
}

impl Default for CorpusParallelConfig {
    fn default() -> Self {
        CorpusParallelConfig {
            chunk_size: 20,
            token_budget: Some(DEFAULT_TOKEN_BUDGET),
            max_concurrency: 4,
            max_retry_depth: 3,
        }
    }
}

pub fn extract_corpus_parallel<F, C>(
    files: &[PathBuf],
    config: CorpusParallelConfig,
    extractor: F,
    on_chunk_done: Option<C>,
) -> ChunkResult
where
    F: Fn(&[PathBuf]) -> Result<ChunkResult, String> + Sync + Send + 'static,
    C: Fn(usize, usize, &ChunkResult) + Send + Sync + 'static,
{
    let chunks: Vec<Vec<PathBuf>> = if let Some(budget) = config.token_budget {
        pack_chunks_by_tokens(files, budget).unwrap_or_else(|_| {
            files
                .chunks(config.chunk_size)
                .map(|c| c.to_vec())
                .collect()
        })
    } else {
        files
            .chunks(config.chunk_size)
            .map(|c| c.to_vec())
            .collect()
    };

    let total = chunks.len();
    let max_retry_depth = config.max_retry_depth;
    let workers = config.max_concurrency.max(1).min(total.max(1));

    let merged = Arc::new(Mutex::new(ChunkResult::empty()));
    let extractor = Arc::new(extractor);
    let on_chunk_done: Arc<Option<C>> = Arc::new(on_chunk_done);

    if workers == 1 {
        for (idx, chunk) in chunks.iter().enumerate() {
            let result = extract_with_adaptive_retry(chunk, extractor.as_ref(), max_retry_depth, 0);
            {
                let mut m = merged.lock().unwrap();
                *m = ChunkResult {
                    nodes: {
                        let mut v = m.nodes.clone();
                        v.extend(result.nodes.clone());
                        v
                    },
                    edges: {
                        let mut v = m.edges.clone();
                        v.extend(result.edges.clone());
                        v
                    },
                    hyperedges: {
                        let mut v = m.hyperedges.clone();
                        v.extend(result.hyperedges.clone());
                        v
                    },
                    input_tokens: m.input_tokens + result.input_tokens,
                    output_tokens: m.output_tokens + result.output_tokens,
                    failed_chunks: m.failed_chunks + result.failed_chunks,
                    finish_reason: "stop".to_string(),
                    model: m.model.clone(),
                };
            }
            if let Some(cb) = on_chunk_done.as_ref() {
                cb(idx, total, &result);
            }
        }
    } else {
        let mut handles = vec![];
        for (idx, chunk) in chunks.into_iter().enumerate() {
            let ext = Arc::clone(&extractor);
            let merged_clone = Arc::clone(&merged);
            let cb_clone = Arc::clone(&on_chunk_done);

            let handle = thread::spawn(move || {
                let result = extract_with_adaptive_retry(&chunk, ext.as_ref(), max_retry_depth, 0);
                {
                    let mut m = merged_clone.lock().unwrap();
                    m.nodes.extend(result.nodes.clone());
                    m.edges.extend(result.edges.clone());
                    m.hyperedges.extend(result.hyperedges.clone());
                    m.input_tokens += result.input_tokens;
                    m.output_tokens += result.output_tokens;
                    m.failed_chunks += result.failed_chunks;
                }
                if let Some(cb) = cb_clone.as_ref() {
                    cb(idx, total, &result);
                }
            });
            handles.push(handle);
        }
        for h in handles {
            let _ = h.join();
        }
    }

    let result = Arc::try_unwrap(merged).unwrap().into_inner().unwrap();
    if result.failed_chunks > 0 {
        eprintln!(
            "[codesynapse] WARNING: {}/{} chunks failed during extraction",
            result.failed_chunks, total
        );
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn make_files(dir: &Path, names: &[&str], content: &str) -> Vec<PathBuf> {
        names
            .iter()
            .map(|n| {
                let p = dir.join(n);
                fs::write(&p, content).unwrap();
                p
            })
            .collect()
    }

    // ---- estimate_file_tokens ------------------------------------------------

    #[test]
    fn test_estimate_file_tokens_chars_fallback() {
        let tmp = TempDir::new().unwrap();
        let f = tmp.path().join("x.py");
        fs::write(&f, "x".repeat(1000)).unwrap();
        // 1000 bytes (capped at 20000) + 80 overhead = 1080 / 4 = 270
        assert_eq!(estimate_file_tokens(&f), 270);
    }

    #[test]
    fn test_estimate_file_tokens_missing_file() {
        assert_eq!(estimate_file_tokens(Path::new("/does/not/exist.py")), 0);
    }

    #[test]
    fn test_estimate_file_tokens_caps_at_file_char_cap() {
        let tmp = TempDir::new().unwrap();
        let f = tmp.path().join("big.py");
        fs::write(&f, "x".repeat(100_000)).unwrap();
        // capped at 20000 + 80 = 20080 / 4 = 5020
        assert_eq!(estimate_file_tokens(&f), 5020);
    }

    // ---- pack_chunks_by_tokens -----------------------------------------------

    #[test]
    fn test_pack_chunks_packs_small_files_together() {
        let tmp = TempDir::new().unwrap();
        let files = make_files(
            tmp.path(),
            &(0..20)
                .map(|i| format!("small_{i}.py"))
                .collect::<Vec<_>>()
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>(),
            "x = 1\n",
        );
        let chunks = pack_chunks_by_tokens(&files, 10_000).unwrap();
        assert_eq!(chunks.len(), 1);
        let mut got = chunks[0].clone();
        let mut want = files.clone();
        got.sort();
        want.sort();
        assert_eq!(got, want);
    }

    #[test]
    fn test_pack_chunks_starts_new_chunk_when_budget_would_overflow() {
        let tmp = TempDir::new().unwrap();
        // Each 10000-char file: (10000+80)/4 = 2520 tokens.
        // Budget 6000 fits 2 (5040 < 6000) but not 3 (7560 > 6000).
        // 5 files → [2, 2, 1]
        let files = make_files(
            tmp.path(),
            &[
                "file_0.py",
                "file_1.py",
                "file_2.py",
                "file_3.py",
                "file_4.py",
            ],
            &"x".repeat(10_000),
        );
        let chunks = pack_chunks_by_tokens(&files, 6_000).unwrap();
        let sizes: Vec<usize> = chunks.iter().map(|c| c.len()).collect();
        assert_eq!(sizes, vec![2, 2, 1]);
        assert_eq!(sizes.iter().sum::<usize>(), 5);
    }

    #[test]
    fn test_pack_chunks_groups_by_directory() {
        let tmp = TempDir::new().unwrap();
        let dir_a = tmp.path().join("a");
        let dir_b = tmp.path().join("b");
        fs::create_dir_all(&dir_a).unwrap();
        fs::create_dir_all(&dir_b).unwrap();

        let a1 = dir_a.join("x.py");
        fs::write(&a1, "a").unwrap();
        let a2 = dir_a.join("y.py");
        fs::write(&a2, "a").unwrap();
        let b1 = dir_b.join("x.py");
        fs::write(&b1, "b").unwrap();
        let b2 = dir_b.join("y.py");
        fs::write(&b2, "b").unwrap();

        let chunks =
            pack_chunks_by_tokens(&[a1.clone(), b1.clone(), a2.clone(), b2.clone()], 1_000_000)
                .unwrap();
        assert_eq!(chunks.len(), 1);
        let chunk = &chunks[0];
        let a_indices: Vec<usize> = chunk
            .iter()
            .enumerate()
            .filter(|(_, p)| p.parent() == Some(dir_a.as_path()))
            .map(|(i, _)| i)
            .collect();
        let b_indices: Vec<usize> = chunk
            .iter()
            .enumerate()
            .filter(|(_, p)| p.parent() == Some(dir_b.as_path()))
            .map(|(i, _)| i)
            .collect();
        assert_eq!(a_indices, a_indices.clone());
        assert_eq!(b_indices, b_indices.clone());
        assert!(
            a_indices.iter().max().unwrap() < b_indices.iter().min().unwrap()
                || b_indices.iter().max().unwrap() < a_indices.iter().min().unwrap()
        );
    }

    #[test]
    fn test_pack_chunks_oversized_file_gets_its_own_chunk() {
        let tmp = TempDir::new().unwrap();
        let big = tmp.path().join("big.py");
        fs::write(&big, "x".repeat(200_000)).unwrap();
        let small = tmp.path().join("small.py");
        fs::write(&small, "x").unwrap();

        let chunks = pack_chunks_by_tokens(&[big, small], 1_000).unwrap();
        let sizes: Vec<usize> = chunks.iter().map(|c| c.len()).collect();
        assert_eq!(sizes, vec![1, 1]);
    }

    #[test]
    fn test_pack_chunks_rejects_zero_budget() {
        let tmp = TempDir::new().unwrap();
        let f = tmp.path().join("x.py");
        fs::write(&f, "a").unwrap();
        assert!(pack_chunks_by_tokens(&[f], 0).is_err());
    }

    // ---- extract_with_adaptive_retry ----------------------------------------

    fn stub_result(file_count: usize, finish_reason: &str) -> ChunkResult {
        ChunkResult {
            nodes: (0..file_count)
                .map(|i| serde_json::json!({"id": format!("n_{i}")}))
                .collect(),
            edges: vec![],
            hyperedges: vec![],
            input_tokens: 100 * file_count,
            output_tokens: 50 * file_count,
            finish_reason: finish_reason.to_string(),
            model: None,
            failed_chunks: 0,
        }
    }

    #[test]
    fn test_adaptive_retry_returns_directly_when_not_truncated() {
        let tmp = TempDir::new().unwrap();
        let files: Vec<PathBuf> = (0..4)
            .map(|i| {
                let f = tmp.path().join(format!("f{i}.py"));
                fs::write(&f, "x").unwrap();
                f
            })
            .collect();

        let call_count = Arc::new(Mutex::new(0usize));
        let cc = Arc::clone(&call_count);
        let extractor = move |chunk: &[PathBuf]| -> Result<ChunkResult, String> {
            *cc.lock().unwrap() += 1;
            Ok(stub_result(chunk.len(), "stop"))
        };

        let result = extract_with_adaptive_retry(&files, &extractor, 3, 0);
        assert_eq!(*call_count.lock().unwrap(), 1);
        assert_eq!(result.nodes.len(), 4);
    }

    #[test]
    fn test_adaptive_retry_splits_when_finish_reason_length() {
        let tmp = TempDir::new().unwrap();
        let files: Vec<PathBuf> = (0..4)
            .map(|i| {
                let f = tmp.path().join(format!("f{i}.py"));
                fs::write(&f, "x").unwrap();
                f
            })
            .collect();

        let call_sizes = Arc::new(Mutex::new(vec![]));
        let cs = Arc::clone(&call_sizes);
        let extractor = move |chunk: &[PathBuf]| -> Result<ChunkResult, String> {
            cs.lock().unwrap().push(chunk.len());
            let finish = if chunk.len() == 4 { "length" } else { "stop" };
            Ok(stub_result(chunk.len(), finish))
        };

        let result = extract_with_adaptive_retry(&files, &extractor, 3, 0);
        let mut sizes = call_sizes.lock().unwrap().clone();
        sizes.sort();
        assert_eq!(sizes, vec![2, 2, 4]);
        assert_eq!(result.nodes.len(), 4);
        assert_eq!(result.finish_reason, "stop");
    }

    #[test]
    fn test_adaptive_retry_recurses_for_persistent_truncation() {
        let tmp = TempDir::new().unwrap();
        let files: Vec<PathBuf> = (0..8)
            .map(|i| {
                let f = tmp.path().join(format!("f{i}.py"));
                fs::write(&f, "x").unwrap();
                f
            })
            .collect();

        let call_sizes = Arc::new(Mutex::new(vec![]));
        let cs = Arc::clone(&call_sizes);
        let extractor = move |chunk: &[PathBuf]| -> Result<ChunkResult, String> {
            cs.lock().unwrap().push(chunk.len());
            let finish = if chunk.len() > 2 { "length" } else { "stop" };
            Ok(stub_result(chunk.len(), finish))
        };

        let result = extract_with_adaptive_retry(&files, &extractor, 3, 0);
        let mut sizes = call_sizes.lock().unwrap().clone();
        sizes.sort();
        assert_eq!(sizes, vec![2, 2, 2, 2, 4, 4, 8]);
        assert_eq!(result.nodes.len(), 8);
    }

    #[test]
    fn test_adaptive_retry_caps_at_max_depth() {
        let tmp = TempDir::new().unwrap();
        let files: Vec<PathBuf> = (0..8)
            .map(|i| {
                let f = tmp.path().join(format!("f{i}.py"));
                fs::write(&f, "x").unwrap();
                f
            })
            .collect();

        let call_count = Arc::new(Mutex::new(0usize));
        let cc = Arc::clone(&call_count);
        let extractor = move |chunk: &[PathBuf]| -> Result<ChunkResult, String> {
            *cc.lock().unwrap() += 1;
            Ok(stub_result(chunk.len(), "length"))
        };

        // max_depth=2 bounds: root(1) + 2 halves + 4 quarters = 7 max
        extract_with_adaptive_retry(&files, &extractor, 2, 0);
        assert!(*call_count.lock().unwrap() <= 7);
    }

    #[test]
    fn test_adaptive_retry_single_file_truncation_does_not_recurse() {
        let tmp = TempDir::new().unwrap();
        let f = tmp.path().join("huge.py");
        fs::write(&f, "x").unwrap();

        let call_count = Arc::new(Mutex::new(0usize));
        let cc = Arc::clone(&call_count);
        let extractor = move |chunk: &[PathBuf]| -> Result<ChunkResult, String> {
            *cc.lock().unwrap() += 1;
            Ok(stub_result(chunk.len(), "length"))
        };

        extract_with_adaptive_retry(&[f], &extractor, 3, 0);
        assert_eq!(*call_count.lock().unwrap(), 1);
    }

    // ---- extract_corpus_parallel --------------------------------------------

    #[test]
    fn test_corpus_parallel_legacy_mode_when_token_budget_is_none() {
        let tmp = TempDir::new().unwrap();
        let files: Vec<PathBuf> = (0..45)
            .map(|i| {
                let f = tmp.path().join(format!("f{i}.py"));
                fs::write(&f, "x").unwrap();
                f
            })
            .collect();

        let chunks_seen = Arc::new(Mutex::new(vec![]));
        let cs = Arc::clone(&chunks_seen);

        let result = extract_corpus_parallel(
            &files,
            CorpusParallelConfig {
                chunk_size: 20,
                token_budget: None,
                max_concurrency: 1,
                max_retry_depth: 3,
            },
            move |chunk: &[PathBuf]| -> Result<ChunkResult, String> {
                cs.lock().unwrap().push(chunk.len());
                Ok(stub_result(chunk.len(), "stop"))
            },
            None::<fn(usize, usize, &ChunkResult)>,
        );

        let sizes = chunks_seen.lock().unwrap().clone();
        assert_eq!(sizes, vec![20, 20, 5]);
        assert_eq!(result.nodes.len(), 45);
    }

    #[test]
    fn test_corpus_parallel_token_budget_default_packs_files() {
        let tmp = TempDir::new().unwrap();
        let files: Vec<PathBuf> = (0..50)
            .map(|i| {
                let f = tmp.path().join(format!("f{i}.py"));
                fs::write(&f, "x = 1\n").unwrap();
                f
            })
            .collect();

        let chunks_seen = Arc::new(Mutex::new(vec![]));
        let cs = Arc::clone(&chunks_seen);

        extract_corpus_parallel(
            &files,
            CorpusParallelConfig {
                token_budget: Some(DEFAULT_TOKEN_BUDGET),
                max_concurrency: 1,
                ..Default::default()
            },
            move |chunk: &[PathBuf]| -> Result<ChunkResult, String> {
                cs.lock().unwrap().push(chunk.len());
                Ok(stub_result(chunk.len(), "stop"))
            },
            None::<fn(usize, usize, &ChunkResult)>,
        );

        let sizes = chunks_seen.lock().unwrap().clone();
        assert_eq!(sizes.len(), 1);
        assert_eq!(sizes[0], 50);
    }

    #[test]
    fn test_corpus_parallel_sequential_when_max_concurrency_is_one() {
        let tmp = TempDir::new().unwrap();
        let files: Vec<PathBuf> = (0..3)
            .map(|i| {
                let f = tmp.path().join(format!("f{i}.py"));
                fs::write(&f, "x").unwrap();
                f
            })
            .collect();

        let call_order = Arc::new(Mutex::new(vec![]));
        let co = Arc::clone(&call_order);

        extract_corpus_parallel(
            &files,
            CorpusParallelConfig {
                chunk_size: 1,
                token_budget: None,
                max_concurrency: 1,
                max_retry_depth: 3,
            },
            move |chunk: &[PathBuf]| -> Result<ChunkResult, String> {
                co.lock()
                    .unwrap()
                    .push(chunk[0].file_name().unwrap().to_string_lossy().to_string());
                Ok(stub_result(chunk.len(), "stop"))
            },
            None::<fn(usize, usize, &ChunkResult)>,
        );

        let order = call_order.lock().unwrap().clone();
        assert_eq!(order, vec!["f0.py", "f1.py", "f2.py"]);
    }

    #[test]
    fn test_corpus_parallel_continues_after_chunk_failure() {
        let tmp = TempDir::new().unwrap();
        let files: Vec<PathBuf> = (0..4)
            .map(|i| {
                let f = tmp.path().join(format!("f{i}.py"));
                fs::write(&f, "x").unwrap();
                f
            })
            .collect();

        let call_count = Arc::new(Mutex::new(0usize));
        let cc = Arc::clone(&call_count);

        let result = extract_corpus_parallel(
            &files,
            CorpusParallelConfig {
                chunk_size: 1,
                token_budget: None,
                max_concurrency: 1,
                max_retry_depth: 3,
            },
            move |chunk: &[PathBuf]| -> Result<ChunkResult, String> {
                let n = {
                    let mut c = cc.lock().unwrap();
                    *c += 1;
                    *c
                };
                if n == 2 {
                    return Err("simulated API error".to_string());
                }
                Ok(stub_result(chunk.len(), "stop"))
            },
            None::<fn(usize, usize, &ChunkResult)>,
        );

        // 4 chunks, 1 failed → 3 chunks contributed 1 node each
        assert_eq!(result.nodes.len(), 3);
    }

    #[test]
    fn test_corpus_parallel_uses_adaptive_retry() {
        let tmp = TempDir::new().unwrap();
        let files: Vec<PathBuf> = (0..4)
            .map(|i| {
                let f = tmp.path().join(format!("f{i}.py"));
                fs::write(&f, "x").unwrap();
                f
            })
            .collect();

        let call_sizes = Arc::new(Mutex::new(vec![]));
        let cs = Arc::clone(&call_sizes);
        let chunk_done_args = Arc::new(Mutex::new(vec![]));
        let cda = Arc::clone(&chunk_done_args);

        let result = extract_corpus_parallel(
            &files,
            CorpusParallelConfig {
                chunk_size: 4,
                token_budget: None,
                max_concurrency: 1,
                max_retry_depth: 3,
            },
            move |chunk: &[PathBuf]| -> Result<ChunkResult, String> {
                cs.lock().unwrap().push(chunk.len());
                let finish = if chunk.len() == 4 { "length" } else { "stop" };
                Ok(stub_result(chunk.len(), finish))
            },
            Some(move |idx: usize, total: usize, r: &ChunkResult| {
                cda.lock().unwrap().push((idx, total, r.nodes.len()));
            }),
        );

        let sizes = call_sizes.lock().unwrap().clone();
        assert_eq!(sizes, vec![4, 2, 2]);
        let done = chunk_done_args.lock().unwrap().clone();
        assert_eq!(done.len(), 1);
        assert_eq!(done[0], (0, 1, 4));
        assert_eq!(result.nodes.len(), 4);
    }

    #[test]
    fn test_corpus_parallel_runs_chunks_concurrently() {
        use std::time::Instant;

        let tmp = TempDir::new().unwrap();
        let files: Vec<PathBuf> = (0..8)
            .map(|i| {
                let f = tmp.path().join(format!("f{i}.py"));
                fs::write(&f, "x").unwrap();
                f
            })
            .collect();

        let t0 = Instant::now();
        extract_corpus_parallel(
            &files,
            CorpusParallelConfig {
                chunk_size: 2,
                token_budget: None,
                max_concurrency: 4,
                max_retry_depth: 3,
            },
            move |chunk: &[PathBuf]| -> Result<ChunkResult, String> {
                thread::sleep(std::time::Duration::from_millis(300));
                Ok(stub_result(chunk.len(), "stop"))
            },
            None::<fn(usize, usize, &ChunkResult)>,
        );
        let elapsed = t0.elapsed().as_secs_f64();
        // 4 chunks × 300ms sequential = 1.2s; parallel ≤ 4 workers ≈ 0.3-0.6s
        assert!(
            elapsed < 1.0,
            "expected parallel speedup, took {elapsed:.2}s"
        );
    }

    // ---- failed_chunks tracking (parity: test_charmap_encoding.py) ----------

    #[test]
    fn test_failed_chunks_zero_when_all_succeed() {
        let tmp = TempDir::new().unwrap();
        let files = make_files(tmp.path(), &["a.py", "b.py", "c.py"], "x = 1\n");
        let result = extract_corpus_parallel(
            &files,
            CorpusParallelConfig {
                chunk_size: 1,
                token_budget: None,
                max_concurrency: 1,
                max_retry_depth: 0,
            },
            |chunk: &[PathBuf]| Ok(stub_result(chunk.len(), "stop")),
            None::<fn(usize, usize, &ChunkResult)>,
        );
        assert_eq!(result.failed_chunks, 0);
    }

    #[test]
    fn test_failed_chunks_increments_on_api_error() {
        let tmp = TempDir::new().unwrap();
        let files = make_files(tmp.path(), &["a.py", "b.py"], "x = 1\n");
        let result = extract_corpus_parallel(
            &files,
            CorpusParallelConfig {
                chunk_size: 1,
                token_budget: None,
                max_concurrency: 1,
                max_retry_depth: 0,
            },
            |_chunk: &[PathBuf]| Err("simulated API error".to_string()),
            None::<fn(usize, usize, &ChunkResult)>,
        );
        assert!(
            result.failed_chunks > 0,
            "expected failed_chunks > 0, got {}",
            result.failed_chunks
        );
    }

    #[test]
    fn test_failed_chunks_count_matches_multiple_failures() {
        let tmp = TempDir::new().unwrap();
        let files: Vec<PathBuf> = (0..4)
            .map(|i| {
                let f = tmp.path().join(format!("f{i}.py"));
                fs::write(&f, "x").unwrap();
                f
            })
            .collect();
        let call_count = Arc::new(Mutex::new(0usize));
        let cc = Arc::clone(&call_count);
        let result = extract_corpus_parallel(
            &files,
            CorpusParallelConfig {
                chunk_size: 1,
                token_budget: None,
                max_concurrency: 1,
                max_retry_depth: 0,
            },
            move |chunk: &[PathBuf]| {
                let n = {
                    let mut c = cc.lock().unwrap();
                    *c += 1;
                    *c
                };
                if n % 2 == 0 {
                    Err("api error".to_string())
                } else {
                    Ok(stub_result(chunk.len(), "stop"))
                }
            },
            None::<fn(usize, usize, &ChunkResult)>,
        );
        assert_eq!(result.failed_chunks, 2);
        assert_eq!(result.nodes.len(), 2);
    }

    #[test]
    fn test_failed_chunks_multi_worker() {
        let tmp = TempDir::new().unwrap();
        let files: Vec<PathBuf> = (0..6)
            .map(|i| {
                let f = tmp.path().join(format!("f{i}.py"));
                fs::write(&f, "x").unwrap();
                f
            })
            .collect();
        let result = extract_corpus_parallel(
            &files,
            CorpusParallelConfig {
                chunk_size: 1,
                token_budget: None,
                max_concurrency: 3,
                max_retry_depth: 0,
            },
            |_chunk: &[PathBuf]| Err("error".to_string()),
            None::<fn(usize, usize, &ChunkResult)>,
        );
        assert_eq!(result.failed_chunks, 6);
        assert_eq!(result.nodes.len(), 0);
    }

    #[test]
    fn test_chunk_result_merge_accumulates_failed_chunks() {
        let a = ChunkResult {
            failed_chunks: 2,
            ..ChunkResult::empty()
        };
        let b = ChunkResult {
            failed_chunks: 3,
            ..ChunkResult::empty()
        };
        let merged = a.merge(b);
        assert_eq!(merged.failed_chunks, 5);
    }

    #[test]
    fn test_read_chunk_content_utf8_file() {
        let tmp = TempDir::new().unwrap();
        let f = tmp.path().join("ok.py");
        fs::write(&f, "x = 1  # → done\n").unwrap();
        let content = read_chunk_content(&f);
        assert!(content.contains("→ done"));
    }

    #[test]
    fn test_read_chunk_content_non_utf8_lossy() {
        let tmp = TempDir::new().unwrap();
        let f = tmp.path().join("latin1.py");
        // Write bytes that are valid latin-1 but not valid UTF-8 (0xe9 = é in latin-1)
        fs::write(&f, b"x = \xe9\n").unwrap();
        let content = read_chunk_content(&f);
        // Must not panic; must be valid UTF-8 string
        assert!(!content.is_empty());
        content.encode_utf16().for_each(|_| {});
    }

    #[test]
    fn test_unicode_content_survives_file_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let unicode = "→ means implies. ✅ done. Score ≥ 90.";
        let f = tmp.path().join("unicode.md");
        fs::write(&f, unicode.as_bytes()).unwrap();
        let content = read_chunk_content(&f);
        assert!(content.contains("→"));
        assert!(content.contains("✅"));
        assert!(content.contains("≥"));
        // Must encode cleanly to UTF-8
        let _ = content.encode_utf16().collect::<Vec<_>>();
        assert!(!content.is_empty());
    }

    #[test]
    fn test_successful_nodes_present_with_partial_failure() {
        let tmp = TempDir::new().unwrap();
        let files: Vec<PathBuf> = (0..3)
            .map(|i| {
                let f = tmp.path().join(format!("f{i}.py"));
                fs::write(&f, "x").unwrap();
                f
            })
            .collect();
        let call_count = Arc::new(Mutex::new(0usize));
        let cc = Arc::clone(&call_count);
        let result = extract_corpus_parallel(
            &files,
            CorpusParallelConfig {
                chunk_size: 1,
                token_budget: None,
                max_concurrency: 1,
                max_retry_depth: 0,
            },
            move |chunk: &[PathBuf]| {
                let n = {
                    let mut c = cc.lock().unwrap();
                    *c += 1;
                    *c
                };
                if n == 2 {
                    Err("fail".to_string())
                } else {
                    Ok(stub_result(chunk.len(), "stop"))
                }
            },
            None::<fn(usize, usize, &ChunkResult)>,
        );
        assert_eq!(result.failed_chunks, 1);
        assert_eq!(result.nodes.len(), 2);
    }

    #[test]
    fn test_context_exceeded_error_not_counted_as_failed_chunk() {
        let tmp = TempDir::new().unwrap();
        let files: Vec<PathBuf> = (0..2)
            .map(|i| {
                let f = tmp.path().join(format!("f{i}.py"));
                fs::write(&f, "x").unwrap();
                f
            })
            .collect();
        let result = extract_corpus_parallel(
            &files,
            CorpusParallelConfig {
                chunk_size: 2,
                token_budget: None,
                max_concurrency: 1,
                max_retry_depth: 3,
            },
            |chunk: &[PathBuf]| {
                if chunk.len() > 1 {
                    Err("context_length_exceeded: too many tokens".to_string())
                } else {
                    Ok(stub_result(chunk.len(), "stop"))
                }
            },
            None::<fn(usize, usize, &ChunkResult)>,
        );
        // Context-exceeded triggers split+retry, not failed_chunks increment
        assert_eq!(result.failed_chunks, 0);
        assert_eq!(result.nodes.len(), 2);
    }
}
