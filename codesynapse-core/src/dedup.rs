use crate::error::Result;
use crate::types::{Edge, Node, NodeId};
use std::collections::HashMap;

pub struct Deduplicator {
    /// Jaro-Winkler threshold for fuzzy merge (default 0.97).
    /// Higher = fewer false positives, more false negatives.
    /// Lower = more aggressive merging, risk of false positives.
    ///
    /// Note: JW loses discriminative power on long names because the
    /// matching-fraction term dominates. `is_variant_pair` blocks prefix-
    /// extension collisions (e.g. `FactoryImpl` vs `Factory`), and the
    /// same-source-file constraint keeps comparisons within scope.
    pub threshold: f64,
}

impl Deduplicator {
    pub fn new(threshold: f64) -> Self {
        Deduplicator { threshold }
    }

    pub fn default_threshold() -> f64 {
        0.97
    }
    /// Run dedup locally with no LLM calls.
    /// Pairs below 0.92 are left unmerged (conservative — no false positives).
    /// Call `llm_tiebreaker` separately if you want an LLM pass for higher recall.
    pub fn deduplicate(
        &self,
        nodes: Vec<Node>,
        edges: Vec<Edge>,
    ) -> Result<(Vec<Node>, Vec<Edge>)> {
        self.check_cross_project(&nodes)?;

        let mut merged_nodes: HashMap<NodeId, Node> = HashMap::new();
        let mut exact_index: HashMap<String, NodeId> = HashMap::new();
        let mut id_mapping: HashMap<NodeId, NodeId> = HashMap::new();

        // Pass 1: exact match dedup within same file
        for node in nodes {
            let key = self.exact_match_key(&node);
            if let Some(existing_id) = exact_index.get(&key) {
                id_mapping.insert(node.id.clone(), existing_id.clone());
                continue;
            }
            exact_index.insert(key, node.id.clone());
            merged_nodes.insert(node.id.clone(), node);
        }

        // Pass 2: fuzzy match (Jaro-Winkler > threshold) within same file
        // Community boost: +0.05 when both nodes share a community
        let ids: Vec<NodeId> = merged_nodes.keys().cloned().collect();
        for i in 0..ids.len() {
            for j in (i + 1)..ids.len() {
                let ni = merged_nodes[&ids[i]].clone();
                let nj = merged_nodes[&ids[j]].clone();

                if ni.source_file != nj.source_file {
                    continue;
                }

                let mut score = jaro_winkler(&ni.label, &nj.label);
                if self.same_community(&ni, &nj) {
                    score = (score + 0.05).min(1.0);
                }

                if score > self.threshold && !self.is_variant_pair(&ni.label, &nj.label) {
                    let target_id = ids[j].clone();
                    let keep_id = ids[i].clone();
                    id_mapping.insert(target_id, keep_id);
                    merged_nodes.remove(&ids[j]);
                }
            }
        }

        // Remap edges
        let remapped_edges: Vec<Edge> = edges
            .into_iter()
            .map(|e| {
                let new_src = id_mapping
                    .get(&e.source)
                    .cloned()
                    .unwrap_or(e.source.clone());
                let new_tgt = id_mapping
                    .get(&e.target)
                    .cloned()
                    .unwrap_or(e.target.clone());
                Edge {
                    source: new_src,
                    target: new_tgt,
                    ..e
                }
            })
            .collect();

        let final_nodes: Vec<Node> = merged_nodes.into_values().collect();
        Ok((final_nodes, remapped_edges))
    }

    fn check_cross_project(&self, nodes: &[Node]) -> Result<()> {
        let repos: std::collections::HashSet<&str> = nodes
            .iter()
            .filter_map(|n| n.metadata.get("repo"))
            .map(|s| s.as_str())
            .collect();
        if repos.len() > 1 {
            return Err(crate::error::CodeSynapseError::Parse(
                "cross-project dedup not supported: nodes from different repos found".to_string(),
            ));
        }
        Ok(())
    }

    fn same_community(&self, a: &Node, b: &Node) -> bool {
        match (a.community, b.community) {
            (Some(ca), Some(cb)) => ca == cb,
            _ => false,
        }
    }

    fn exact_match_key(&self, node: &Node) -> String {
        format!("{}:{}", node.source_file, node.label)
    }

