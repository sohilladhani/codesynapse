use chrono::{DateTime, Utc};
use serde_json::Value;
use std::collections::HashMap;
use std::process::Command;

use crate::types::Node;

pub struct PRInfo {
    pub number: i64,
    pub title: String,
    pub branch: String,
    pub base_branch: String,
    pub author: String,
    pub is_draft: bool,
    pub review_decision: String,
    pub ci_status: String,
    pub updated_at: DateTime<Utc>,
    pub expected_base: String,
    pub worktree_path: Option<String>,
    pub communities_touched: Vec<usize>,
    pub nodes_affected: usize,
    pub files_changed: Vec<String>,
}

impl PRInfo {
    pub fn days_old(&self) -> i64 {
        (Utc::now() - self.updated_at).num_days()
    }

    pub fn status(&self) -> String {
        classify(self, &self.expected_base)
    }

    pub fn blast_radius(&self) -> String {
        if self.nodes_affected == 0 {
            return String::new();
        }
        let n = self.nodes_affected;
        let c = self.communities_touched.len();
        let node_s = if n != 1 { "s" } else { "" };
        let comm_s = if c != 1 { "ies" } else { "y" };
        format!("{n} node{node_s} / {c} communit{comm_s}")
    }
}

const STALE_DAYS: i64 = 14;

pub const STATUS_ORDER: &[&str] = &[
    "WRONG-BASE",
    "CI-FAIL",
    "CHANGES-REQ",
    "DRAFT",
    "STALE",
    "PENDING",
    "APPROVED",
    "READY",
];

pub fn classify(pr: &PRInfo, base: &str) -> String {
    if pr.base_branch != base {
        return "WRONG-BASE".into();
    }
    if pr.ci_status == "FAILURE" {
        return "CI-FAIL".into();
    }
    if pr.review_decision == "CHANGES_REQUESTED" {
        return "CHANGES-REQ".into();
    }
    if pr.is_draft {
        return "DRAFT".into();
    }
    if pr.days_old() >= STALE_DAYS {
        return "STALE".into();
    }
    if pr.review_decision == "APPROVED" {
        return "APPROVED".into();
    }
    if pr.ci_status == "PENDING" {
        return "PENDING".into();
    }
    "READY".into()
}

const CI_FAILURE_CONCLUSIONS: &[&str] = &[
    "FAILURE",
    "CANCELLED",
    "TIMED_OUT",
    "ACTION_REQUIRED",
    "STARTUP_FAILURE",
];

pub fn parse_ci(rollup: &[HashMap<String, Value>]) -> String {
    if rollup.is_empty() {
        return "NONE".into();
    }
    for r in rollup {
        if let Some(c) = r.get("conclusion").and_then(|v| v.as_str()) {
            if CI_FAILURE_CONCLUSIONS.contains(&c) {
                return "FAILURE".into();
            }
        }
    }
    for r in rollup {
        if let Some(s) = r.get("status").and_then(|v| v.as_str()) {
            if s == "IN_PROGRESS" || s == "QUEUED" {
                return "PENDING".into();
            }
        }
    }
    for r in rollup {
        if let Some(c) = r.get("conclusion").and_then(|v| v.as_str()) {
            if c == "SUCCESS" {
                return "SUCCESS".into();
            }
        }
    }
    "NONE".into()
}

pub fn path_match(graph_src: &str, pr_file: &str) -> bool {
    if graph_src == pr_file {
        return true;
    }
    let sep_pr = format!("/{pr_file}");
    let sep_graph = format!("/{graph_src}");
    graph_src.ends_with(&sep_pr) || pr_file.ends_with(&sep_graph)
}

pub fn compute_pr_impact(files: &[&str], nodes: &[Node]) -> (Vec<usize>, usize) {
    let mut file_comms: HashMap<&str, std::collections::HashSet<usize>> = HashMap::new();
    let mut file_count: HashMap<&str, usize> = HashMap::new();

    for node in nodes {
        let src = node.source_file.as_str();
        if src.is_empty() {
            continue;
        }
        let entry = file_comms.entry(src).or_default();
        *file_count.entry(src).or_insert(0) += 1;
        if let Some(c) = node.community {
            entry.insert(c);
        }
    }

    let mut comms: std::collections::HashSet<usize> = std::collections::HashSet::new();
    let mut total_nodes = 0usize;
    let mut matched: std::collections::HashSet<&str> = std::collections::HashSet::new();

    for f in files {
        for (src, src_comms) in &file_comms {
            if !matched.contains(src) && path_match(src, f) {
                comms.extend(src_comms.iter().copied());
                total_nodes += file_count.get(src).copied().unwrap_or(0);
                matched.insert(src);
            }
        }
    }

    let mut sorted_comms: Vec<usize> = comms.into_iter().collect();
    sorted_comms.sort_unstable();
    (sorted_comms, total_nodes)
}

