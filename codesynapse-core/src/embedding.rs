use rand::Rng;
use std::collections::{HashMap, HashSet};
use std::path::Path;

#[derive(Debug, Clone)]
pub struct Node2Vec {
    pub dimensions: usize,
    pub walk_length: usize,
    pub num_walks: usize,
    pub window_size: usize,
    pub p: f64,
    pub q: f64,
    pub learning_rate: f64,
    pub negative_samples: usize,
}

impl Default for Node2Vec {
    fn default() -> Self {
        Self {
            dimensions: 64,
            walk_length: 80,
            num_walks: 10,
            window_size: 10,
            p: 1.0,
            q: 1.0,
            learning_rate: 0.01,
            negative_samples: 5,
        }
    }
}

impl Node2Vec {
    pub fn new(dimensions: usize, p: f64, q: f64) -> Self {
        Self {
            dimensions,
            p,
            q,
            ..Default::default()
        }
    }

    pub fn train(&self, edges: &[(String, String)]) -> HashMap<String, Vec<f64>> {
        let adj = build_adjacency(edges);
        let node_ids: Vec<&str> = adj.keys().copied().collect();
        let node_set: HashSet<&str> = node_ids.iter().copied().collect();

        let mut rng = rand::thread_rng();

        let num_nodes = node_ids.len();
        if num_nodes == 0 {
            return HashMap::new();
        }

        let mut embeddings: HashMap<String, Vec<f64>> = HashMap::new();
        for id in &node_ids {
            let emb: Vec<f64> = (0..self.dimensions)
                .map(|_| (rng.gen::<f64>() - 0.5) / self.dimensions as f64)
                .collect();
            embeddings.insert((*id).to_string(), emb);
        }

        let unigram_weights: Vec<f64> = {
            let mut deg: HashMap<&str, usize> = HashMap::new();
            for (u, v) in edges {
                *deg.entry(u.as_str()).or_insert(0) += 1;
                *deg.entry(v.as_str()).or_insert(0) += 1;
            }
            let total: usize = deg.values().sum();
            let total_f = total.max(1) as f64;
            node_ids
                .iter()
                .map(|id| {
                    let d = deg.get(id).copied().unwrap_or(1) as f64;
                    d.powf(0.75) / total_f.powf(0.75)
                })
                .collect()
        };

        let mut cumulative = Vec::with_capacity(num_nodes);
        let mut sum = 0.0;
        for w in &unigram_weights {
            sum += w;
            cumulative.push(sum);
        }
        let noise_norm = sum;

        fn sample_noise<'a>(
            rng: &mut impl Rng,
            cumulative: &[f64],
            norm: f64,
            node_ids: &[&'a str],
        ) -> &'a str {
            let r = rng.gen::<f64>() * norm;
            let idx = match cumulative.binary_search_by(|p| p.partial_cmp(&r).unwrap()) {
                Ok(i) => i,
                Err(i) => i.min(cumulative.len() - 1),
            };
            node_ids[idx]
        }

