use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

pub trait NodeEmbedder {
    fn embed_nodes(&self, nodes: &[(&str, &str)]) -> HashMap<String, Vec<f32>>;
}

impl NodeEmbedder for crate::embedding::StaticEmbedder {
    fn embed_nodes(&self, nodes: &[(&str, &str)]) -> HashMap<String, Vec<f32>> {
        crate::embedding::StaticEmbedder::embed_nodes(self, nodes)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EmbedEntry {
    hash: String,
    embeddings: HashMap<String, Vec<f32>>,
}

pub struct EmbedCache {
    path: PathBuf,
    entries: HashMap<String, EmbedEntry>,
}

impl EmbedCache {
    pub fn load(path: PathBuf) -> Self {
        let entries = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Self { path, entries }
    }

    pub fn save(&self) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let json = serde_json::to_string(&self.entries).map_err(|e| e.to_string())?;
        std::fs::write(&self.path, json).map_err(|e| e.to_string())
    }

    pub fn get_if_fresh<'a>(
        &'a self,
        file_path: &str,
        hash: &str,
    ) -> Option<&'a HashMap<String, Vec<f32>>> {
        self.entries
            .get(file_path)
            .filter(|e| e.hash == hash)
            .map(|e| &e.embeddings)
    }

    pub fn insert(
        &mut self,
        file_path: String,
        hash: String,
        embeddings: HashMap<String, Vec<f32>>,
    ) {
        self.entries
            .insert(file_path, EmbedEntry { hash, embeddings });
    }
}

pub fn embed_file_nodes<E: NodeEmbedder>(
    embedder: &E,
    nodes: &[(&str, &str)],
    file_path: &str,
    hash: &str,
    cache: &mut EmbedCache,
) -> HashMap<String, Vec<f32>> {
    if let Some(cached) = cache.get_if_fresh(file_path, hash) {
        return cached.clone();
    }
    let embs = embedder.embed_nodes(nodes);
    cache.insert(file_path.to_string(), hash.to_string(), embs.clone());
    embs
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    struct FakeEmbedder;

    impl NodeEmbedder for FakeEmbedder {
        fn embed_nodes(&self, nodes: &[(&str, &str)]) -> HashMap<String, Vec<f32>> {
            nodes
                .iter()
                .map(|(id, _)| (id.to_string(), vec![1.0f32, 2.0, 3.0]))
                .collect()
        }
    }

    #[test]
    fn test_embed_file_nodes_computes_embeddings() {
        let tmp = tempdir().unwrap();
        let cache_path = tmp.path().join("embed_cache.json");
        let mut cache = EmbedCache::load(cache_path);
        let embedder = FakeEmbedder;
        let nodes = vec![("node1", "MyClass"), ("node2", "doThing")];
        let result = embed_file_nodes(&embedder, &nodes, "src/foo.rs", "abc123", &mut cache);
        assert_eq!(result.len(), 2);
        assert!(result.contains_key("node1"));
        assert_eq!(result["node1"], vec![1.0f32, 2.0, 3.0]);
    }

    #[test]
    fn test_embed_file_nodes_cache_hit() {
        let tmp = tempdir().unwrap();
        let cache_path = tmp.path().join("embed_cache.json");
        let mut cache = EmbedCache::load(cache_path);
        let embedder = FakeEmbedder;
        let nodes = vec![("n1", "Foo")];

        let first = embed_file_nodes(&embedder, &nodes, "src/bar.rs", "hash1", &mut cache);
        // Insert sentinel to detect if embedder called again
        cache.insert(
            "src/bar.rs".to_string(),
            "hash1".to_string(),
            [("n1".to_string(), vec![9.0f32])].into_iter().collect(),
        );
        let second = embed_file_nodes(&embedder, &nodes, "src/bar.rs", "hash1", &mut cache);
        // second should return the sentinel (cache hit), not recompute
        assert_eq!(second["n1"], vec![9.0f32]);
        let _ = first;
    }

    #[test]
    fn test_embed_file_nodes_cache_miss_on_changed_hash() {
        let tmp = tempdir().unwrap();
        let cache_path = tmp.path().join("embed_cache.json");
        let mut cache = EmbedCache::load(cache_path);
        let embedder = FakeEmbedder;
        let nodes = vec![("n1", "Foo")];

        embed_file_nodes(&embedder, &nodes, "src/baz.rs", "hash1", &mut cache);
        // Change the stored entry to a sentinel
        cache.insert(
            "src/baz.rs".to_string(),
            "hash1".to_string(),
            [("n1".to_string(), vec![9.0f32])].into_iter().collect(),
        );
        // Now call with a different hash — should recompute (not return sentinel)
        let result = embed_file_nodes(&embedder, &nodes, "src/baz.rs", "hash2", &mut cache);
        assert_eq!(result["n1"], vec![1.0f32, 2.0, 3.0]);
    }

    #[test]
    fn test_embed_cache_save_and_reload() {
        let tmp = tempdir().unwrap();
        let cache_path = tmp.path().join("embed_cache.json");

        let mut cache = EmbedCache::load(cache_path.clone());
        let embedder = FakeEmbedder;
        let nodes = vec![("x", "Thing")];
        embed_file_nodes(&embedder, &nodes, "src/x.rs", "deadbeef", &mut cache);
        cache.save().unwrap();

        let cache2 = EmbedCache::load(cache_path);
        let hit = cache2.get_if_fresh("src/x.rs", "deadbeef");
        assert!(hit.is_some());
        assert_eq!(hit.unwrap()["x"], vec![1.0f32, 2.0, 3.0]);
    }

    #[test]
    fn test_embed_cache_stale_hash_returns_none() {
        let tmp = tempdir().unwrap();
        let cache_path = tmp.path().join("embed_cache.json");
        let mut cache = EmbedCache::load(cache_path);
        cache.insert("f.rs".to_string(), "old".to_string(), Default::default());
        assert!(cache.get_if_fresh("f.rs", "new").is_none());
        assert!(cache.get_if_fresh("f.rs", "old").is_some());
    }
}
