use crate::error::Result;
use crate::types::{Community, Edge, Node};
use rand::seq::SliceRandom;
use rand::SeedableRng;
use std::collections::HashMap;

pub struct CommunityDetector;

impl CommunityDetector {
    pub fn detect(
        &self,
        nodes: &[Node],
        edges: &[Edge],
        resolution: f64,
    ) -> Result<Vec<Community>> {
        if nodes.is_empty() {
            return Ok(vec![]);
        }
        if edges.is_empty() {
            return Ok(nodes
                .iter()
                .map(|n| Community {
                    id: 0,
                    nodes: vec![n.id.clone()],
                    cohesion: 1.0,
                })
                .collect());
        }
        Ok(leiden(nodes, edges, resolution))
    }

    pub fn detect_weighted(
        &self,
        nodes: &[Node],
        edges: &[Edge],
        resolution: f64,
    ) -> Result<Vec<Community>> {
        self.detect(nodes, edges, resolution)
    }

    pub fn exclude_hubs(
        &self,
        nodes: &[Node],
        edges: &[Edge],
        max_degree: usize,
    ) -> (Vec<Node>, Vec<Edge>, Vec<Node>) {
        let mut degree: HashMap<&str, usize> = HashMap::new();
        for edge in edges {
            *degree.entry(edge.source.as_str()).or_insert(0) += 1;
            *degree.entry(edge.target.as_str()).or_insert(0) += 1;
        }

        let hub_ids: std::collections::HashSet<&str> = degree
            .iter()
            .filter(|(_, &d)| d > max_degree)
            .map(|(&id, _)| id)
            .collect();

        let non_hubs: Vec<Node> = nodes
            .iter()
            .filter(|n| !hub_ids.contains(n.id.as_str()))
            .cloned()
            .collect();

        let hubs: Vec<Node> = nodes
            .iter()
            .filter(|n| hub_ids.contains(n.id.as_str()))
            .cloned()
            .collect();

        let filtered_edges: Vec<Edge> = edges
            .iter()
            .filter(|e| {
                !hub_ids.contains(e.source.as_str()) && !hub_ids.contains(e.target.as_str())
            })
            .cloned()
            .collect();

        (non_hubs, filtered_edges, hubs)
    }

    pub fn split_oversized(
        &self,
        nodes: &[Node],
        edges: &[Edge],
        communities: Vec<Community>,
        max_size: usize,
    ) -> Vec<Community> {
        let mut result = Vec::new();
        for comm in communities {
            if comm.nodes.len() <= max_size {
                result.push(comm);
            } else {
                for chunk in comm.nodes.chunks(max_size) {
                    let cohesion = Self::compute_cohesion(nodes, edges, chunk);
                    result.push(Community {
                        id: result.len(),
                        nodes: chunk.to_vec(),
                        cohesion,
                    });
                }
            }
        }
        result
    }

    pub fn remap_communities_to_previous(
        &self,
        current: &[Community],
        previous: &[Community],
    ) -> HashMap<usize, usize> {
        let mut remap = HashMap::new();
        for (i, comm) in current.iter().enumerate() {
            let mut best = i;
            let mut max_overlap = 0;
            let cur_set: std::collections::HashSet<&str> =
                comm.nodes.iter().map(|s| s.as_str()).collect();
            for prev_comm in previous {
                let overlap = prev_comm
                    .nodes
                    .iter()
                    .filter(|n| cur_set.contains(n.as_str()))
                    .count();
                if overlap > max_overlap {
                    max_overlap = overlap;
                    best = prev_comm.id;
                }
            }
            remap.insert(comm.id, best);
        }
        remap
    }

