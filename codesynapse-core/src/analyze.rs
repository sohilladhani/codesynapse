use crate::error::Result;
use crate::types::{AnalysisResult, Edge, Node, NodeId};
use std::collections::{HashMap, HashSet};
use std::process::Command;

pub struct Analyzer;

impl Analyzer {
    pub fn god_nodes(&self, nodes: &[Node], edges: &[Edge], top_n: usize) -> Vec<Node> {
        let mut degree: HashMap<&str, usize> = HashMap::new();
        for edge in edges {
            *degree.entry(edge.source.as_str()).or_insert(0) += 1;
            *degree.entry(edge.target.as_str()).or_insert(0) += 1;
        }

        let mut candidates: Vec<&Node> = nodes
            .iter()
            .filter(|n| {
                let deg = *degree.get(n.id.as_str()).unwrap_or(&0);
                !n.label.ends_with(".py")
                    && !n.label.ends_with(".js")
                    && !n.label.ends_with(".ts")
                    && !n.label.ends_with(".rs")
                    && !n.label.ends_with(".go")
                    && !n.label.ends_with(".java")
                    && !n.label.ends_with(".c")
                    && !n.label.ends_with(".cpp")
                    && !n.label.ends_with(".h")
                    && !n.label.contains("().")
                    && !(n.label.starts_with(".") && n.label.ends_with("()"))
                    && !(n.label.ends_with("()") && deg <= 1)
            })
            .collect();

        candidates.sort_by(|a, b| {
            let deg_a = degree.get(a.id.as_str()).unwrap_or(&0);
            let deg_b = degree.get(b.id.as_str()).unwrap_or(&0);
            deg_b.cmp(deg_a)
        });

        candidates.truncate(top_n);
        candidates.into_iter().cloned().collect()
    }

    pub fn surprising_connections(
        &self,
        edges: &[Edge],
        cross_language_suppression: bool,
    ) -> Vec<Edge> {
        if cross_language_suppression {
            edges
                .iter()
                .filter(|e| {
                    e.confidence == "INFERRED"
                        && e.source_file.as_deref().is_none_or(|sf| {
                            sf.ends_with(".py")
                                || sf.ends_with(".js")
                                || sf.ends_with(".ts")
                                || sf.ends_with(".rs")
                                || sf.ends_with(".go")
                        })
                })
                .cloned()
                .collect()
        } else {
            edges
                .iter()
                .filter(|e| e.confidence == "INFERRED")
                .cloned()
                .collect()
        }
    }

    pub fn suggest_questions(&self, nodes: &[Node], edges: &[Edge]) -> Vec<String> {
        let mut questions = Vec::new();

        questions.push("What are the core modules and how do they relate?".to_string());
        questions.push("Which components have the most dependencies?".to_string());

        let has_cycles = self.detect_cycles(nodes, edges);
        if has_cycles {
            questions.push("Are there any circular dependencies between modules?".to_string());
        }

        questions.push("How does data flow through the system?".to_string());

        let lang_count = self.count_languages(nodes);
        if lang_count.len() > 1 {
            questions.push(format!(
                "How do the {} different languages interact?",
                lang_count.len()
            ));
        }

        questions
    }

    pub fn cohesion_score(
        &self,
        _nodes: &[Node],
        edges: &[Edge],
        community_nodes: &[NodeId],
    ) -> f64 {
        if community_nodes.len() <= 1 {
            return 1.0;
        }

        let node_set: HashSet<&str> = community_nodes.iter().map(|s| s.as_str()).collect();
        let internal_edges = edges
            .iter()
            .filter(|e| {
                node_set.contains(e.source.as_str()) && node_set.contains(e.target.as_str())
            })
            .count();
        let max_possible = community_nodes.len() * (community_nodes.len() - 1) / 2;

        if max_possible == 0 {
            return 1.0;
        }

        internal_edges as f64 / max_possible as f64
    }