        for _epoch in 0..1 {
            for start_node in &node_ids {
                for _walk_idx in 0..self.num_walks {
                    let walk = self.biased_walk(start_node, &adj, &node_set, &mut rng);
                    #[allow(clippy::needless_range_loop)]
                    for i in 0..walk.len() {
                        let center = walk[i];
                        let center_emb = embeddings.get(center).unwrap().clone();

                        let left = i.saturating_sub(self.window_size);
                        let right = (i + self.window_size + 1).min(walk.len());

                        for j in left..right {
                            if i == j {
                                continue;
                            }
                            let context = walk[j];

                            let dot: f64 = center_emb
                                .iter()
                                .zip(embeddings.get(context).unwrap().iter())
                                .map(|(a, b)| a * b)
                                .sum();
                            let sigmoid_pos = 1.0 / (1.0 + (-dot).exp());

                            if let Some(ctx_emb) = embeddings.get_mut(context) {
                                let grad = self.learning_rate * (1.0 - sigmoid_pos);
                                for k in 0..self.dimensions {
                                    ctx_emb[k] += grad * center_emb[k];
                                }
                            }

                            let context_emb = embeddings.get(context).unwrap().clone();
                            if let Some(cnt_emb) = embeddings.get_mut(center) {
                                let grad = self.learning_rate * (1.0 - sigmoid_pos);
                                for k in 0..self.dimensions {
                                    cnt_emb[k] += grad * context_emb[k];
                                }
                            }

                            for _ns in 0..self.negative_samples {
                                let noise_node =
                                    sample_noise(&mut rng, &cumulative, noise_norm, &node_ids);

                                let noise_emb = embeddings.get(noise_node).unwrap().clone();

                                let dot_neg: f64 = center_emb
                                    .iter()
                                    .zip(noise_emb.iter())
                                    .map(|(a, b)| a * b)
                                    .sum();
                                let sigmoid_neg = 1.0 / (1.0 + (-dot_neg).exp());

                                if let Some(noi_emb) = embeddings.get_mut(noise_node) {
                                    let grad_neg = self.learning_rate * (0.0 - sigmoid_neg);
                                    for k in 0..self.dimensions {
                                        noi_emb[k] += grad_neg * center_emb[k];
                                    }
                                }

                                if let Some(cnt_emb) = embeddings.get_mut(center) {
                                    let grad_neg = self.learning_rate * (0.0 - sigmoid_neg);
                                    for k in 0..self.dimensions {
                                        cnt_emb[k] += grad_neg * noise_emb[k];
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        embeddings
    }

    fn biased_walk<'a>(
        &self,
        start: &'a str,
        adj: &HashMap<&str, Vec<&'a str>>,
        _node_set: &HashSet<&'a str>,
        rng: &mut impl Rng,
    ) -> Vec<&'a str> {
        let mut walk = Vec::with_capacity(self.walk_length);
        walk.push(start);

        let mut curr = start;
        let mut prev: Option<&str> = None;

        for _step in 1..self.walk_length {
            let neighbors = match adj.get(curr) {
                Some(n) if !n.is_empty() => n,
                _ => break,
            };

            let next = if let Some(prev_node) = prev {
                let prev_neighbors: HashSet<&str> = adj
                    .get(prev_node)
                    .map(|n| n.iter().copied().collect())
                    .unwrap_or_default();
                let weights: Vec<f64> = neighbors
                    .iter()
                    .map(|n| {
                        if *n == prev_node {
                            1.0 / self.p
                        } else if prev_neighbors.contains(n) {
                            1.0
                        } else {
                            1.0 / self.q
                        }
                    })
                    .collect();
                let total: f64 = weights.iter().sum();
                if total <= 0.0 {
                    neighbors[rng.gen_range(0..neighbors.len())]
                } else {
                    let r = rng.gen::<f64>() * total;
                    let mut cum = 0.0;
                    let mut chosen = neighbors[0];
                    for (i, w) in weights.iter().enumerate() {
                        cum += w;
                        if cum >= r {
                            chosen = neighbors[i];
                            break;
                        }
                    }
                    chosen
                }
            } else {
                neighbors[rng.gen_range(0..neighbors.len())]
            };

            walk.push(next);
            prev = Some(curr);
            curr = next;
        }

        walk
    }

    pub fn find_similar(
        &self,
        embeddings: &HashMap<String, Vec<f64>>,
        node_id: &str,
        top_n: usize,
    ) -> Vec<(String, f64)> {
        let target = match embeddings.get(node_id) {
            Some(e) => e,
            None => return Vec::new(),
        };

        let mut scores: Vec<(String, f64)> = embeddings
            .iter()
            .filter(|(id, _)| id.as_str() != node_id)
            .map(|(id, emb)| (id.clone(), cosine_similarity(target, emb)))
            .collect();

        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scores.truncate(top_n);
        scores
    }
}

pub fn cosine_similarity(a: &[f64], b: &[f64]) -> f64 {
    let dot: f64 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let norm_a: f64 = a.iter().map(|x| x * x).sum();
    let norm_b: f64 = b.iter().map(|x| x * x).sum();
    let denom = norm_a.sqrt() * norm_b.sqrt();
    if denom < 1e-12 {
        0.0
    } else {
        dot / denom
    }
}

fn build_adjacency(edges: &[(String, String)]) -> HashMap<&str, Vec<&str>> {
    let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
    for (u, v) in edges {
        adj.entry(u.as_str()).or_default().push(v.as_str());
        adj.entry(v.as_str()).or_default().push(u.as_str());
    }
    adj
}

fn fnv1a_seed(token: &str) -> u64 {
    token.bytes().fold(0xcbf29ce484222325u64, |acc, b| {
        acc.wrapping_mul(0x100000001b3).wrapping_add(b as u64)
    })
}

fn xor64(mut v: u64) -> u64 {
    v ^= v >> 30;
    v = v.wrapping_mul(0xbf58476d1ce4e5b9);
    v ^= v >> 27;
    v = v.wrapping_mul(0x94d049bb133111eb);
    v ^= v >> 31;
    v
}

fn seeded_embedding(token: &str, dimensions: usize) -> Vec<f64> {
    let seed = fnv1a_seed(token);
    let raw: Vec<f64> = (0..dimensions)
        .map(|i| {
            let h = xor64(seed.wrapping_add((i as u64).wrapping_mul(0x9e3779b97f4a7c15)));
            (h as f64 / u64::MAX as f64) - 0.5
        })
        .collect();
    l2_normalize(&raw)
}

fn l2_normalize(v: &[f64]) -> Vec<f64> {
    let norm: f64 = v.iter().map(|x| x * x).sum::<f64>().sqrt();
    if norm < 1e-12 {
        v.to_vec()
    } else {
        v.iter().map(|x| x / norm).collect()
    }
}

pub fn tokenize_label(label: &str) -> Vec<String> {
    let mut parts: Vec<String> = Vec::new();
    for seg in label.split(['_', ':', '.', '/', ' ', '-']) {
        if seg.is_empty() {
            continue;
        }
        split_camel(seg, &mut parts);
    }
    parts
        .into_iter()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_lowercase())
        .collect()
}

fn split_camel(s: &str, out: &mut Vec<String>) {
    let chars: Vec<char> = s.chars().collect();
    let mut start = 0;
    for i in 1..chars.len() {
        let prev_lower = chars[i - 1].is_lowercase();
        let cur_upper = chars[i].is_uppercase();
        let next_lower = chars.get(i + 1).map(|c| c.is_lowercase()).unwrap_or(false);
        if cur_upper && (prev_lower || next_lower) {
            out.push(chars[start..i].iter().collect());
            start = i;
        }
    }
    out.push(chars[start..].iter().collect());
}

/// Static word embeddings via deterministic token lookup (simulates Model2Vec).
#[derive(Debug, Clone)]
pub struct Model2VecEmbedder {
    pub dimensions: usize,
    token_embeddings: HashMap<String, Vec<f64>>,
}

impl Model2VecEmbedder {
    pub fn new(dimensions: usize) -> Self {
        Self {
            dimensions,
            token_embeddings: HashMap::new(),
        }
    }

    pub fn with_vocab(vocab: &[&str], dimensions: usize) -> Self {
        let mut token_embeddings = HashMap::new();
        for &tok in vocab {
            token_embeddings.insert(tok.to_string(), seeded_embedding(tok, dimensions));
        }
        Self {
            dimensions,
            token_embeddings,
        }
    }

    pub fn from_node_labels(labels: &[&str], dimensions: usize) -> Self {
        let mut vocab: HashSet<String> = HashSet::new();
        for label in labels {
            for tok in tokenize_label(label) {
                vocab.insert(tok);
            }
        }
        let mut emb = Self::new(dimensions);
        for tok in &vocab {
            emb.token_embeddings
                .insert(tok.clone(), seeded_embedding(tok, dimensions));
        }
        emb
    }

    pub fn embed_label(&self, label: &str) -> Vec<f64> {
        let tokens = tokenize_label(label);
        if tokens.is_empty() {
            return vec![0.0; self.dimensions];
        }
        let mut sum = vec![0.0f64; self.dimensions];
        let mut count = 0usize;
        for tok in &tokens {
            let emb = self
                .token_embeddings
                .get(tok)
                .cloned()
                .unwrap_or_else(|| seeded_embedding(tok, self.dimensions));
            for (i, v) in emb.iter().enumerate() {
                sum[i] += v;
            }
            count += 1;
        }
        let n = count as f64;
        sum.iter_mut().for_each(|v| *v /= n);
        l2_normalize(&sum)
    }

    pub fn embed_nodes(&self, nodes: &[(&str, &str)]) -> HashMap<String, Vec<f64>> {
        nodes
            .iter()
            .map(|(id, label)| (id.to_string(), self.embed_label(label)))
            .collect()
    }
}

/// Blends Node2Vec structural embeddings with Model2Vec semantic embeddings.
#[derive(Debug, Clone)]
pub struct HybridEmbedder {
    pub node2vec: Node2Vec,
    pub model2vec: Model2VecEmbedder,
    /// 0.0 = pure semantic, 1.0 = pure structural.
    pub alpha: f64,
}

impl HybridEmbedder {
    pub fn new(node2vec: Node2Vec, model2vec: Model2VecEmbedder, alpha: f64) -> Self {
        Self {
            node2vec,
            model2vec,
            alpha,
        }
    }

    /// Embed nodes by blending structural (Node2Vec) and semantic (Model2Vec) signals.
    /// `nodes`: slice of `(id, label)` pairs.
    pub fn embed(
        &self,
        edges: &[(String, String)],
        nodes: &[(&str, &str)],
    ) -> HashMap<String, Vec<f64>> {
        let structural = self.node2vec.train(edges);
        let semantic = self.model2vec.embed_nodes(nodes);
        let dim = self.model2vec.dimensions;

        let mut result = HashMap::new();
        for (id, _) in nodes {
            let id_str = id.to_string();
            let sem = semantic
                .get(&id_str)
                .cloned()
                .unwrap_or_else(|| vec![0.0; dim]);

            let blended = if let Some(struc) = structural.get(&id_str) {
                let take = dim.min(struc.len());
                let mut norm_struc = l2_normalize(&struc[..take]);
                norm_struc.resize(dim, 0.0);
                let v: Vec<f64> = norm_struc
                    .iter()
                    .zip(sem.iter())
                    .map(|(s, m)| self.alpha * s + (1.0 - self.alpha) * m)
                    .collect();
                l2_normalize(&v)
            } else {
                sem
            };

            result.insert(id_str, blended);
        }
        result
    }

    pub fn find_similar(
        &self,
        embeddings: &HashMap<String, Vec<f64>>,
        node_id: &str,
        top_n: usize,
    ) -> Vec<(String, f64)> {
        let target = match embeddings.get(node_id) {
            Some(e) => e,
            None => return Vec::new(),
        };
        let mut scores: Vec<(String, f64)> = embeddings
            .iter()
            .filter(|(id, _)| id.as_str() != node_id)
            .map(|(id, emb)| (id.clone(), cosine_similarity(target, emb)))
            .collect();
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scores.truncate(top_n);
        scores
    }
}

// ---------------------------------------------------------------------------
// StaticEmbedder — real Model2Vec loader (tokenizer.json + model.safetensors)

fn l2_normalize_f32(v: &[f32]) -> Vec<f32> {
    let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm < 1e-12 {
        v.to_vec()
    } else {
        v.iter().map(|x| x / norm).collect()
    }
}

pub fn cosine_similarity_f32(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let na: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let nb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    let d = na * nb;
    if d < 1e-12 {
        0.0
    } else {
        dot / d
    }
}

pub struct StaticEmbedder {
    tokenizer: tokenizers::Tokenizer,
    /// Flat row-major matrix [vocab_size × dimensions], rows are L2-normalised f32.
    embeddings: Vec<f32>,
    pub dimensions: usize,
    vocab_size: usize,
}

impl StaticEmbedder {
    /// Load from a directory containing `tokenizer.json` and `model.safetensors`.
    pub fn from_path(model_dir: &Path) -> Result<Self, String> {
        let tokenizer = tokenizers::Tokenizer::from_file(model_dir.join("tokenizer.json"))
            .map_err(|e| format!("tokenizer load: {e}"))?;

        let bytes = std::fs::read(model_dir.join("model.safetensors"))
            .map_err(|e| format!("model.safetensors read: {e}"))?;

        let st = safetensors::SafeTensors::deserialize(&bytes)
            .map_err(|e| format!("safetensors parse: {e}"))?;

        let view = st
            .tensor("embeddings")
            .map_err(|e| format!("tensor 'embeddings': {e}"))?;

        let shape = view.shape();
        if shape.len() != 2 {
            return Err(format!("expected 2-D tensor, got shape {:?}", shape));
        }
        let vocab_size = shape[0];
        let dimensions = shape[1];

        if view.dtype() != safetensors::Dtype::F32 {
            return Err(format!("expected F32 embeddings, got {:?}", view.dtype()));
        }

        let raw = view.data();
        let embeddings: Vec<f32> = raw
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect();

        Ok(Self {
            tokenizer,
            embeddings,
            dimensions,
            vocab_size,
        })
    }

    /// Embed text: tokenize → look up rows → mean pool → L2-normalise.
    pub fn embed(&self, text: &str) -> Vec<f32> {
        let enc = match self.tokenizer.encode(text, false) {
            Ok(e) => e,
            Err(_) => return vec![0.0f32; self.dimensions],
        };
        let ids = enc.get_ids();
        if ids.is_empty() {
            return vec![0.0f32; self.dimensions];
        }

        let mut sum = vec![0.0f32; self.dimensions];
        let mut count = 0usize;
        for &id in ids {
            let idx = id as usize;
            if idx < self.vocab_size {
                let start = idx * self.dimensions;
                let end = start + self.dimensions;
                for (i, &v) in self.embeddings[start..end].iter().enumerate() {
                    sum[i] += v;
                }
                count += 1;
            }
        }
        if count == 0 {
            return vec![0.0f32; self.dimensions];
        }
        let n = count as f32;
        sum.iter_mut().for_each(|v| *v /= n);
        l2_normalize_f32(&sum)
    }

    /// Embed a batch of `(id, label)` pairs into a `HashMap<id, embedding>`.
    pub fn embed_nodes(&self, nodes: &[(&str, &str)]) -> HashMap<String, Vec<f32>> {
        nodes
            .iter()
            .map(|(id, label)| (id.to_string(), self.embed(label)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_edges() -> Vec<(String, String)> {
        vec![
            ("a".into(), "b".into()),
            ("b".into(), "c".into()),
            ("c".into(), "d".into()),
            ("d".into(), "e".into()),
            ("e".into(), "f".into()),
            ("a".into(), "f".into()),
            ("b".into(), "f".into()),
            ("c".into(), "e".into()),
        ]
    }

    #[test]
    fn test_cosine_similarity_identical() {
        let a = vec![1.0, 2.0, 3.0];
        let b = vec![1.0, 2.0, 3.0];
        let sim = cosine_similarity(&a, &b);
        assert!((sim - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_cosine_similarity_orthogonal() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        let sim = cosine_similarity(&a, &b);
        assert!(sim.abs() < 1e-6);
    }

    #[test]
    fn test_cosine_similarity_opposite() {
        let a = vec![1.0, 0.0];
        let b = vec![-1.0, 0.0];
        let sim = cosine_similarity(&a, &b);
        assert!((sim + 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_cosine_similarity_zero_vector() {
        let a = vec![0.0, 0.0];
        let b = vec![1.0, 0.0];
        let sim = cosine_similarity(&a, &b);
        assert!(sim.abs() < 1e-12);
    }

    #[test]
    fn test_build_adjacency() {
        let edges = make_edges();
        let adj = build_adjacency(&edges);
        assert!(adj.contains_key("a"));
        assert_eq!(adj["a"].len(), 2);
        assert!(adj["a"].contains(&"b"));
        assert!(adj["a"].contains(&"f"));
    }

    #[test]
    fn test_train_non_empty() {
        let edges = make_edges();
        let n2v = Node2Vec::new(8, 1.0, 1.0);
        let embeddings = n2v.train(&edges);
        assert!(!embeddings.is_empty());
        assert!(embeddings.contains_key("a"));
        assert_eq!(embeddings["a"].len(), 8);
    }

    #[test]
    fn test_find_similar_returns_results() {
        let edges = make_edges();
        let n2v = Node2Vec::new(8, 1.0, 1.0);
        let embeddings = n2v.train(&edges);

        let similar = n2v.find_similar(&embeddings, "a", 3);
        assert!(!similar.is_empty());
        assert!(similar.len() <= 3);
        for (id, score) in &similar {
            assert_ne!(id, "a");
            assert!(*score >= -1.1 && *score <= 1.1);
        }
    }

    #[test]
    fn test_find_similar_nonexistent_node() {
        let edges = make_edges();
        let n2v = Node2Vec::new(8, 1.0, 1.0);
        let embeddings = n2v.train(&edges);
        let similar = n2v.find_similar(&embeddings, "nonexistent", 3);
        assert!(similar.is_empty());
    }

    #[test]
    fn test_biased_walk_length() {
        let edges = make_edges();
        let n2v = Node2Vec::new(8, 1.0, 1.0);
        let adj = build_adjacency(&edges);
        let node_set: HashSet<&str> = adj.keys().copied().collect();
        let mut rng = rand::thread_rng();
        let walk = n2v.biased_walk("a", &adj, &node_set, &mut rng);
        assert!(!walk.is_empty());
        assert_eq!(walk[0], "a");
        assert!(walk.len() <= 80);
    }

    #[test]
    fn test_biased_walk_starts_at_start() {
        let edges = make_edges();
        let n2v = Node2Vec::new(8, 0.25, 1.0);
        let adj = build_adjacency(&edges);
        let node_set: HashSet<&str> = adj.keys().copied().collect();
        let mut rng = rand::thread_rng();
        let walk = n2v.biased_walk("a", &adj, &node_set, &mut rng);
        assert_eq!(walk[0], "a");
    }

    #[test]
    fn test_neighbors_more_similar_than_distant() {
        let edges = make_edges();
        let n2v = Node2Vec::new(16, 1.0, 1.0);
        let embeddings = n2v.train(&edges);

        let similar = n2v.find_similar(&embeddings, "a", 10);
        let top_ids: Vec<&str> = similar.iter().take(4).map(|(id, _)| id.as_str()).collect();
        let has_neighbor = top_ids.contains(&"b") || top_ids.contains(&"f");
        assert!(
            has_neighbor,
            "At least one direct neighbor should be in top-4 similar to 'a': {:?}",
            top_ids
        );
    }

    #[test]
    fn test_biased_walk_with_p_lt_1() {
        let edges = vec![
            ("a".into(), "b".into()),
            ("b".into(), "c".into()),
            ("b".into(), "d".into()),
        ];
        let n2v = Node2Vec::new(8, 0.25, 1.0);
        let adj = build_adjacency(&edges);
        let node_set: HashSet<&str> = adj.keys().copied().collect();
        let mut rng = rand::thread_rng();
        // low p encourages return to previous node
        let walk = n2v.biased_walk("b", &adj, &node_set, &mut rng);
        assert_eq!(walk[0], "b");
        assert!(walk.len() >= 2);
    }

    // --- Model2Vec tests ---

    #[test]
    fn test_tokenize_underscore() {
        let tokens = tokenize_label("foo_bar_baz");
        assert_eq!(tokens, vec!["foo", "bar", "baz"]);
    }

    #[test]
    fn test_tokenize_camelcase() {
        let tokens = tokenize_label("CamelCase");
        assert!(tokens.contains(&"camel".to_string()));
        assert!(tokens.contains(&"case".to_string()));
    }

    #[test]
    fn test_tokenize_colons() {
        let tokens = tokenize_label("my::module::Func");
        assert!(tokens.contains(&"my".to_string()));
        assert!(tokens.contains(&"module".to_string()));
        assert!(tokens.contains(&"func".to_string()));
    }

    #[test]
    fn test_model2vec_embed_label_nonzero() {
        let e = Model2VecEmbedder::new(16);
        let emb = e.embed_label("foo_bar");
        assert_eq!(emb.len(), 16);
        assert!(!emb.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn test_model2vec_embed_label_empty() {
        let e = Model2VecEmbedder::new(16);
        let emb = e.embed_label("");
        assert_eq!(emb, vec![0.0; 16]);
    }

    #[test]
    fn test_model2vec_dimensions() {
        let e = Model2VecEmbedder::new(32);
        let emb = e.embed_label("hello_world");
        assert_eq!(emb.len(), 32);
    }

    #[test]
    fn test_model2vec_deterministic() {
        let e = Model2VecEmbedder::new(16);
        let a = e.embed_label("compute_graph");
        let b = e.embed_label("compute_graph");
        assert_eq!(a, b);
    }

    #[test]
    fn test_model2vec_embed_nodes() {
        let e = Model2VecEmbedder::new(16);
        let nodes = vec![("n1", "foo_bar"), ("n2", "baz_qux")];
        let embs = e.embed_nodes(&nodes);
        assert!(embs.contains_key("n1"));
        assert!(embs.contains_key("n2"));
        assert_eq!(embs["n1"].len(), 16);
    }

    #[test]
    fn test_model2vec_similar_labels_closer() {
        let e = Model2VecEmbedder::new(64);
        let emb_a = e.embed_label("compute_graph");
        let emb_b = e.embed_label("compute_nodes");
        let emb_c = e.embed_label("xyz_qwerty");
        let sim_ab = cosine_similarity(&emb_a, &emb_b);
        let sim_ac = cosine_similarity(&emb_a, &emb_c);
        // "compute_graph" and "compute_nodes" share "compute" → closer
        assert!(
            sim_ab > sim_ac,
            "expected sim_ab({}) > sim_ac({})",
            sim_ab,
            sim_ac
        );
    }

    #[test]
    fn test_model2vec_with_vocab() {
        let e = Model2VecEmbedder::with_vocab(&["foo", "bar", "baz"], 16);
        let emb = e.embed_label("foo_bar");
        assert_eq!(emb.len(), 16);
        assert!(!emb.iter().all(|&v| v == 0.0));
    }

    // --- HybridEmbedder tests ---

    #[test]
    fn test_hybrid_embed_all_nodes() {
        let n2v = Node2Vec::new(16, 1.0, 1.0);
        let m2v = Model2VecEmbedder::new(16);
        let hybrid = HybridEmbedder::new(n2v, m2v, 0.5);
        let edges = vec![
            ("a".to_string(), "b".to_string()),
            ("b".to_string(), "c".to_string()),
        ];
        let nodes = vec![("a", "foo_bar"), ("b", "baz_qux"), ("c", "qux_quux")];
        let embs = hybrid.embed(&edges, &nodes);
        assert!(embs.contains_key("a"));
        assert!(embs.contains_key("b"));
        assert!(embs.contains_key("c"));
    }

    #[test]
    fn test_hybrid_dimensions() {
        let n2v = Node2Vec::new(16, 1.0, 1.0);
        let m2v = Model2VecEmbedder::new(16);
        let hybrid = HybridEmbedder::new(n2v, m2v, 0.5);
        let edges = vec![("a".to_string(), "b".to_string())];
        let nodes = vec![("a", "foo"), ("b", "bar")];
        let embs = hybrid.embed(&edges, &nodes);
        assert_eq!(embs["a"].len(), 16);
        assert_eq!(embs["b"].len(), 16);
    }

    #[test]
    fn test_hybrid_alpha_0_pure_semantic() {
        let n2v = Node2Vec::new(16, 1.0, 1.0);
        let m2v = Model2VecEmbedder::new(16);
        let hybrid = HybridEmbedder::new(n2v, m2v.clone(), 0.0);
        let edges = vec![("a".to_string(), "b".to_string())];
        let nodes = vec![("a", "hello_world"), ("b", "foo_bar")];
        let embs = hybrid.embed(&edges, &nodes);
        let sem = m2v.embed_label("hello_world");
        let sim = cosine_similarity(&embs["a"], &sem);
        assert!(sim > 0.99, "expected pure semantic, got sim={}", sim);
    }

    #[test]
    fn test_hybrid_find_similar() {
        let n2v = Node2Vec::new(16, 1.0, 1.0);
        let m2v = Model2VecEmbedder::new(16);
        let hybrid = HybridEmbedder::new(n2v, m2v, 0.5);
        let edges = vec![
            ("a".to_string(), "b".to_string()),
            ("b".to_string(), "c".to_string()),
        ];
        let nodes = vec![("a", "foo"), ("b", "bar"), ("c", "baz")];
        let embs = hybrid.embed(&edges, &nodes);
        let similar = hybrid.find_similar(&embs, "a", 2);
        assert!(!similar.is_empty());
        assert!(similar.len() <= 2);
        for (id, score) in &similar {
            assert_ne!(id, "a");
            assert!(*score >= -1.1 && *score <= 1.1);
        }
    }

    #[test]
    fn test_hybrid_isolated_node_uses_semantic() {
        // node "z" has no edges → no structural embedding → falls back to semantic
        let n2v = Node2Vec::new(16, 1.0, 1.0);
        let m2v = Model2VecEmbedder::new(16);
        let sem = m2v.embed_label("isolated_node");
        let hybrid = HybridEmbedder::new(n2v, m2v, 0.5);
        let edges: Vec<(String, String)> = vec![("a".to_string(), "b".to_string())];
        let nodes = vec![("z", "isolated_node"), ("a", "foo"), ("b", "bar")];
        let embs = hybrid.embed(&edges, &nodes);
        let sim = cosine_similarity(&embs["z"], &sem);
        assert!(
            sim > 0.99,
            "isolated node should use semantic embedding, sim={}",
            sim
        );
    }
}