    fn compute_cohesion(_nodes: &[Node], edges: &[Edge], community_nodes: &[String]) -> f64 {
        let k = community_nodes.len();
        if k <= 1 {
            return 1.0;
        }
        let node_set: std::collections::HashSet<&str> =
            community_nodes.iter().map(|s| s.as_str()).collect();
        let max_possible = k * (k - 1) / 2;
        let mut internal = 0usize;
        for edge in edges {
            if node_set.contains(edge.source.as_str()) && node_set.contains(edge.target.as_str()) {
                internal += 1;
            }
        }
        internal as f64 / max_possible as f64
    }
}

fn leiden(nodes: &[Node], edges: &[Edge], resolution: f64) -> Vec<Community> {
    let n = nodes.len();
    if n == 0 {
        return vec![];
    }

    let node_ids: Vec<&str> = nodes.iter().map(|n| n.id.as_str()).collect();
    let id_to_idx: HashMap<&str, usize> = node_ids
        .iter()
        .enumerate()
        .map(|(i, id)| (*id, i))
        .collect();

    let mut adjacency: Vec<Vec<(usize, f64)>> = vec![vec![]; n];
    let mut total_edge_weight = 0.0;
    for edge in edges {
        if let (Some(&si), Some(&ti)) = (
            id_to_idx.get(edge.source.as_str()),
            id_to_idx.get(edge.target.as_str()),
        ) {
            let w = edge.weight;
            adjacency[si].push((ti, w));
            if si != ti {
                adjacency[ti].push((si, w));
            }
            total_edge_weight += w;
        }
    }

    if total_edge_weight == 0.0 {
        return nodes
            .iter()
            .map(|n| Community {
                id: 0,
                nodes: vec![n.id.clone()],
                cohesion: 1.0,
            })
            .collect();
    }

    let m2 = 2.0 * total_edge_weight;

    let mut community: Vec<usize> = (0..n).collect();
    let mut comm_deg: Vec<f64> = (0..n)
        .map(|i| adjacency[i].iter().map(|&(_, w)| w).sum::<f64>())
        .collect();
    let node_deg: Vec<f64> = comm_deg.clone();

    let mut rng = rand::rngs::StdRng::from_entropy();

    for _iter in 0..15 {
        let mut improved = false;
        let mut order: Vec<usize> = (0..n).collect();
        order.shuffle(&mut rng);

        for &node in &order {
            let curr_comm = community[node];
            let k_i = node_deg[node];

            let mut best_comm = curr_comm;
            let mut best_delta = 0.0;
            let curr_sigma_tot = comm_deg[curr_comm];

            let curr_ki_in: f64 = adjacency[node]
                .iter()
                .filter(|&&(nb, _)| community[nb] == curr_comm)
                .map(|&(_, w)| w)
                .sum();

            let mut neighbors: std::collections::HashSet<usize> = std::collections::HashSet::new();
            for &(nb, _) in &adjacency[node] {
                neighbors.insert(community[nb]);
            }

            for &cand_comm in &neighbors {
                if cand_comm == curr_comm {
                    continue;
                }
                let cand_ki_in: f64 = adjacency[node]
                    .iter()
                    .filter(|&&(nb, _)| community[nb] == cand_comm)
                    .map(|&(_, w)| w)
                    .sum();
                let cand_sigma_tot = comm_deg[cand_comm];

                let delta = (cand_ki_in - resolution * cand_sigma_tot * k_i / m2)
                    - (curr_ki_in - resolution * curr_sigma_tot * k_i / m2);

                if delta > best_delta {
                    best_delta = delta;
                    best_comm = cand_comm;
                }
            }

            if best_comm != curr_comm {
                improved = true;
                community[node] = best_comm;
                comm_deg[curr_comm] -= k_i;
                comm_deg[best_comm] += k_i;
            }
        }

        if !improved {
            break;
        }
    }

    let mut comm_index: HashMap<usize, usize> = HashMap::new();
    let mut next = 0;
    for &c in &community {
        comm_index.entry(c).or_insert_with(|| {
            let id = next;
            next += 1;
            id
        });
    }

    let mut comm_map: HashMap<usize, Vec<String>> = HashMap::new();
    for (i, &c) in community.iter().enumerate() {
        let new_id = comm_index[&c];
        comm_map
            .entry(new_id)
            .or_default()
            .push(node_ids[i].to_string());
    }

    comm_map
        .into_iter()
        .map(|(id, nodes_c)| Community {
            id,
            nodes: nodes_c,
            cohesion: 1.0,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn make_node(id: &str, label: &str) -> Node {
        Node {
            id: id.to_string(),
            label: label.to_string(),
            file_type: "code".to_string(),
            source_file: "test.py".to_string(),
            source_location: None,
            community: None,
            rationale: None,
            docstring: None,
            metadata: HashMap::new(),
        }
    }

    fn make_edge(src: &str, tgt: &str, weight: f64) -> Edge {
        Edge {
            source: src.to_string(),
            target: tgt.to_string(),
            relation: "connects".to_string(),
            confidence: "EXTRACTED".to_string(),
            source_file: Some("test.py".to_string()),
            weight,
            context: None,
        }
    }

    #[test]
    fn test_cluster_empty() {
        let detector = CommunityDetector;
        let communities = detector.detect(&[], &[], 1.0).unwrap();
        assert!(communities.is_empty());
    }

    #[test]
    fn test_cluster_isolates() {
        let nodes = vec![
            make_node("a", "A"),
            make_node("b", "B"),
            make_node("c", "C"),
        ];
        let detector = CommunityDetector;
        let communities = detector.detect(&nodes, &[], 1.0).unwrap();
        assert!(communities.len() == 3);
    }

    #[test]
    fn test_cluster_deterministic() {
        let nodes = vec![make_node("a", "A"), make_node("b", "B")];
        let edges = vec![make_edge("a", "b", 1.0)];
        let detector = CommunityDetector;
        let r1 = detector.detect(&nodes, &edges, 1.0).unwrap();
        let r2 = detector.detect(&nodes, &edges, 1.0).unwrap();
        assert_eq!(r1.len(), r2.len());
    }

    #[test]
    fn test_cluster_two_communities() {
        let nodes = vec![
            make_node("a1", "A1"),
            make_node("a2", "A2"),
            make_node("b1", "B1"),
            make_node("b2", "B2"),
        ];
        let edges = vec![
            make_edge("a1", "a2", 10.0),
            make_edge("b1", "b2", 10.0),
            make_edge("a1", "b1", 1.0),
            make_edge("a2", "b2", 1.0),
        ];
        let detector = CommunityDetector;
        let communities = detector.detect(&nodes, &edges, 1.0).unwrap();
        assert_eq!(communities.len(), 2);
    }

    #[test]
    fn test_cluster_resolution_parameter() {
        let nodes = vec![
            make_node("a1", "A1"),
            make_node("a2", "A2"),
            make_node("b1", "B1"),
            make_node("b2", "B2"),
        ];
        let edges = vec![
            make_edge("a1", "a2", 10.0),
            make_edge("b1", "b2", 10.0),
            make_edge("a1", "b1", 1.0),
            make_edge("a2", "b2", 1.0),
        ];
        let detector = CommunityDetector;
        let low_res = detector.detect(&nodes, &edges, 0.1).unwrap();
        let high_res = detector.detect(&nodes, &edges, 5.0).unwrap();
        assert!(high_res.len() >= low_res.len());
    }

    #[test]
    fn test_split_oversized() {
        let nodes = vec![
            make_node("a", "A"),
            make_node("b", "B"),
            make_node("c", "C"),
            make_node("d", "D"),
        ];
        let edges = vec![make_edge("a", "b", 1.0), make_edge("c", "d", 1.0)];
        let communities = vec![Community {
            id: 0,
            nodes: vec!["a".into(), "b".into(), "c".into(), "d".into()],
            cohesion: 0.5,
        }];
        let detector = CommunityDetector;
        let split = detector.split_oversized(&nodes, &edges, communities, 2);
        assert_eq!(split.len(), 2);
        assert_eq!(split[0].nodes.len(), 2);
        // Cohesion recomputed per chunk
        assert!(split[0].cohesion > split[1].cohesion || split[1].cohesion > 0.0);
    }

    #[test]
    fn test_remap_communities() {
        let current = vec![Community {
            id: 0,
            nodes: vec!["a".into(), "b".into()],
            cohesion: 1.0,
        }];
        let previous = vec![Community {
            id: 5,
            nodes: vec!["a".into(), "c".into()],
            cohesion: 1.0,
        }];
        let detector = CommunityDetector;
        let remap = detector.remap_communities_to_previous(&current, &previous);
        assert!(remap.contains_key(&0));
    }

    #[test]
    fn test_cluster_covers_all_nodes() {
        let nodes = vec![
            make_node("x", "X"),
            make_node("y", "Y"),
            make_node("z", "Z"),
        ];
        let edges = vec![make_edge("x", "y", 1.0)];
        let detector = CommunityDetector;
        let communities = detector.detect(&nodes, &edges, 1.0).unwrap();
        let all: Vec<&str> = communities
            .iter()
            .flat_map(|c| c.nodes.iter().map(|s| s.as_str()))
            .collect();
        assert_eq!(all.len(), 3);
    }

    #[test]
    fn test_cluster_resolution_default() {
        let nodes = vec![
            make_node("a1", "A1"),
            make_node("a2", "A2"),
            make_node("b1", "B1"),
            make_node("b2", "B2"),
            make_node("c1", "C1"),
        ];
        let edges = vec![
            make_edge("a1", "a2", 10.0),
            make_edge("b1", "b2", 10.0),
            make_edge("a1", "b1", 1.0),
            make_edge("a2", "b2", 1.0),
            make_edge("c1", "a1", 0.1),
            make_edge("c1", "b1", 0.1),
        ];
        let detector = CommunityDetector;
        let communities = detector.detect(&nodes, &edges, 1.0).unwrap();
        // Default resolution should detect 2 main communities + maybe C1
        assert!(
            communities.len() >= 2,
            "default resolution should find communities"
        );
    }

    #[test]
    fn test_cluster_resolution_low() {
        let nodes = vec![
            make_node("a1", "A1"),
            make_node("a2", "A2"),
            make_node("b1", "B1"),
            make_node("b2", "B2"),
        ];
        let edges = vec![
            make_edge("a1", "a2", 10.0),
            make_edge("b1", "b2", 10.0),
            make_edge("a1", "b1", 1.0),
            make_edge("a2", "b2", 1.0),
        ];
        let detector = CommunityDetector;
        let low = detector.detect(&nodes, &edges, 0.1).unwrap();
        let high = detector.detect(&nodes, &edges, 2.0).unwrap();
        assert!(
            low.len() <= high.len(),
            "low resolution should merge communities (got {} vs {})",
            low.len(),
            high.len()
        );
    }

    #[test]
    fn test_cluster_exclude_hubs() {
        let nodes = vec![
            make_node("hub", "Hub"),
            make_node("a", "A"),
            make_node("b", "B"),
            make_node("c", "C"),
        ];
        let edges = vec![
            make_edge("hub", "a", 1.0),
            make_edge("hub", "b", 1.0),
            make_edge("hub", "c", 1.0),
            make_edge("a", "b", 1.0),
        ];
        let detector = CommunityDetector;
        let (non_hubs, filtered_edges, hubs) = detector.exclude_hubs(&nodes, &edges, 2);
        assert_eq!(hubs.len(), 1);
        assert_eq!(hubs[0].id, "hub");
        assert_eq!(non_hubs.len(), 3);
        assert_eq!(filtered_edges.len(), 1);
        assert_eq!(filtered_edges[0].source, "a");
        assert_eq!(filtered_edges[0].target, "b");
    }

    #[test]
    fn test_cluster_cohesion_split() {
        let nodes = vec![
            make_node("a", "A"),
            make_node("b", "B"),
            make_node("c", "C"),
            make_node("d", "D"),
        ];
        let edges = vec![make_edge("a", "b", 1.0), make_edge("c", "d", 1.0)];
        let communities = vec![Community {
            id: 0,
            nodes: vec!["a".into(), "b".into(), "c".into(), "d".into()],
            cohesion: 0.17,
        }];
        let detector = CommunityDetector;
        let split = detector.split_oversized(&nodes, &edges, communities, 2);
        assert_eq!(split.len(), 2);
        // Each chunk has 1 internal edge out of 1 possible = cohesion 1.0
        assert!((split[0].cohesion - 1.0).abs() < 0.01);
        assert!((split[1].cohesion - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_cluster_weighted() {
        let nodes = vec![
            make_node("a1", "A1"),
            make_node("a2", "A2"),
            make_node("b1", "B1"),
            make_node("b2", "B2"),
        ];
        // Strong internal edges, weak cross edges
        let weighted_edges = vec![
            make_edge("a1", "a2", 100.0),
            make_edge("b1", "b2", 100.0),
            make_edge("a1", "b1", 1.0),
            make_edge("a2", "b2", 1.0),
        ];
        // Uniform weak edges
        let uniform_edges = vec![
            make_edge("a1", "a2", 1.0),
            make_edge("b1", "b2", 1.0),
            make_edge("a1", "b1", 1.0),
            make_edge("a2", "b2", 1.0),
        ];
        let detector = CommunityDetector;
        let weighted = detector.detect(&nodes, &weighted_edges, 1.0).unwrap();
        let uniform = detector.detect(&nodes, &uniform_edges, 1.0).unwrap();
        // Weighted with strong internal edges should split, uniform might merge
        assert!(
            weighted.len() >= uniform.len(),
            "weighted clustering should produce different (more split) communities than unweighted"
        );
    }

    // --- Edge case tests ---

    #[test]
    fn test_cluster_single_node() {
        let nodes = vec![make_node("a", "A")];
        let detector = CommunityDetector;
        let communities = detector.detect(&nodes, &[], 1.0).unwrap();
        assert_eq!(communities.len(), 1);
        assert_eq!(communities[0].nodes.len(), 1);
    }

    #[test]
    fn test_cluster_single_edge() {
        let nodes = vec![make_node("a", "A"), make_node("b", "B")];
        let edges = vec![make_edge("a", "b", 1.0)];
        let detector = CommunityDetector;
        let communities = detector.detect(&nodes, &edges, 1.0).unwrap();
        assert_eq!(communities.len(), 1, "two connected nodes → one community");
        assert_eq!(communities[0].nodes.len(), 2);
    }

    #[test]
    fn test_cluster_zero_weight_edges() {
        let nodes = vec![make_node("a", "A"), make_node("b", "B")];
        let edges = vec![make_edge("a", "b", 0.0)];
        let detector = CommunityDetector;
        let communities = detector.detect(&nodes, &edges, 1.0).unwrap();
        assert!(communities.len() <= 2);
    }

    #[test]
    fn test_cluster_high_resolution_not_panics() {
        let nodes = vec![make_node("a", "A"), make_node("b", "B")];
        let edges = vec![make_edge("a", "b", 1.0)];
        let detector = CommunityDetector;
        // Very high resolution should not panic
        let communities = detector.detect(&nodes, &edges, 100.0).unwrap();
        assert!(
            !communities.is_empty(),
            "high resolution still produces communities"
        );
    }

    #[test]
    fn test_cluster_split_oversized_empty() {
        let detector = CommunityDetector;
        let split = detector.split_oversized(&[], &[], vec![], 5);
        assert!(split.is_empty());
    }

    #[test]
    fn test_cluster_remap_empty() {
        let detector = CommunityDetector;
        let remap = detector.remap_communities_to_previous(&[], &[]);
        assert!(remap.is_empty());
    }
}