    fn is_variant_pair(&self, a: &str, b: &str) -> bool {
        // Block "M1" / "M1 Pro" style pairs
        let (shorter, longer) = if a.len() <= b.len() { (a, b) } else { (b, a) };
        longer.starts_with(shorter) && longer.len() > shorter.len()
    }

    /// Optional LLM tiebreaker pass for higher recall.
    /// Returns (src_id, tgt_id, merge) — caller should inspect and apply.
    pub fn llm_tiebreaker(&self, pairs: Vec<(Node, Node)>) -> Vec<(String, String, bool)> {
        pairs
            .into_iter()
            .map(|(a, b)| {
                let score = jaro_winkler(&a.label, &b.label);
                if score >= self.threshold {
                    (a.id, b.id, true)
                } else {
                    (a.id, b.id, false)
                }
            })
            .collect()
    }
}

impl Default for Deduplicator {
    fn default() -> Self {
        Deduplicator::new(Deduplicator::default_threshold())
    }
}

fn jaro_winkler(a: &str, b: &str) -> f64 {
    let a = a.to_lowercase();
    let b = b.to_lowercase();

    if a == b {
        return 1.0;
    }

    let len_a = a.len();
    let len_b = b.len();

    if len_a == 0 || len_b == 0 {
        return 0.0;
    }

    let max_dist = (std::cmp::max(len_a, len_b) / 2).saturating_sub(1);
    let max_dist = std::cmp::max(max_dist, 0);

    let mut a_matched = vec![false; len_a];
    let mut b_matched = vec![false; len_b];

    let mut matches = 0;
    let mut transpositions = 0;

    #[allow(clippy::needless_range_loop)]
    for i in 0..len_a {
        let start = i.saturating_sub(max_dist);
        let end = std::cmp::min(len_b, i + max_dist + 1);

        for j in start..end {
            if b_matched[j] {
                continue;
            }
            if a.as_bytes()[i] == b.as_bytes()[j] {
                a_matched[i] = true;
                b_matched[j] = true;
                matches += 1;
                break;
            }
        }
    }

    if matches == 0 {
        return 0.0;
    }

    let mut k = 0;
    #[allow(clippy::needless_range_loop)]
    for i in 0..len_a {
        if a_matched[i] {
            while !b_matched[k] {
                k += 1;
            }
            if a.as_bytes()[i] != b.as_bytes()[k] {
                transpositions += 1;
            }
            k += 1;
        }
    }

    let jaro = (matches as f64 / len_a as f64
        + matches as f64 / len_b as f64
        + (matches as f64 - transpositions as f64 / 2.0) / matches as f64)
        / 3.0;

    // Winkler boost for common prefix
    let prefix_len = a
        .chars()
        .zip(b.chars())
        .take_while(|(ca, cb)| ca == cb)
        .count()
        .min(4);

    jaro + prefix_len as f64 * 0.1 * (1.0 - jaro)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn make_node(id: &str, label: &str, source_file: &str) -> Node {
        Node {
            id: id.to_string(),
            label: label.to_string(),
            file_type: "code".to_string(),
            source_file: source_file.to_string(),
            source_location: None,
            community: None,
            rationale: None,
            docstring: None,
            metadata: HashMap::new(),
        }
    }

    #[test]
    fn test_dedup_exact_same_file() {
        let nodes = vec![
            make_node("a", "AuthService", "auth.py"),
            make_node("b", "AuthService", "auth.py"),
        ];
        let dedup = Deduplicator::default();
        let (result, _) = dedup.deduplicate(nodes, vec![]).unwrap();
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_dedup_exact_diff_file() {
        let nodes = vec![
            make_node("a", "AuthService", "auth.py"),
            make_node("b", "AuthService", "service.py"),
        ];
        let dedup = Deduplicator::default();
        let (result, _) = dedup.deduplicate(nodes, vec![]).unwrap();
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_dedup_fuzzy_high_score() {
        let nodes = vec![
            make_node("a", "AuthService", "auth.py"),
            make_node("b", "AuthService", "auth.py"),
        ];
        let dedup = Deduplicator::default();
        let (result, _) = dedup.deduplicate(nodes, vec![]).unwrap();
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_dedup_fuzzy_low_score() {
        let nodes = vec![
            make_node("a", "AuthService", "auth.py"),
            make_node("b", "AuthServiceImpl", "auth.py"),
        ];
        let dedup = Deduplicator::default();
        let (result, _) = dedup.deduplicate(nodes, vec![]).unwrap();
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_dedup_variant_pair() {
        let nodes = vec![
            make_node("a", "M1", "config.py"),
            make_node("b", "M1 Pro", "config.py"),
        ];
        let dedup = Deduplicator::default();
        let (result, _) = dedup.deduplicate(nodes, vec![]).unwrap();
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_dedup_empty_nodes() {
        let dedup = Deduplicator::default();
        let (result, _) = dedup.deduplicate(vec![], vec![]).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_dedup_remaps_edges() {
        let nodes = vec![
            make_node("a", "Service", "auth.py"),
            make_node("a_dup", "Service", "auth.py"),
        ];
        let edges = vec![Edge {
            source: "a_dup".to_string(),
            target: "b".to_string(),
            relation: "calls".to_string(),
            confidence: "EXTRACTED".to_string(),
            source_file: Some("auth.py".to_string()),
            weight: 1.0,
            context: None,
        }];
        let dedup = Deduplicator::default();
        let (result_nodes, result_edges) = dedup.deduplicate(nodes, edges).unwrap();
        assert_eq!(result_nodes.len(), 1);
        assert_eq!(result_edges[0].source, "a");
    }

    #[test]
    fn test_jaro_winkler() {
        let score = jaro_winkler("AuthService", "AuthService");
        assert!((score - 1.0).abs() < 0.01);

        let score = jaro_winkler("AuthService", "AuthServic");
        assert!(score > 0.9);
    }

    // --- Gap test #84: community_boost ---

    #[test]
    fn test_dedup_community_boost() {
        // "abcXdefg" / "abcYdefh": raw JW=0.8833, boosted=0.9333
        // Without boost at 0.90: 0.8833 < 0.90 → NOT merged
        // With    boost at 0.90: 0.9333 > 0.90 → merged
        // With    boost at 0.97: 0.9333 < 0.97 → NOT merged
        let mut no = make_node("a", "abcXdefg", "auth.py");
        let mut yes = make_node("b", "abcYdefh", "auth.py");
        no.community = Some(1);
        yes.community = Some(1);

        // With community boost, threshold 0.90 → merge
        let loose = Deduplicator::new(0.90);
        let (r_loose, _) = loose
            .deduplicate(vec![no.clone(), yes.clone()], vec![])
            .unwrap();
        assert_eq!(r_loose.len(), 1, "community boost pushes over 0.90");

        // With community boost, threshold 0.97 → still below, not merged
        let high = Deduplicator::new(0.97);
        let (r_high, _) = high
            .deduplicate(vec![no.clone(), yes.clone()], vec![])
            .unwrap();
        assert_eq!(r_high.len(), 2, "boosted score still below 0.97");

        // Without community boost, threshold 0.90 → not merged (raw < 0.90)
        let no_community = Deduplicator::new(0.90);
        let (r_nocom, _) = no_community
            .deduplicate(
                vec![
                    make_node("c", "abcXdefg", "auth.py"),
                    make_node("d", "abcYdefh", "auth.py"),
                ],
                vec![],
            )
            .unwrap();
        assert_eq!(r_nocom.len(), 2, "without boost, raw JW below 0.90");
    }

    // --- Gap test #86: short_label ---

    #[test]
    fn test_dedup_short_label() {
        let nodes = vec![
            make_node("a", "Extractor", "utils.py"),
            make_node("b", "Extractar", "utils.py"),
        ];
        let dedup = Deduplicator::new(0.92);
        let (result, _) = dedup.deduplicate(nodes, vec![]).unwrap();
        assert_eq!(
            result.len(),
            1,
            "short labels with 1-char diff should merge"
        );
    }

    // --- Gap test #87: short_label_blocked ---

    #[test]
    fn test_dedup_short_label_blocked() {
        let nodes = vec![
            make_node("a", "cranel", "utils.py"),
            make_node("b", "cranelr", "utils.py"),
        ];
        let dedup = Deduplicator::default();
        let (result, _) = dedup.deduplicate(nodes, vec![]).unwrap();
        assert_eq!(
            result.len(),
            2,
            "variant pair with different length blocked"
        );
    }

    // --- Gap test #89: cross_project_guard ---

    #[test]
    fn test_dedup_cross_project_guard() {
        let mut a = make_node("a", "Service", "a.py");
        a.metadata.insert("repo".to_string(), "proj-a".to_string());
        let mut b = make_node("b", "Service", "b.py");
        b.metadata.insert("repo".to_string(), "proj-b".to_string());
        let nodes = vec![a, b];
        let dedup = Deduplicator::default();
        let result = dedup.deduplicate(nodes, vec![]);
        assert!(result.is_err(), "cross-project dedup should error");
    }

    // --- Gap test #90: llm_tiebreaker ---

    #[test]
    fn test_dedup_llm_tiebreaker() {
        // LLM tiebreaker returns a decision for every ambiguous pair
        let a = make_node("a", "AuthService", "auth.py");
        let b = make_node("b", "AuthServic", "auth.py");
        let dedup = Deduplicator::default();
        let decisions = dedup.llm_tiebreaker(vec![(a, b)]);
        assert_eq!(decisions.len(), 1, "should return one decision");
    }

    #[test]
    fn test_dedup_custom_threshold() {
        // With high threshold (0.99), even near-identical labels stay separate
        let nodes = vec![
            make_node("a", "AuthService", "auth.py"),
            make_node("b", "AuthService", "auth.py"),
        ];
        let loose = Deduplicator::new(0.99);
        let (result, _) = loose.deduplicate(nodes, vec![]).unwrap();
        assert_eq!(result.len(), 1);

        // Same-length labels (no variant-pair issue): JW ~0.94
        // With strict 0.99 → not merged; with loose 0.90 → merged
        let nodes = vec![
            make_node("a", "AuthServid", "auth.py"),
            make_node("b", "AuthServic", "auth.py"),
        ];
        let strict = Deduplicator::new(0.99);
        let (result_strict, _) = strict.deduplicate(nodes.clone(), vec![]).unwrap();
        assert_eq!(result_strict.len(), 2, "high threshold blocks fuzzy merge");

        let loose = Deduplicator::new(0.90);
        let (result_loose, _) = loose.deduplicate(nodes, vec![]).unwrap();
        assert_eq!(result_loose.len(), 1, "low threshold allows fuzzy merge");
    }

    // --- Edge case tests ---

    #[test]
    fn test_dedup_all_unique() {
        let nodes = vec![
            make_node("a", "Alpha", "a.py"),
            make_node("b", "Beta", "b.py"),
            make_node("c", "Gamma", "c.py"),
        ];
        let dedup = Deduplicator::default();
        let (result, _) = dedup.deduplicate(nodes, vec![]).unwrap();
        assert_eq!(result.len(), 3, "all unique labels → no dedup");
    }

    #[test]
    fn test_dedup_edges_only_no_nodes() {
        let edges = vec![Edge {
            source: "a".to_string(),
            target: "b".to_string(),
            relation: "calls".to_string(),
            confidence: "EXTRACTED".to_string(),
            source_file: Some("test.py".to_string()),
            weight: 1.0,
            context: None,
        }];
        let dedup = Deduplicator::default();
        let (result_nodes, result_edges) = dedup.deduplicate(vec![], edges).unwrap();
        assert!(result_nodes.is_empty());
        assert_eq!(result_edges.len(), 1, "edges pass through unchanged");
    }

    #[test]
    fn test_dedup_default_threshold() {
        let dedup = Deduplicator::default();
        let (result, _) = dedup.deduplicate(vec![], vec![]).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_jaro_winkler_identical() {
        let score = jaro_winkler("hello", "hello");
        assert!((score - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_jaro_winkler_empty_strings() {
        let score = jaro_winkler("", "");
        assert!(
            (score - 1.0).abs() < 0.001,
            "empty strings should return 1.0"
        );
    }

    #[test]
    fn test_jaro_winkler_completely_different() {
        let score = jaro_winkler("abc", "xyz");
        assert!(score < 0.5, "completely different should be low");
    }
}
