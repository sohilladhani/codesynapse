use crate::types::ExtractionFragment;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub struct FileCache {
    cache_dir: PathBuf,
}

impl FileCache {
    pub fn new(cache_dir: PathBuf) -> Self {
        Self { cache_dir }
    }

    pub fn from_output_dir(output_dir: &Path) -> Self {
        Self {
            cache_dir: output_dir.join("cache"),
        }
    }

    pub fn cache_path(&self, hash: &str) -> PathBuf {
        self.cache_dir.join(format!("{}.json", hash))
    }

    pub fn compute_hash(content: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(content);
        format!("{:x}", hasher.finalize())
    }

    pub fn get_cached(&self, hash: &str) -> Option<ExtractionFragment> {
        let path = self.cache_path(hash);
        if !path.exists() {
            return None;
        }
        match std::fs::read_to_string(&path) {
            Ok(json) => serde_json::from_str(&json).ok(),
            Err(_) => None,
        }
    }

    pub fn set_cached(
        &self,
        hash: &str,
        fragment: &ExtractionFragment,
    ) -> Result<(), Box<dyn std::error::Error>> {
        std::fs::create_dir_all(&self.cache_dir)?;
        let path = self.cache_path(hash);
        let json = serde_json::to_string(fragment)?;
        std::fs::write(&path, json)?;
        Ok(())
    }

    pub fn is_cached(&self, hash: &str) -> bool {
        self.cache_path(hash).exists()
    }

    pub fn clear(&self) -> Result<(), std::io::Error> {
        if self.cache_dir.exists() {
            std::fs::remove_dir_all(&self.cache_dir)?;
        }
        Ok(())
    }
}

/// Check semantic cache for a list of file paths.
/// Returns (cached_nodes, cached_edges, cached_hyperedges, uncached_files).
/// Cache location: `<root>/codesynapse-out/cache/semantic/<sha256>.json`
pub fn check_semantic_cache(
    files: &[&str],
    root: &Path,
) -> (Vec<Value>, Vec<Value>, Vec<Value>, Vec<String>) {
    let cache_dir = root.join("codesynapse-out").join("cache").join("semantic");
    let mut cached_nodes: Vec<Value> = Vec::new();
    let mut cached_edges: Vec<Value> = Vec::new();
    let mut cached_hyperedges: Vec<Value> = Vec::new();
    let mut uncached: Vec<String> = Vec::new();

    for &fpath in files {
        let p = if std::path::Path::new(fpath).is_absolute() {
            std::path::PathBuf::from(fpath)
        } else {
            root.join(fpath)
        };
        let content = match std::fs::read(&p) {
            Ok(c) => c,
            Err(_) => {
                uncached.push(fpath.to_string());
                continue;
            }
        };
        let hash = FileCache::compute_hash(&content);
        let entry = cache_dir.join(format!("{}.json", hash));
        let hit = entry
            .exists()
            .then(|| std::fs::read_to_string(&entry).ok())
            .flatten()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok());
        match hit {
            Some(result) => {
                if let Some(arr) = result.get("nodes").and_then(|v| v.as_array()) {
                    cached_nodes.extend(arr.iter().cloned());
                }
                if let Some(arr) = result.get("edges").and_then(|v| v.as_array()) {
                    cached_edges.extend(arr.iter().cloned());
                }
                if let Some(arr) = result.get("hyperedges").and_then(|v| v.as_array()) {
                    cached_hyperedges.extend(arr.iter().cloned());
                }
            }
            None => uncached.push(fpath.to_string()),
        }
    }

    (cached_nodes, cached_edges, cached_hyperedges, uncached)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Edge, Node};
    use std::collections::HashMap;

    fn test_fragment() -> ExtractionFragment {
        ExtractionFragment {
            nodes: vec![Node {
                id: "test_node".into(),
                label: "TestNode".into(),
                file_type: "code".into(),
                source_file: "test.py".into(),
                source_location: Some("1:1".into()),
                community: None,
                rationale: None,
                docstring: None,
                metadata: HashMap::new(),
            }],
            edges: vec![Edge {
                source: "a".into(),
                target: "b".into(),
                relation: "calls".into(),
                confidence: "EXTRACTED".into(),
                source_file: Some("test.py".into()),
                weight: 1.0,
                context: None,
            }],
        }
    }

    #[test]
    fn test_compute_hash_deterministic() {
        let h1 = FileCache::compute_hash(b"hello world");
        let h2 = FileCache::compute_hash(b"hello world");
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64);
    }

    #[test]
    fn test_compute_hash_different_inputs() {
        let h1 = FileCache::compute_hash(b"hello");
        let h2 = FileCache::compute_hash(b"world");
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_cache_roundtrip() {
        let dir = std::env::temp_dir().join("codesynapse-test-cache-roundtrip");
        let _ = std::fs::remove_dir_all(&dir);
        let cache = FileCache::from_output_dir(&dir);
        let fragment = test_fragment();
        let hash = FileCache::compute_hash(b"test content");

        cache.set_cached(&hash, &fragment).unwrap();
        assert!(cache.is_cached(&hash));

        let loaded = cache.get_cached(&hash).unwrap();
        assert_eq!(loaded.nodes.len(), 1);
        assert_eq!(loaded.nodes[0].id, "test_node");
        assert_eq!(loaded.edges.len(), 1);
        assert_eq!(loaded.edges[0].relation, "calls");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_cache_miss() {
        let dir = std::env::temp_dir().join("codesynapse-test-cache-miss");
        let _ = std::fs::remove_dir_all(&dir);
        let cache = FileCache::from_output_dir(&dir);
        assert!(!cache.is_cached("nonexistent_hash"));
        assert!(cache.get_cached("nonexistent_hash").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_cache_clear() {
        let dir = std::env::temp_dir().join("codesynapse-test-cache-clear");
        let _ = std::fs::remove_dir_all(&dir);
        let cache = FileCache::from_output_dir(&dir);
        let fragment = test_fragment();
        let hash = FileCache::compute_hash(b"clear test");

        cache.set_cached(&hash, &fragment).unwrap();
        assert!(cache.is_cached(&hash));

        cache.clear().unwrap();
        assert!(!cache.is_cached(&hash));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