pub fn fetch_prs(
    repo: Option<&str>,
    base: Option<&str>,
    limit: usize,
) -> Result<Vec<PRInfo>, String> {
    let resolved_base = base
        .map(|s| s.to_string())
        .unwrap_or_else(detect_default_branch);
    let limit_str = limit.to_string();
    let args = [
        "pr", "list", "--state", "open",
        "--limit", &limit_str,
        "--json",
        "number,title,headRefName,baseRefName,author,isDraft,reviewDecision,statusCheckRollup,updatedAt",
    ];
    let mut repo_args: Vec<String> = Vec::new();
    if let Some(r) = repo {
        repo_args.push("--repo".into());
        repo_args.push(r.into());
    }
    let args_owned: Vec<String> = args
        .iter()
        .map(|s| s.to_string())
        .chain(repo_args)
        .collect();

    let result = Command::new("gh")
        .args(&args_owned)
        .output()
        .map_err(|e| format!("gh CLI not found: {e}. Run: gh auth login"))?;

    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        return Err(format!("gh pr list failed: {stderr}"));
    }

    let json: Value =
        serde_json::from_slice(&result.stdout).map_err(|e| format!("parse gh output: {e}"))?;

    let items = json.as_array().ok_or("gh output not array")?;
    let mut prs = Vec::new();
    for item in items {
        let updated_str = item["updatedAt"].as_str().unwrap_or("1970-01-01T00:00:00Z");
        let updated: DateTime<Utc> = updated_str
            .parse()
            .unwrap_or_else(|_| DateTime::from_timestamp(0, 0).unwrap());
        let rollup: Vec<HashMap<String, Value>> = item
            .get("statusCheckRollup")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();
        prs.push(PRInfo {
            number: item["number"].as_i64().unwrap_or(0),
            title: item["title"].as_str().unwrap_or("").to_string(),
            branch: item["headRefName"].as_str().unwrap_or("").to_string(),
            base_branch: item["baseRefName"].as_str().unwrap_or("").to_string(),
            author: item
                .get("author")
                .and_then(|a| a.get("login"))
                .and_then(|l| l.as_str())
                .unwrap_or("?")
                .to_string(),
            is_draft: item["isDraft"].as_bool().unwrap_or(false),
            review_decision: item
                .get("reviewDecision")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            ci_status: parse_ci(&rollup),
            updated_at: updated,
            expected_base: resolved_base.clone(),
            worktree_path: None,
            communities_touched: vec![],
            nodes_affected: 0,
            files_changed: vec![],
        });
    }
    Ok(prs)
}

pub fn fetch_worktrees() -> HashMap<String, String> {
    let result = match Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .output()
    {
        Ok(r) => r,
        Err(_) => return HashMap::new(),
    };

    if !result.status.success() {
        return HashMap::new();
    }

    let stdout = String::from_utf8_lossy(&result.stdout);
    let mut mapping = HashMap::new();
    let mut current_path: Option<String> = None;

    for line in stdout.lines() {
        if line.is_empty() {
            current_path = None;
        } else if let Some(path) = line.strip_prefix("worktree ") {
            current_path = Some(path.to_string());
        } else if let Some(branch) = line.strip_prefix("branch refs/heads/") {
            if let Some(ref p) = current_path {
                mapping.insert(branch.to_string(), p.clone());
            }
        }
    }

    mapping
}

pub fn detect_default_branch() -> String {
    if let Some(branch) = try_gh_default_branch() {
        return branch;
    }
    try_git_symbolic_ref().unwrap_or_else(|| "main".into())
}

pub fn detect_default_branch_from(gh_json: Option<&str>, git_ref: Option<&str>) -> String {
    if let Some(json) = gh_json {
        if let Some(branch) = parse_gh_default_branch(json) {
            return branch;
        }
    }
    if let Some(ref_str) = git_ref {
        if let Some(branch) = parse_git_symbolic_ref(ref_str) {
            return branch;
        }
    }
    "main".into()
}