    pub fn pagerank(&self, edges: &[Edge], damping: f64, max_iter: usize) -> HashMap<String, f64> {
        let mut node_degree: HashMap<&str, f64> = HashMap::new();
        let mut outlinks: HashMap<&str, Vec<&str>> = HashMap::new();
        let mut all_nodes: HashSet<&str> = HashSet::new();

        for edge in edges {
            all_nodes.insert(edge.source.as_str());
            all_nodes.insert(edge.target.as_str());
            *node_degree.entry(edge.source.as_str()).or_insert(0.0) += 1.0;
            outlinks
                .entry(edge.source.as_str())
                .or_default()
                .push(edge.target.as_str());
        }

        let n = all_nodes.len() as f64;
        if n == 0.0 {
            return HashMap::new();
        }

        let mut ranks: HashMap<String, f64> = all_nodes
            .iter()
            .map(|&id| (id.to_string(), 1.0 / n))
            .collect();

        for _iter in 0..max_iter {
            let mut new_ranks: HashMap<String, f64> = HashMap::new();
            let dangling_sum: f64 = all_nodes
                .iter()
                .filter(|id| outlinks.get(*id).is_none_or(|v| v.is_empty()))
                .map(|id| ranks[*id])
                .sum();

            for &node in &all_nodes {
                let mut score = (1.0 - damping) / n;
                score += damping * dangling_sum / n;

                for (&src, targets) in &outlinks {
                    if targets.contains(&node) {
                        score +=
                            damping * ranks.get(src).copied().unwrap_or(0.0) / targets.len() as f64;
                    }
                }

                new_ranks.insert(node.to_string(), score);
            }

            ranks = new_ranks;
        }

        ranks
    }

    pub fn graph_diff(&self, old: &[Edge], new: &[Edge]) -> (Vec<Edge>, Vec<Edge>) {
        let old_set: HashSet<(&str, &str, &str)> = old
            .iter()
            .map(|e| (e.source.as_str(), e.target.as_str(), e.relation.as_str()))
            .collect();
        let new_set: HashSet<(&str, &str, &str)> = new
            .iter()
            .map(|e| (e.source.as_str(), e.target.as_str(), e.relation.as_str()))
            .collect();

        let added: Vec<Edge> = new
            .iter()
            .filter(|e| {
                !old_set.contains(&(e.source.as_str(), e.target.as_str(), e.relation.as_str()))
            })
            .cloned()
            .collect();

        let removed: Vec<Edge> = old
            .iter()
            .filter(|e| {
                !new_set.contains(&(e.source.as_str(), e.target.as_str(), e.relation.as_str()))
            })
            .cloned()
            .collect();

        (added, removed)
    }

    fn detect_cycles(&self, _nodes: &[Node], edges: &[Edge]) -> bool {
        let mut graph: HashMap<&str, Vec<&str>> = HashMap::new();
        for edge in edges {
            graph
                .entry(edge.source.as_str())
                .or_default()
                .push(edge.target.as_str());
        }

        let mut visited: HashSet<&str> = HashSet::new();
        let mut stack: HashSet<&str> = HashSet::new();

        fn dfs<'a>(
            node: &'a str,
            graph: &HashMap<&'a str, Vec<&'a str>>,
            visited: &mut HashSet<&'a str>,
            stack: &mut HashSet<&'a str>,
        ) -> bool {
            if stack.contains(node) {
                return true;
            }
            if visited.contains(node) {
                return false;
            }
            visited.insert(node);
            stack.insert(node);
            if let Some(neighbors) = graph.get(node) {
                for &next in neighbors {
                    if dfs(next, graph, visited, stack) {
                        return true;
                    }
                }
            }
            stack.remove(node);
            false
        }

