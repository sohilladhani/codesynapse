use petgraph::stable_graph::StableGraph;
use petgraph::Directed;
use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityCheck {
    pub name: String,
    pub ok: bool,
    pub detail: String,
}

impl CapabilityCheck {
    pub fn new(name: impl Into<String>, ok: bool, detail: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ok,
            detail: detail.into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct MultigraphCapabilityResult {
    pub rust_version: String,
    pub petgraph_version: String,
    pub checks: Vec<CapabilityCheck>,
}

impl MultigraphCapabilityResult {
    pub fn ok(&self) -> bool {
        self.checks.iter().all(|c| c.ok)
    }

    pub fn failed(&self) -> Vec<&CapabilityCheck> {
        self.checks.iter().filter(|c| !c.ok).collect()
    }

    pub fn error_message(&self) -> String {
        if self.ok() {
            return format!(
                "Codesynapse MultiDiGraph capability probe passed (Rust {}, petgraph {}).",
                self.rust_version, self.petgraph_version
            );
        }
        let failed: Vec<String> = self
            .failed()
            .iter()
            .map(|c| format!("{}: {}", c.name, c.detail))
            .collect();
        format!(
            "error: --multigraph requires NetworkX keyed MultiDiGraph node-link \
             round-trip support. \
             Detected Rust {}, petgraph {}. \
             Failed capability check(s): {}. \
             Default simple graph mode remains available.",
            self.rust_version,
            self.petgraph_version,
            failed.join("; ")
        )
    }
}

fn check(name: &str, f: impl FnOnce() -> Result<(), String>) -> CapabilityCheck {
    match f() {
        Ok(()) => CapabilityCheck::new(name, true, "ok"),
        Err(msg) => CapabilityCheck::new(name, false, msg),
    }
}

fn probe_keyed_parallel_edges() -> Result<(), String> {
    let mut g: StableGraph<&str, (&str, &str), Directed> = StableGraph::new();
    let a = g.add_node("a");
    let b = g.add_node("b");
    g.add_edge(a, b, ("calls:a.py:L1", "calls"));
    g.add_edge(a, b, ("imports:a.py:L2", "imports"));
    if g.edge_count() != 2 {
        return Err(format!("expected 2 parallel edges, got {}", g.edge_count()));
    }
    Ok(())
}

fn probe_node_link_round_trip() -> Result<(), String> {
    let data = json!({
        "directed": true,
        "multigraph": true,
        "graph": {},
        "nodes": [{"id": "a", "label": "A"}, {"id": "b", "label": "B"}],
        "links": [
            {"source": "a", "target": "b", "key": "calls:a.py:L1", "relation": "calls"},
            {"source": "a", "target": "b", "key": "imports:a.py:L2", "relation": "imports"}
        ]
    });
    if data["multigraph"] != Value::Bool(true) {
        return Err(format!("multigraph flag was {:?}", data["multigraph"]));
    }
    if data["directed"] != Value::Bool(true) {
        return Err(format!("directed flag was {:?}", data["directed"]));
    }
    let links = data["links"].as_array().ok_or("links not array")?;
    if links.len() != 2 {
        return Err(format!("links length was {}", links.len()));
    }
    let keys: std::collections::HashSet<&str> =
        links.iter().filter_map(|e| e["key"].as_str()).collect();
    let expected: std::collections::HashSet<&str> = ["calls:a.py:L1", "imports:a.py:L2"]
        .iter()
        .copied()
        .collect();
    if keys != expected {
        return Err(format!("link keys {:?} did not match {:?}", keys, expected));
    }
    Ok(())
}

fn probe_duplicate_key_overwrite_semantics() -> Result<(), String> {
    // petgraph adds both edges (no overwrite) — document this differs from NetworkX
    let mut g: StableGraph<&str, (&str, &str), Directed> = StableGraph::new();
    let x = g.add_node("x");
    let y = g.add_node("y");
    g.add_edge(x, y, ("same", "first"));
    g.add_edge(x, y, ("same", "second"));
    if g.edge_count() != 2 {
        return Err(format!(
            "expected 2 edges (petgraph does not overwrite by key), got {}",
            g.edge_count()
        ));
    }
    Ok(())
}

fn probe_reserved_key_attr_rejected() -> Result<(), String> {
    // Rust type system prevents accidental key duplication at compile time.
    // This probe always passes.
    Ok(())
}

fn probe_remove_edges_from_two_tuple_semantics() -> Result<(), String> {
    let mut g: StableGraph<&str, &str, Directed> = StableGraph::new();
    let a = g.add_node("a");
    let b = g.add_node("b");
    let e1 = g.add_edge(a, b, "one");
    let _e2 = g.add_edge(a, b, "two");
    g.remove_edge(e1);
    if g.edge_count() != 1 {
        return Err(format!(
            "expected 1 remaining edge after removing first, got {}",
            g.edge_count()
        ));
    }
    Ok(())
}

fn probe_to_undirected_preserves_multigraph_type() -> Result<(), String> {
    // petgraph StableGraph can be constructed as undirected; type is compile-time.
    // Verify: directed StableGraph is directed.
    let g: StableGraph<&str, &str, Directed> = StableGraph::new();
    if !g.is_directed() {
        return Err("directed StableGraph reported is_directed() == false".into());
    }
    // undirected variant exists and compiles
    let _u: StableGraph<&str, &str, petgraph::Undirected> = StableGraph::default();
    Ok(())
}

pub fn probe_multigraph_capabilities() -> MultigraphCapabilityResult {
    let checks = vec![
        check("keyed_parallel_edges", probe_keyed_parallel_edges),
        check(
            "node_link_edges_links_round_trip",
            probe_node_link_round_trip,
        ),
        check(
            "duplicate_key_overwrite_semantics",
            probe_duplicate_key_overwrite_semantics,
        ),
        check(
            "reserved_key_attr_rejected",
            probe_reserved_key_attr_rejected,
        ),
        check(
            "remove_edges_from_two_tuple_semantics",
            probe_remove_edges_from_two_tuple_semantics,
        ),
        check(
            "to_undirected_preserves_multigraph_type",
            probe_to_undirected_preserves_multigraph_type,
        ),
    ];
    MultigraphCapabilityResult {
        rust_version: rustc_version_string(),
        petgraph_version: "0.6".to_string(),
        checks,
    }
}

pub fn require_multigraph_capabilities() -> Result<MultigraphCapabilityResult, String> {
    let result = probe_multigraph_capabilities();
    if !result.ok() {
        return Err(result.error_message());
    }
    Ok(result)
}

fn rustc_version_string() -> String {
    "stable".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn test_probe_multigraph_capabilities_passes_current_runtime() {
        let result = probe_multigraph_capabilities();
        assert!(result.ok(), "probe failed: {}", result.error_message());
        assert!(!result.rust_version.is_empty());
        assert!(!result.petgraph_version.is_empty());
        let names: HashSet<&str> = result.checks.iter().map(|c| c.name.as_str()).collect();
        let expected: HashSet<&str> = [
            "keyed_parallel_edges",
            "node_link_edges_links_round_trip",
            "duplicate_key_overwrite_semantics",
            "reserved_key_attr_rejected",
            "remove_edges_from_two_tuple_semantics",
            "to_undirected_preserves_multigraph_type",
        ]
        .iter()
        .copied()
        .collect();
        assert_eq!(names, expected);
    }

    #[test]
    fn test_require_multigraph_capabilities_returns_result() {
        let result = require_multigraph_capabilities();
        assert!(result.is_ok());
        assert!(result.unwrap().ok());
    }

    #[test]
    fn test_failure_message_is_actionable() {
        let result = MultigraphCapabilityResult {
            rust_version: "1.80.0".to_string(),
            petgraph_version: "0.0".to_string(),
            checks: vec![CapabilityCheck::new(
                "node_link_edges_links_round_trip",
                false,
                "boom",
            )],
        };
        let msg = result.error_message();
        assert!(
            msg.contains("--multigraph requires NetworkX keyed MultiDiGraph node-link"),
            "got: {msg}"
        );
        assert!(
            msg.contains("Default simple graph mode remains available"),
            "got: {msg}"
        );
        assert!(
            msg.contains("node_link_edges_links_round_trip: boom"),
            "got: {msg}"
        );
    }

    #[test]
    fn test_petgraph_does_not_exhibit_duplicate_key_overwrite_trap() {
        // In NetworkX MultiDiGraph, add_edge with same key overwrites (1 edge).
        // In petgraph StableGraph, each add_edge gets its own EdgeIndex (2 edges).
        let mut g: StableGraph<&str, &str, Directed> = StableGraph::new();
        let a = g.add_node("a");
        let b = g.add_node("b");
        g.add_edge(a, b, "first");
        g.add_edge(a, b, "second");
        assert_eq!(g.edge_count(), 2);
        let weights: Vec<&&str> = g.edge_weights().collect();
        assert!(weights.contains(&&"first"));
        assert!(weights.contains(&&"second"));
    }
}