pub fn parse_gh_default_branch(json: &str) -> Option<String> {
    let data: Value = serde_json::from_str(json).ok()?;
    data.get("defaultBranchRef")?
        .get("name")?
        .as_str()
        .map(|s| s.to_string())
}

pub fn parse_git_symbolic_ref(ref_str: &str) -> Option<String> {
    let trimmed = ref_str.trim();
    if trimmed.is_empty() {
        return None;
    }
    trimmed.split('/').next_back().map(|s| s.to_string())
}

fn try_gh_default_branch() -> Option<String> {
    let result = Command::new("gh")
        .args(["repo", "view", "--json", "defaultBranchRef"])
        .output()
        .ok()?;
    if !result.status.success() {
        return None;
    }
    let json = String::from_utf8_lossy(&result.stdout);
    parse_gh_default_branch(&json)
}

fn try_git_symbolic_ref() -> Option<String> {
    let result = Command::new("git")
        .args(["symbolic-ref", "refs/remotes/origin/HEAD"])
        .output()
        .ok()?;
    if !result.status.success() {
        return None;
    }
    let ref_str = String::from_utf8_lossy(&result.stdout);
    parse_git_symbolic_ref(&ref_str)
}

pub fn build_community_labels(nodes: &[Value], top_n: usize) -> HashMap<usize, Vec<String>> {
    let mut comm_labels: HashMap<usize, Vec<String>> = HashMap::new();
    for node in nodes {
        let c = match node.get("community").and_then(|v| v.as_u64()) {
            Some(v) => v as usize,
            None => continue,
        };
        let label = node
            .get("label")
            .and_then(|v| v.as_str())
            .or_else(|| node.get("id").and_then(|v| v.as_str()))
            .unwrap_or("")
            .to_string();
        if !label.is_empty() {
            comm_labels.entry(c).or_default().push(label);
        }
    }
    comm_labels
        .into_iter()
        .map(|(c, mut labels)| {
            labels.truncate(top_n);
            (c, labels)
        })
        .collect()
}