        let all_nodes: Vec<&str> = graph.keys().copied().collect();
        for node in all_nodes {
            if dfs(node, &graph, &mut visited, &mut stack) {
                return true;
            }
        }
        false
    }

    fn count_languages(&self, nodes: &[Node]) -> HashMap<String, usize> {
        let mut counts: HashMap<String, usize> = HashMap::new();
        for node in nodes {
            let lang = language_from_file(&node.source_file);
            *counts.entry(lang).or_insert(0) += 1;
        }
        counts
    }

    pub fn tarjan_scc(&self, edges: &[Edge]) -> Vec<Vec<String>> {
        let mut graph: HashMap<&str, Vec<&str>> = HashMap::new();
        for edge in edges {
            graph
                .entry(edge.source.as_str())
                .or_default()
                .push(edge.target.as_str());
            graph.entry(edge.target.as_str()).or_default();
        }

        let mut index_counter = 0usize;
        let mut index: HashMap<&str, usize> = HashMap::new();
        let mut lowlink: HashMap<&str, usize> = HashMap::new();
        let mut stack: Vec<&str> = Vec::new();
        let mut on_stack: HashSet<&str> = HashSet::new();
        let mut sccs: Vec<Vec<String>> = Vec::new();

        #[allow(clippy::too_many_arguments)]
        fn strongconnect<'a>(
            v: &'a str,
            graph: &HashMap<&'a str, Vec<&'a str>>,
            index_counter: &mut usize,
            index: &mut HashMap<&'a str, usize>,
            lowlink: &mut HashMap<&'a str, usize>,
            stack: &mut Vec<&'a str>,
            on_stack: &mut HashSet<&'a str>,
            sccs: &mut Vec<Vec<String>>,
        ) {
            index.insert(v, *index_counter);
            lowlink.insert(v, *index_counter);
            *index_counter += 1;
            stack.push(v);
            on_stack.insert(v);

            if let Some(neighbors) = graph.get(v) {
                for &w in neighbors {
                    if !index.contains_key(w) {
                        strongconnect(
                            w,
                            graph,
                            index_counter,
                            index,
                            lowlink,
                            stack,
                            on_stack,
                            sccs,
                        );
                        let v_low = lowlink[v].min(lowlink[w]);
                        lowlink.insert(v, v_low);
                    } else if on_stack.contains(w) {
                        let v_low = lowlink[v].min(index[w]);
                        lowlink.insert(v, v_low);
                    }
                }
            }

            if lowlink[v] == index[v] {
                let mut scc = Vec::new();
                loop {
                    let w = stack.pop().unwrap();
                    on_stack.remove(w);
                    scc.push(w.to_string());
                    if w == v {
                        break;
                    }
                }
                sccs.push(scc);
            }
        }

        let all_nodes: Vec<&str> = graph.keys().copied().collect();
        for node in all_nodes {
            if !index.contains_key(node) {
                strongconnect(
                    node,
                    &graph,
                    &mut index_counter,
                    &mut index,
                    &mut lowlink,
                    &mut stack,
                    &mut on_stack,
                    &mut sccs,
                );
            }
        }

        sccs
    }

    pub fn bridge_edges(&self, nodes: &[Node], edges: &[Edge]) -> Vec<Edge> {
        if nodes.is_empty() || edges.is_empty() {
            return vec![];
        }

        let id_to_idx: HashMap<&str, usize> = nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.id.as_str(), i))
            .collect();
        let n = nodes.len();
        let mut adj: Vec<Vec<(usize, usize)>> = vec![vec![]; n];

        for (ei, edge) in edges.iter().enumerate() {
            if let (Some(&si), Some(&ti)) = (
                id_to_idx.get(edge.source.as_str()),
                id_to_idx.get(edge.target.as_str()),
            ) {
                adj[si].push((ti, ei));
                adj[ti].push((si, ei));
            }
        }

        let mut visited = vec![false; n];
        let mut tin = vec![0usize; n];
        let mut low = vec![0usize; n];
        let mut timer = 0usize;
        let mut is_bridge = vec![false; edges.len()];

        #[allow(clippy::too_many_arguments)]
        fn dfs(
            v: usize,
            p: Option<usize>,
            adj: &[Vec<(usize, usize)>],
            visited: &mut [bool],
            tin: &mut [usize],
            low: &mut [usize],
            timer: &mut usize,
            is_bridge: &mut [bool],
        ) {
            visited[v] = true;
            tin[v] = *timer;
            low[v] = *timer;
            *timer += 1;

            for &(to, ei) in &adj[v] {
                if Some(to) == p {
                    continue;
                }
                if visited[to] {
                    low[v] = low[v].min(tin[to]);
                } else {
                    dfs(to, Some(v), adj, visited, tin, low, timer, is_bridge);
                    low[v] = low[v].min(low[to]);
                    if low[to] > tin[v] {
                        is_bridge[ei] = true;
                    }
                }
            }
        }

        for i in 0..n {
            if !visited[i] {
                dfs(
                    i,
                    None,
                    &adj,
                    &mut visited,
                    &mut tin,
                    &mut low,
                    &mut timer,
                    &mut is_bridge,
                );
            }
        }

        edges
            .iter()
            .enumerate()
            .filter(|(i, _)| is_bridge[*i])
            .map(|(_, e)| e.clone())
            .collect()
    }

    pub fn topological_sort(&self, edges: &[Edge]) -> Vec<String> {
        let mut graph: HashMap<&str, Vec<&str>> = HashMap::new();
        let mut in_degree: HashMap<&str, usize> = HashMap::new();

        for edge in edges {
            graph
                .entry(edge.source.as_str())
                .or_default()
                .push(edge.target.as_str());
            in_degree.entry(edge.source.as_str()).or_insert(0);
            *in_degree.entry(edge.target.as_str()).or_insert(0) += 1;
        }

        let mut queue = std::collections::VecDeque::new();
        for (&node, &deg) in &in_degree {
            if deg == 0 {
                queue.push_back(node);
            }
        }

        let mut result = Vec::new();
        while let Some(node) = queue.pop_front() {
            result.push(node.to_string());
            if let Some(neighbors) = graph.get(node) {
                for &next in neighbors {
                    if let Some(d) = in_degree.get_mut(next) {
                        *d -= 1;
                        if *d == 0 {
                            queue.push_back(next);
                        }
                    }
                }
            }
        }

        result
    }

    pub fn analyze(&self, nodes: &[Node], edges: &[Edge]) -> Result<AnalysisResult> {
        let god_nodes = self.god_nodes(nodes, edges, 10);
        let surprising = self.surprising_connections(edges, false);
        let questions = self.suggest_questions(nodes, edges);

        Ok(AnalysisResult {
            god_nodes,
            surprising_connections: surprising,
            suggested_questions: questions,
            community_cohesion: vec![],
        })
    }

    pub fn find_similar(&self, edges: &[Edge], node_id: &str, top_n: usize) -> Vec<(String, f64)> {
        let pairs: Vec<(String, String)> = edges
            .iter()
            .map(|e| (e.source.clone(), e.target.clone()))
            .collect();
        let n2v = crate::embedding::Node2Vec::new(64, 1.0, 1.0);
        let embeddings = n2v.train(&pairs);
        n2v.find_similar(&embeddings, node_id, top_n)
    }

    /// Compute temporal risk for file nodes based on git history.
    ///
    /// For each file node, the risk score is:
    ///   risk_score = churn_rate * degree_centrality * community_bridge_factor
    /// where:
    ///   - churn_rate: number of commits that touched the file (`git rev-list --count HEAD -- <file>`)
    ///   - degree_centrality: number of edges connected to the node
    ///   - community_bridge_factor: 1.0 + number of bridge edges the node participates in
    pub fn compute_temporal_risk(&self, nodes: &mut [Node], edges: &[Edge]) -> Result<()> {
        let is_git_repo = Command::new("git")
            .arg("rev-parse")
            .arg("--is-inside-work-tree")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false);

        if !is_git_repo {
            return Ok(());
        }

        let mut churn_map: HashMap<String, usize> = HashMap::new();
        for node in nodes.iter() {
            if node.file_type != "file" {
                continue;
            }
            let mut churn = 0usize;
            if let Ok(output) = Command::new("git")
                .arg("rev-list")
                .arg("--count")
                .arg("HEAD")
                .arg("--")
                .arg(&node.source_file)
                .output()
            {
                if output.status.success() {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    if let Ok(count) = stdout.trim().parse::<usize>() {
                        churn = count;
                    }
                }
            }
            churn_map.insert(node.id.clone(), churn);
        }

        self.apply_risk_scores(nodes, edges, &churn_map);
        Ok(())
    }

    /// Apply risk scores using pre-computed churn values. Separated for testability.
    pub(crate) fn apply_risk_scores(
        &self,
        nodes: &mut [Node],
        edges: &[Edge],
        churn_map: &HashMap<String, usize>,
    ) {
        let mut degree: HashMap<String, usize> = HashMap::new();
        for edge in edges {
            *degree.entry(edge.source.clone()).or_insert(0) += 1;
            *degree.entry(edge.target.clone()).or_insert(0) += 1;
        }

        let bridges = self.bridge_edges(nodes, edges);
        let mut bridge_count: HashMap<String, usize> = HashMap::new();
        for bridge in &bridges {
            *bridge_count.entry(bridge.source.clone()).or_insert(0) += 1;
            *bridge_count.entry(bridge.target.clone()).or_insert(0) += 1;
        }

        for node in nodes.iter_mut() {
            if node.file_type != "file" {
                continue;
            }
            let churn = churn_map.get(&node.id).copied().unwrap_or(0);
            let deg = degree.get(&node.id).copied().unwrap_or(0);
            let bridge_factor = 1.0 + bridge_count.get(&node.id).copied().unwrap_or(0) as f64;
            let risk_score = churn as f64 * deg as f64 * bridge_factor;

            node.metadata
                .insert("risk_score".to_string(), risk_score.to_string());
            node.rationale = Some(format!(
                "Risk score: {risk_score:.2} (churn={churn}, degree={deg}, bridge_factor={bridge_factor:.1})"
            ));
        }
    }
}

fn language_from_file(path: &str) -> String {
    if path.ends_with(".py") {
        "Python".to_string()
    } else if path.ends_with(".js") || path.ends_with(".jsx") {
        "JavaScript".to_string()
    } else if path.ends_with(".ts") || path.ends_with(".tsx") {
        "TypeScript".to_string()
    } else if path.ends_with(".rs") {
        "Rust".to_string()
    } else if path.ends_with(".go") {
        "Go".to_string()
    } else if path.ends_with(".java") {
        "Java".to_string()
    } else if path.ends_with(".c") || path.ends_with(".h") {
        "C".to_string()
    } else if path.ends_with(".cpp") || path.ends_with(".hpp") {
        "C++".to_string()
    } else if path.ends_with(".rb") {
        "Ruby".to_string()
    } else if path.ends_with(".php") {
        "PHP".to_string()
    } else if path.ends_with(".swift") {
        "Swift".to_string()
    } else if path.ends_with(".kt") || path.ends_with(".kts") {
        "Kotlin".to_string()
    } else {
        "Other".to_string()
    }
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

    fn make_node_id(id: &str) -> Node {
        make_node(id, id, "test.py")
    }

    fn make_edge(src: &str, tgt: &str, relation: &str, confidence: &str) -> Edge {
        Edge {
            source: src.to_string(),
            target: tgt.to_string(),
            relation: relation.to_string(),
            confidence: confidence.to_string(),
            source_file: Some("test.py".to_string()),
            weight: 1.0,
            context: None,
        }
    }

    fn make_edge_with_file(
        src: &str,
        tgt: &str,
        relation: &str,
        confidence: &str,
        file: &str,
    ) -> Edge {
        Edge {
            source: src.to_string(),
            target: tgt.to_string(),
            relation: relation.to_string(),
            confidence: confidence.to_string(),
            source_file: Some(file.to_string()),
            weight: 1.0,
            context: None,
        }
    }

    #[test]
    fn test_god_nodes_basic() {
        let nodes = vec![make_node_id("a"), make_node_id("b"), make_node_id("c")];
        let edges = vec![
            make_edge("a", "b", "imports", "EXTRACTED"),
            make_edge("a", "c", "imports", "EXTRACTED"),
        ];
        let analyzer = Analyzer;
        let gods = analyzer.god_nodes(&nodes, &edges, 10);
        assert_eq!(gods[0].id, "a");
    }

    #[test]
    fn test_god_nodes_exclude_file() {
        let nodes = vec![
            make_node("a", "A", "test.py"),
            make_node("main.py", "main.py", "test.py"),
        ];
        let edges = vec![make_edge("main.py", "a", "contains", "EXTRACTED")];
        let analyzer = Analyzer;
        let gods = analyzer.god_nodes(&nodes, &edges, 10);
        assert_eq!(gods.len(), 1);
        assert_eq!(gods[0].id, "a");
    }

    #[test]
    fn test_god_nodes_exclude_method_stub() {
        // ".foo()" method stub and isolated "bar()" function stub (degree 1) must be excluded
        let nodes = vec![
            make_node("hub", "HubClass", "app.py"),
            make_node("stub_method", ".init()", "app.py"),
            make_node("stub_fn", "setup()", "app.py"),
        ];
        let edges = vec![
            make_edge("hub", "a", "calls", "EXTRACTED"),
            make_edge("hub", "b", "calls", "EXTRACTED"),
            make_edge("hub", "c", "calls", "EXTRACTED"),
            // stub_fn has degree 1 (only one edge)
            make_edge("hub", "stub_fn", "contains", "EXTRACTED"),
        ];
        let analyzer = Analyzer;
        let gods = analyzer.god_nodes(&nodes, &edges, 10);
        assert!(
            gods.iter().all(|n| n.id != "stub_method"),
            "method stub should be excluded"
        );
        assert!(
            gods.iter().all(|n| n.id != "stub_fn"),
            "isolated fn stub should be excluded"
        );
    }

    #[test]
    fn test_surprising_connections_basic() {
        let edges = vec![
            make_edge("a", "b", "imports", "EXTRACTED"),
            make_edge("c", "d", "calls", "INFERRED"),
        ];
        let analyzer = Analyzer;
        let surprising = analyzer.surprising_connections(&edges, false);
        assert_eq!(surprising.len(), 1);
        assert_eq!(surprising[0].relation, "calls");
    }

    #[test]
    fn test_surprising_cross_language_suppression() {
        let edges = vec![
            make_edge_with_file("a", "b", "imports", "INFERRED", "source.py"),
            make_edge_with_file("c", "d", "imports", "INFERRED", "data.json"),
        ];
        let analyzer = Analyzer;
        let filtered = analyzer.surprising_connections(&edges, true);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].source_file.as_deref(), Some("source.py"));
    }

    #[test]
    fn test_suggest_questions_basic() {
        let analyzer = Analyzer;
        let questions = analyzer.suggest_questions(&[], &[]);
        assert!(questions.len() >= 3);
    }

    #[test]
    fn test_suggest_questions_with_cycle() {
        let nodes = vec![make_node_id("a"), make_node_id("b")];
        let edges = vec![
            make_edge("a", "b", "imports", "EXTRACTED"),
            make_edge("b", "a", "imports", "EXTRACTED"),
        ];
        let analyzer = Analyzer;
        let questions = analyzer.suggest_questions(&nodes, &edges);
        let has_cycle_q = questions.iter().any(|q| q.contains("circular"));
        assert!(has_cycle_q);
    }

    #[test]
    fn test_suggest_questions_multi_lang() {
        let nodes = vec![
            make_node("a", "A", "main.py"),
            make_node("b", "B", "utils.ts"),
        ];
        let analyzer = Analyzer;
        let questions = analyzer.suggest_questions(&nodes, &[]);
        let has_lang_q = questions.iter().any(|q| q.contains("languages"));
        assert!(has_lang_q);
    }

    #[test]
    fn test_cohesion_complete() {
        let nodes = ["a", "b", "c"];
        let edges = vec![
            make_edge("a", "b", "connects", "EXTRACTED"),
            make_edge("a", "c", "connects", "EXTRACTED"),
            make_edge("b", "c", "connects", "EXTRACTED"),
        ];
        let analyzer = Analyzer;
        let score = analyzer.cohesion_score(
            &[],
            &edges,
            &nodes.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        );
        assert!((score - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_cohesion_empty() {
        let analyzer = Analyzer;
        let score = analyzer.cohesion_score(&[], &[], &[]);
        assert!((score - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_cohesion_single() {
        let analyzer = Analyzer;
        let score = analyzer.cohesion_score(&[], &[], &["a".to_string()]);
        assert!((score - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_pagerank_basic() {
        let edges = vec![
            make_edge("a", "b", "links", "EXTRACTED"),
            make_edge("b", "c", "links", "EXTRACTED"),
            make_edge("c", "a", "links", "EXTRACTED"),
        ];
        let analyzer = Analyzer;
        let ranks = analyzer.pagerank(&edges, 0.85, 100);
        assert!((ranks["a"] - ranks["b"]).abs() < 0.01);
        assert!((ranks["b"] - ranks["c"]).abs() < 0.01);
    }

    #[test]
    fn test_pagerank_skewed() {
        let edges = vec![
            make_edge("hub", "a", "links", "EXTRACTED"),
            make_edge("hub", "b", "links", "EXTRACTED"),
            make_edge("hub", "c", "links", "EXTRACTED"),
        ];
        let analyzer = Analyzer;
        let ranks = analyzer.pagerank(&edges, 0.85, 100);
        assert!(ranks["hub"] > 0.0);
        assert!(ranks["a"] > 0.0);
    }

    #[test]
    fn test_graph_diff_added() {
        let old = vec![make_edge("a", "b", "imports", "EXTRACTED")];
        let new = vec![
            make_edge("a", "b", "imports", "EXTRACTED"),
            make_edge("a", "c", "imports", "EXTRACTED"),
        ];
        let analyzer = Analyzer;
        let (added, removed) = analyzer.graph_diff(&old, &new);
        assert_eq!(added.len(), 1);
        assert_eq!(added[0].target, "c");
        assert!(removed.is_empty());
    }

    #[test]
    fn test_graph_diff_removed() {
        let old = vec![
            make_edge("a", "b", "imports", "EXTRACTED"),
            make_edge("a", "c", "imports", "EXTRACTED"),
        ];
        let new = vec![make_edge("a", "b", "imports", "EXTRACTED")];
        let analyzer = Analyzer;
        let (added, removed) = analyzer.graph_diff(&old, &new);
        assert!(added.is_empty());
        assert_eq!(removed.len(), 1);
        assert_eq!(removed[0].target, "c");
    }

    #[test]
    fn test_detect_cycles() {
        let nodes = vec![make_node_id("a"), make_node_id("b")];
        let edges = vec![
            make_edge("a", "b", "imports", "EXTRACTED"),
            make_edge("b", "a", "imports", "EXTRACTED"),
        ];
        let analyzer = Analyzer;
        assert!(analyzer.detect_cycles(&nodes, &edges));
    }

    #[test]
    fn test_detect_no_cycles() {
        let nodes = vec![make_node_id("a"), make_node_id("b"), make_node_id("c")];
        let edges = vec![
            make_edge("a", "b", "imports", "EXTRACTED"),
            make_edge("b", "c", "imports", "EXTRACTED"),
        ];
        let analyzer = Analyzer;
        assert!(!analyzer.detect_cycles(&nodes, &edges));
    }

    #[test]
    fn test_tarjan_scc_simple() {
        let edges = vec![
            make_edge("a", "b", "links", "EXTRACTED"),
            make_edge("b", "c", "links", "EXTRACTED"),
            make_edge("c", "a", "links", "EXTRACTED"),
        ];
        let analyzer = Analyzer;
        let sccs = analyzer.tarjan_scc(&edges);
        let large_scc = sccs.iter().find(|s| s.len() >= 3);
        assert!(large_scc.is_some(), "a->b->c->a should be one SCC");
    }

    #[test]
    fn test_tarjan_scc_no_cycle() {
        let edges = vec![
            make_edge("a", "b", "links", "EXTRACTED"),
            make_edge("b", "c", "links", "EXTRACTED"),
        ];
        let analyzer = Analyzer;
        let sccs = analyzer.tarjan_scc(&edges);
        for scc in &sccs {
            assert_eq!(scc.len(), 1, "DAG should only have singleton SCCs");
        }
    }

    #[test]
    fn test_tarjan_scc_empty() {
        let analyzer = Analyzer;
        let sccs = analyzer.tarjan_scc(&[]);
        assert!(sccs.is_empty());
    }

    #[test]
    fn test_bridge_edges_simple() {
        let nodes = vec![make_node_id("a"), make_node_id("b"), make_node_id("c")];
        let edges = vec![
            make_edge("a", "b", "connects", "EXTRACTED"),
            make_edge("b", "c", "connects", "EXTRACTED"),
        ];
        let analyzer = Analyzer;
        let bridges = analyzer.bridge_edges(&nodes, &edges);
        assert_eq!(bridges.len(), 2, "both edges are bridges");
    }

    #[test]
    fn test_bridge_edges_cycle() {
        let nodes = vec![make_node_id("a"), make_node_id("b"), make_node_id("c")];
        let edges = vec![
            make_edge("a", "b", "connects", "EXTRACTED"),
            make_edge("b", "c", "connects", "EXTRACTED"),
            make_edge("c", "a", "connects", "EXTRACTED"),
        ];
        let analyzer = Analyzer;
        let bridges = analyzer.bridge_edges(&nodes, &edges);
        assert_eq!(bridges.len(), 0, "cycle has no bridges");
    }

    #[test]
    fn test_bridge_edges_empty() {
        let analyzer = Analyzer;
        let bridges = analyzer.bridge_edges(&[], &[]);
        assert!(bridges.is_empty());
    }

    fn make_file_node(id: &str) -> Node {
        Node {
            id: id.to_string(),
            label: id.to_string(),
            file_type: "file".to_string(),
            source_file: format!("{id}.py"),
            source_location: None,
            community: None,
            rationale: None,
            docstring: None,
            metadata: HashMap::new(),
        }
    }

    #[test]
    fn test_risk_scores_bridge_factor() {
        // Linear chain a→b→c: both edges are bridges; b is endpoint of 2 bridges
        let mut nodes = vec![
            make_file_node("a"),
            make_file_node("b"),
            make_file_node("c"),
        ];
        let edges = vec![
            make_edge("a", "b", "imports", "EXTRACTED"),
            make_edge("b", "c", "imports", "EXTRACTED"),
        ];
        let churn_map: HashMap<String, usize> = [
            ("a".to_string(), 2),
            ("b".to_string(), 3),
            ("c".to_string(), 1),
        ]
        .into_iter()
        .collect();
        let analyzer = Analyzer;
        analyzer.apply_risk_scores(&mut nodes, &edges, &churn_map);

        // a: churn=2, deg=1, bridge_factor=1+1=2.0 → 4.0
        // b: churn=3, deg=2, bridge_factor=1+2=3.0 → 18.0
        // c: churn=1, deg=1, bridge_factor=1+1=2.0 → 2.0
        let risk_a: f64 = nodes[0].metadata["risk_score"].parse().unwrap();
        let risk_b: f64 = nodes[1].metadata["risk_score"].parse().unwrap();
        let risk_c: f64 = nodes[2].metadata["risk_score"].parse().unwrap();
        assert!((risk_a - 4.0).abs() < 0.01, "a risk={risk_a}");
        assert!((risk_b - 18.0).abs() < 0.01, "b risk={risk_b}");
        assert!((risk_c - 2.0).abs() < 0.01, "c risk={risk_c}");
    }

    #[test]
    fn test_risk_scores_no_bridges_in_cycle() {
        // Cycle a→b→c→a: no bridge edges → bridge_factor=1.0 for all
        let mut nodes = vec![
            make_file_node("a"),
            make_file_node("b"),
            make_file_node("c"),
        ];
        let edges = vec![
            make_edge("a", "b", "connects", "EXTRACTED"),
            make_edge("b", "c", "connects", "EXTRACTED"),
            make_edge("c", "a", "connects", "EXTRACTED"),
        ];
        let churn_map: HashMap<String, usize> = [
            ("a".to_string(), 2),
            ("b".to_string(), 2),
            ("c".to_string(), 2),
        ]
        .into_iter()
        .collect();
        let analyzer = Analyzer;
        analyzer.apply_risk_scores(&mut nodes, &edges, &churn_map);
        // Each node: churn=2, deg=2, bridge_factor=1.0 → 4.0
        for node in &nodes {
            let risk: f64 = node.metadata["risk_score"].parse().unwrap();
            assert!((risk - 4.0).abs() < 0.01, "{} risk={risk}", node.id);
        }
    }

    #[test]
    fn test_risk_scores_skips_non_file_nodes() {
        let mut nodes = vec![make_node_id("a")]; // file_type = "code"
        let churn_map: HashMap<String, usize> = [("a".to_string(), 5)].into_iter().collect();
        let analyzer = Analyzer;
        analyzer.apply_risk_scores(&mut nodes, &[], &churn_map);
        assert!(!nodes[0].metadata.contains_key("risk_score"));
    }

    #[test]
    fn test_risk_scores_rationale_contains_bridge_factor() {
        let mut nodes = vec![make_file_node("a"), make_file_node("b")];
        let edges = vec![make_edge("a", "b", "imports", "EXTRACTED")];
        let churn_map: HashMap<String, usize> = [("a".to_string(), 1), ("b".to_string(), 1)]
            .into_iter()
            .collect();
        let analyzer = Analyzer;
        analyzer.apply_risk_scores(&mut nodes, &edges, &churn_map);
        for node in &nodes {
            let rationale = node.rationale.as_deref().unwrap_or("");
            assert!(
                rationale.contains("bridge_factor="),
                "rationale missing bridge_factor: {rationale}"
            );
        }
    }

    #[test]
    fn test_topological_sort_simple() {
        let edges = vec![
            make_edge("a", "b", "depends", "EXTRACTED"),
            make_edge("b", "c", "depends", "EXTRACTED"),
        ];
        let analyzer = Analyzer;
        let sorted = analyzer.topological_sort(&edges);
        let pos = |id: &str| sorted.iter().position(|s| s == id).unwrap();
        assert!(pos("a") < pos("b"), "a should come before b");
        assert!(pos("b") < pos("c"), "b should come before c");
    }

    #[test]
    fn test_topological_sort_empty() {
        let analyzer = Analyzer;
        let sorted = analyzer.topological_sort(&[]);
        assert!(sorted.is_empty());
    }

    #[test]
    fn test_topological_sort_isolated() {
        let edges = vec![make_edge("a", "b", "depends", "EXTRACTED")];
        let analyzer = Analyzer;
        let sorted = analyzer.topological_sort(&edges);
        assert_eq!(sorted.len(), 2);
        assert_eq!(sorted[0], "a");
        assert_eq!(sorted[1], "b");
    }
}