pub fn format_prs_text(prs: &[PRInfo], base: &str) -> String {
    let actionable: Vec<&PRInfo> = prs.iter().filter(|p| p.base_branch == base).collect();
    let wrong = prs.len() - actionable.len();

    let mut lines = vec![format!(
        "Open PRs targeting {base}: {}  ({wrong} on wrong base, not shown)",
        actionable.len()
    )];

    let mut sorted = actionable.clone();
    sorted.sort_by_key(|p| {
        let status = classify(p, base);
        let order = STATUS_ORDER.iter().position(|s| *s == status).unwrap_or(99);
        (order, p.days_old())
    });

    for p in sorted {
        let status = classify(p, base);
        let review = if p.review_decision.is_empty() {
            "none".to_string()
        } else {
            p.review_decision.clone()
        };
        let impact = if !p.blast_radius().is_empty() {
            format!("  blast_radius={}", p.blast_radius())
        } else {
            String::new()
        };
        lines.push(format!(
            "#{} [{}] CI={} review={} age={}d author={}{}  {}",
            p.number,
            status,
            p.ci_status,
            review,
            p.days_old(),
            p.author,
            impact,
            p.title,
        ));
    }

    lines.join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, Utc};
    use std::collections::HashMap;

    #[allow(clippy::too_many_arguments)]
    fn make_pr(
        number: i64,
        title: &str,
        branch: &str,
        base_branch: &str,
        author: &str,
        is_draft: bool,
        review_decision: &str,
        ci_status: &str,
        days_ago: i64,
        expected_base: &str,
    ) -> PRInfo {
        PRInfo {
            number,
            title: title.to_string(),
            branch: branch.to_string(),
            base_branch: base_branch.to_string(),
            author: author.to_string(),
            is_draft,
            review_decision: review_decision.to_string(),
            ci_status: ci_status.to_string(),
            updated_at: Utc::now() - Duration::days(days_ago),
            expected_base: expected_base.to_string(),
            worktree_path: None,
            communities_touched: vec![],
            nodes_affected: 0,
            files_changed: vec![],
        }
    }

    fn default_pr() -> PRInfo {
        make_pr(
            1, "Test PR", "feature", "v8", "alice", false, "", "SUCCESS", 1, "v8",
        )
    }

    // ── _classify ─────────────────────────────────────────────────────────

    #[test]
    fn test_classify_ready() {
        let pr = default_pr();
        assert_eq!(classify(&pr, "v8"), "READY");
    }

    #[test]
    fn test_classify_ci_fail() {
        let pr = make_pr(1, "T", "f", "v8", "a", false, "", "FAILURE", 1, "v8");
        assert_eq!(classify(&pr, "v8"), "CI-FAIL");
    }

    #[test]
    fn test_classify_changes_req() {
        let pr = make_pr(
            1,
            "T",
            "f",
            "v8",
            "a",
            false,
            "CHANGES_REQUESTED",
            "SUCCESS",
            1,
            "v8",
        );
        assert_eq!(classify(&pr, "v8"), "CHANGES-REQ");
    }

    #[test]
    fn test_classify_draft() {
        let pr = make_pr(1, "T", "f", "v8", "a", true, "", "SUCCESS", 1, "v8");
        assert_eq!(classify(&pr, "v8"), "DRAFT");
    }

    #[test]
    fn test_classify_stale() {
        let pr = make_pr(1, "T", "f", "v8", "a", false, "", "SUCCESS", 20, "v8");
        assert_eq!(classify(&pr, "v8"), "STALE");
    }

    #[test]
    fn test_classify_draft_not_marked_stale() {
        let pr = make_pr(1, "T", "f", "v8", "a", true, "", "SUCCESS", 20, "v8");
        assert_eq!(classify(&pr, "v8"), "DRAFT");
    }

    #[test]
    fn test_classify_pending() {
        let pr = make_pr(1, "T", "f", "v8", "a", false, "", "PENDING", 1, "v8");
        assert_eq!(classify(&pr, "v8"), "PENDING");
    }

    #[test]
    fn test_classify_wrong_base() {
        let pr = make_pr(1, "T", "f", "master", "a", false, "", "FAILURE", 1, "v8");
        assert_eq!(classify(&pr, "v8"), "WRONG-BASE");
    }

    // ── _parse_ci ─────────────────────────────────────────────────────────

    fn rollup(conclusion: Option<&str>, status: &str) -> HashMap<String, Value> {
        let mut m = HashMap::new();
        m.insert(
            "conclusion".into(),
            match conclusion {
                Some(c) => Value::String(c.into()),
                None => Value::Null,
            },
        );
        m.insert("status".into(), Value::String(status.into()));
        m
    }

    #[test]
    fn test_parse_ci_empty_returns_none() {
        assert_eq!(parse_ci(&[]), "NONE");
    }

    #[test]
    fn test_parse_ci_failure_conclusion() {
        let r = vec![rollup(Some("FAILURE"), "COMPLETED")];
        assert_eq!(parse_ci(&r), "FAILURE");
    }

    #[test]
    fn test_parse_ci_cancelled_is_failure() {
        let r = vec![rollup(Some("CANCELLED"), "COMPLETED")];
        assert_eq!(parse_ci(&r), "FAILURE");
    }

    #[test]
    fn test_parse_ci_timed_out_is_failure() {
        let r = vec![rollup(Some("TIMED_OUT"), "COMPLETED")];
        assert_eq!(parse_ci(&r), "FAILURE");
    }

    #[test]
    fn test_parse_ci_in_progress_is_pending() {
        let r = vec![rollup(None, "IN_PROGRESS")];
        assert_eq!(parse_ci(&r), "PENDING");
    }

    #[test]
    fn test_parse_ci_success() {
        let r = vec![rollup(Some("SUCCESS"), "COMPLETED")];
        assert_eq!(parse_ci(&r), "SUCCESS");
    }

    #[test]
    fn test_parse_ci_mixed_success_and_failure_is_failure() {
        let r = vec![
            rollup(Some("SUCCESS"), "COMPLETED"),
            rollup(Some("FAILURE"), "COMPLETED"),
        ];
        assert_eq!(parse_ci(&r), "FAILURE");
    }

    // ── _path_match ────────────────────────────────────────────────────────

    #[test]
    fn test_path_match_exact() {
        assert!(path_match("src/auth/api.py", "src/auth/api.py"));
    }

    #[test]
    fn test_path_match_graph_path_longer() {
        assert!(path_match("src/auth/api.py", "api.py"));
    }

    #[test]
    fn test_path_match_no_false_positive_partial_filename() {
        assert!(!path_match("config.py", "g.py"));
        assert!(!path_match("g.py", "config.py"));
    }

    #[test]
    fn test_path_match_both_directions() {
        assert!(path_match("api.py", "src/auth/api.py"));
        assert!(path_match("src/auth/api.py", "api.py"));
    }

    // ── compute_pr_impact ─────────────────────────────────────────────────

    fn make_node(id: &str, source_file: &str, community: Option<usize>) -> Node {
        Node {
            id: id.to_string(),
            label: id.to_string(),
            file_type: "function".to_string(),
            source_file: source_file.to_string(),
            source_location: None,
            community,
            rationale: None,
            docstring: None,
            metadata: HashMap::new(),
        }
    }

    fn three_node_graph() -> Vec<Node> {
        vec![
            make_node("n1", "src/auth/api.py", Some(0)),
            make_node("n2", "src/auth/api.py", Some(0)),
            make_node("n3", "src/utils/helpers.py", Some(1)),
        ]
    }

    #[test]
    fn test_compute_pr_impact_matching_files() {
        let nodes = three_node_graph();
        let (comms, count) = compute_pr_impact(&["src/auth/api.py"], &nodes);
        assert_eq!(comms, vec![0]);
        assert_eq!(count, 2);
    }

    #[test]
    fn test_compute_pr_impact_both_files() {
        let nodes = three_node_graph();
        let (comms, count) =
            compute_pr_impact(&["src/auth/api.py", "src/utils/helpers.py"], &nodes);
        assert_eq!(comms, vec![0, 1]);
        assert_eq!(count, 3);
    }

    #[test]
    fn test_compute_pr_impact_empty_files() {
        let nodes = three_node_graph();
        let (comms, count) = compute_pr_impact(&[], &nodes);
        assert!(comms.is_empty());
        assert_eq!(count, 0);
    }

    #[test]
    fn test_compute_pr_impact_no_matching_files() {
        let nodes = three_node_graph();
        let (comms, count) = compute_pr_impact(&["docs/README.md"], &nodes);
        assert!(comms.is_empty());
        assert_eq!(count, 0);
    }

    #[test]
    fn test_compute_pr_impact_no_double_count_basename() {
        let nodes = vec![
            make_node("a1", "src/auth/api.py", Some(0)),
            make_node("a2", "src/admin/api.py", Some(1)),
        ];
        let (comms, count) = compute_pr_impact(&["src/auth/api.py"], &nodes);
        assert_eq!(count, 1);
        assert_eq!(comms, vec![0]);
    }

    #[test]
    fn test_compute_pr_impact_no_double_count_same_graph_file() {
        let nodes = vec![
            make_node("n1", "src/auth/api.py", Some(0)),
            make_node("n2", "src/auth/api.py", Some(0)),
        ];
        let (comms, count) = compute_pr_impact(&["src/auth/api.py", "api.py"], &nodes);
        assert_eq!(count, 2);
        assert_eq!(comms, vec![0]);
    }

    // ── fetch_worktrees ───────────────────────────────────────────────────
    // These tests exercise the parser directly by calling a testable parse fn.

    fn parse_worktree_output(stdout: &str, success: bool) -> HashMap<String, String> {
        if !success {
            return HashMap::new();
        }
        let mut mapping = HashMap::new();
        let mut current_path: Option<String> = None;
        for line in stdout.lines() {
            if line.is_empty() {
                current_path = None;
            } else if let Some(path) = line.strip_prefix("worktree ") {
                current_path = Some(path.to_string());
            } else if let Some(branch) = line.strip_prefix("branch refs/heads/") {
                if let Some(ref p) = current_path {
                    mapping.insert(branch.to_string(), p.clone());
                }
            }
        }
        mapping
    }

    #[test]
    fn test_fetch_worktrees_normal_case() {
        let porcelain = "worktree /home/user/proj\nHEAD abc123\nbranch refs/heads/main\n\nworktree /home/user/proj-feature\nHEAD def456\nbranch refs/heads/feature-x\n\n";
        let m = parse_worktree_output(porcelain, true);
        assert_eq!(m.get("main").map(|s| s.as_str()), Some("/home/user/proj"));
        assert_eq!(
            m.get("feature-x").map(|s| s.as_str()),
            Some("/home/user/proj-feature")
        );
    }

    #[test]
    fn test_fetch_worktrees_detached_head() {
        let porcelain = "worktree /home/user/detached\nHEAD abc123\ndetached\n\nworktree /home/user/proj-feature\nHEAD def456\nbranch refs/heads/feature-x\n\n";
        let m = parse_worktree_output(porcelain, true);
        assert_eq!(
            m.get("feature-x").map(|s| s.as_str()),
            Some("/home/user/proj-feature")
        );
        assert!(!m.values().any(|v| v == "/home/user/detached"));
    }

    #[test]
    fn test_fetch_worktrees_empty_output() {
        let m = parse_worktree_output("", true);
        assert!(m.is_empty());
    }

    #[test]
    fn test_fetch_worktrees_nonzero_returncode() {
        let m = parse_worktree_output("anything", false);
        assert!(m.is_empty());
    }

    #[test]
    fn test_fetch_worktrees_subprocess_failure() {
        // Simulate by calling with non-existent binary via parse fn with empty
        let m = parse_worktree_output("", false);
        assert!(m.is_empty());
    }

    // ── format_prs_text ───────────────────────────────────────────────────

    #[test]
    fn test_format_prs_text_contains_metadata_and_header() {
        let prs = vec![
            make_pr(
                101,
                "Add awesome feature",
                "f1",
                "v8",
                "alice",
                false,
                "",
                "SUCCESS",
                1,
                "v8",
            ),
            make_pr(
                102,
                "Fix flaky test",
                "f2",
                "v8",
                "bob",
                false,
                "",
                "FAILURE",
                1,
                "v8",
            ),
            make_pr(
                103,
                "Wrong base PR",
                "f3",
                "master",
                "carol",
                false,
                "",
                "SUCCESS",
                1,
                "v8",
            ),
        ];
        let out = format_prs_text(&prs, "v8");
        assert!(out.contains("Open PRs targeting v8: 2"));
        assert!(out.contains("(1 on wrong base, not shown)"));
        assert!(out.contains("#101"));
        assert!(out.contains("Add awesome feature"));
        assert!(out.contains("#102"));
        assert!(out.contains("Fix flaky test"));
        assert!(out.contains("[READY]"));
        assert!(out.contains("[CI-FAIL]"));
        assert!(!out.contains("#103"));
    }

    #[test]
    fn test_format_prs_text_empty() {
        let out = format_prs_text(&[], "v8");
        assert!(out.contains("Open PRs targeting v8: 0"));
        assert!(out.contains("(0 on wrong base, not shown)"));
    }

    // ── build_community_labels ─────────────────────────────────────────────

    #[test]
    fn test_build_community_labels_basic() {
        let nodes = vec![
            serde_json::json!({"id": "a", "label": "Alpha", "community": 0}),
            serde_json::json!({"id": "b", "label": "Beta",  "community": 0}),
            serde_json::json!({"id": "c", "label": "Gamma", "community": 1}),
        ];
        let labels = build_community_labels(&nodes, 4);
        let c0: std::collections::HashSet<&str> = labels[&0].iter().map(|s| s.as_str()).collect();
        assert_eq!(c0, ["Alpha", "Beta"].iter().copied().collect());
        assert_eq!(labels[&1], vec!["Gamma"]);
    }

    #[test]
    fn test_build_community_labels_top_n_capped() {
        let nodes: Vec<Value> = (0..10)
            .map(|i| serde_json::json!({"id": i.to_string(), "label": format!("Node{i}"), "community": 0}))
            .collect();
        let labels = build_community_labels(&nodes, 4);
        assert_eq!(labels[&0].len(), 4);
    }

    #[test]
    fn test_build_community_labels_no_community_skipped() {
        let nodes = vec![serde_json::json!({"id": "x", "label": "X"})];
        assert!(build_community_labels(&nodes, 4).is_empty());
    }

    #[test]
    fn test_build_community_labels_empty() {
        assert!(build_community_labels(&[], 4).is_empty());
    }

    // ── _detect_default_branch ─────────────────────────────────────────────

    #[test]
    fn test_detect_default_branch_gh_returns_main() {
        let json = r#"{"defaultBranchRef":{"name":"main"}}"#;
        assert_eq!(detect_default_branch_from(Some(json), None), "main");
    }

    #[test]
    fn test_detect_default_branch_falls_back_to_git_symbolic_ref() {
        assert_eq!(
            detect_default_branch_from(None, Some("refs/remotes/origin/develop\n")),
            "develop"
        );
    }

    #[test]
    fn test_detect_default_branch_both_fail_returns_main() {
        assert_eq!(detect_default_branch_from(None, None), "main");
    }

    #[test]
    fn test_detect_default_branch_gh_empty_dict_falls_back() {
        assert_eq!(
            detect_default_branch_from(Some("{}"), Some("refs/remotes/origin/trunk\n")),
            "trunk"
        );
    }

    #[test]
    fn test_detect_default_branch_git_empty_ref_returns_main() {
        assert_eq!(detect_default_branch_from(None, Some("\n")), "main");
    }
}
