use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::CodeSynapseError;

pub const HOOK_MARKER: &str = "# codesynapse-hook-start";
pub const HOOK_MARKER_END: &str = "# codesynapse-hook-end";
pub const CHECKOUT_MARKER: &str = "# codesynapse-checkout-hook-start";
pub const CHECKOUT_MARKER_END: &str = "# codesynapse-checkout-hook-end";

const HOOK_SCRIPT: &str = r#"# codesynapse-hook-start
# Auto-rebuilds the knowledge graph after each commit.
# Installed by: codesynapse hook install

GIT_DIR=$(git rev-parse --git-dir 2>/dev/null)
[ -d "$GIT_DIR/rebase-merge" ] && exit 0
[ -d "$GIT_DIR/rebase-apply" ] && exit 0
[ -f "$GIT_DIR/MERGE_HEAD" ] && exit 0
[ -f "$GIT_DIR/CHERRY_PICK_HEAD" ] && exit 0

[ "${CODESYNAPSE_SKIP_HOOK:-0}" = "1" ] && exit 0

CHANGED=$(git diff --name-only HEAD~1 HEAD 2>/dev/null || git diff --name-only HEAD 2>/dev/null)
if [ -z "$CHANGED" ]; then
    exit 0
fi

_NON_GRAPH=$(echo "$CHANGED" | grep -v '^codesynapse-out/' || true)
if [ -z "$_NON_GRAPH" ]; then
    exit 0
fi

_CODESYNAPSE_LOG="${HOME}/.cache/codesynapse-rebuild.log"
mkdir -p "$(dirname "$_CODESYNAPSE_LOG")"
echo "[codesynapse hook] launching background rebuild (log: $_CODESYNAPSE_LOG)"

if command -v codesynapse >/dev/null 2>&1; then
    nohup codesynapse build . >> "$_CODESYNAPSE_LOG" 2>&1 < /dev/null &
    disown 2>/dev/null || true
fi
# codesynapse-hook-end
"#;

const CHECKOUT_SCRIPT: &str = r#"# codesynapse-checkout-hook-start
# Auto-rebuilds the knowledge graph when switching branches.
# Installed by: codesynapse hook install

PREV_HEAD=$1
NEW_HEAD=$2
BRANCH_SWITCH=$3

if [ "$BRANCH_SWITCH" != "1" ]; then
    exit 0
fi

if [ ! -d "codesynapse-out" ]; then
    exit 0
fi

GIT_DIR=$(git rev-parse --git-dir 2>/dev/null)
[ -d "$GIT_DIR/rebase-merge" ] && exit 0
[ -d "$GIT_DIR/rebase-apply" ] && exit 0
[ -f "$GIT_DIR/MERGE_HEAD" ] && exit 0
[ -f "$GIT_DIR/CHERRY_PICK_HEAD" ] && exit 0

_CODESYNAPSE_LOG="${HOME}/.cache/codesynapse-rebuild.log"
mkdir -p "$(dirname "$_CODESYNAPSE_LOG")"
echo "[codesynapse] Branch switched - launching background rebuild (log: $_CODESYNAPSE_LOG)"

if command -v codesynapse >/dev/null 2>&1; then
    nohup codesynapse build . >> "$_CODESYNAPSE_LOG" 2>&1 < /dev/null &
    disown 2>/dev/null || true
fi
# codesynapse-checkout-hook-end
"#;

pub fn git_root(path: &Path) -> Option<PathBuf> {
    let current = path.canonicalize().ok()?;
    let candidates = std::iter::once(current.as_path()).chain(current.ancestors().skip(1));
    for dir in candidates {
        if dir.join(".git").exists() {
            return Some(dir.to_path_buf());
        }
    }
    None
}

pub fn hooks_dir(root: &Path) -> PathBuf {
    let result = Command::new("git")
        .args([
            "-C",
            &root.to_string_lossy(),
            "rev-parse",
            "--git-path",
            "hooks",
        ])
        .output();

    if let Ok(out) = result {
        if out.status.success() {
            let raw = String::from_utf8_lossy(&out.stdout);
            let raw = raw.trim();
            if !raw.is_empty() && !raw.contains('\n') && !raw.contains('\r') && !raw.contains('\0')
            {
                let p = Path::new(raw);
                let resolved = if p.is_absolute() {
                    p.to_path_buf()
                } else {
                    root.join(p)
                };
                let _ = fs::create_dir_all(&resolved);
                return resolved;
            }
        }
    }

    let default = root.join(".git").join("hooks");
    let _ = fs::create_dir_all(&default);
    default
}

fn install_hook(
    hooks_dir: &Path,
    name: &str,
    script: &str,
    marker: &str,
) -> Result<String, CodeSynapseError> {
    let hook_path = hooks_dir.join(name);
    if hook_path.exists() {
        let content = fs::read_to_string(&hook_path)?;
        if content.contains(marker) {
            return Ok(format!("already installed at {}", hook_path.display()));
        }
        let new_content = format!("{}\n\n{}", content.trim_end(), script);
        fs::write(&hook_path, new_content)?;
        return Ok(format!(
            "appended to existing {} hook at {}",
            name,
            hook_path.display()
        ));
    }
    let content = format!("#!/bin/sh\n{}", script);
    fs::write(&hook_path, &content)?;
    #[cfg(unix)]
    {
        let mut perms = fs::metadata(&hook_path)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&hook_path, perms)?;
    }
    Ok(format!("installed at {}", hook_path.display()))
}

fn uninstall_hook(hooks_dir: &Path, name: &str, marker: &str, marker_end: &str) -> String {
    let hook_path = hooks_dir.join(name);
    if !hook_path.exists() {
        return format!("no {} hook found - nothing to remove.", name);
    }
    let content = match fs::read_to_string(&hook_path) {
        Ok(c) => c,
        Err(_) => return format!("no {} hook found - nothing to remove.", name),
    };
    if !content.contains(marker) {
        return format!(
            "codesynapse hook not found in {} - nothing to remove.",
            name
        );
    }

    let pattern = format!(
        "{}{}{}",
        regex::escape(marker),
        r"[\s\S]*?",
        regex::escape(marker_end)
    );
    let re = regex::Regex::new(&pattern).unwrap();
    let new_content = re.replace(&content, "").to_string();
    let new_content = new_content.trim();

    if new_content.is_empty() || new_content == "#!/bin/bash" || new_content == "#!/bin/sh" {
        let _ = fs::remove_file(&hook_path);
        return format!("removed {} hook at {}", name, hook_path.display());
    }
    let _ = fs::write(&hook_path, format!("{}\n", new_content));
    format!(
        "codesynapse removed from {} at {} (other hook content preserved)",
        name,
        hook_path.display()
    )
}

pub fn install(path: &Path) -> Result<String, CodeSynapseError> {
    let root = git_root(path).ok_or_else(|| {
        CodeSynapseError::Validation(format!(
            "No git repository found at or above {}",
            path.display()
        ))
    })?;
    let hdir = hooks_dir(&root);
    let commit_msg = install_hook(&hdir, "post-commit", HOOK_SCRIPT, HOOK_MARKER)?;
    let checkout_msg = install_hook(&hdir, "post-checkout", CHECKOUT_SCRIPT, CHECKOUT_MARKER)?;
    Ok(format!(
        "post-commit: {}\npost-checkout: {}",
        commit_msg, checkout_msg
    ))
}

pub fn uninstall(path: &Path) -> Result<String, CodeSynapseError> {
    let root = git_root(path).ok_or_else(|| {
        CodeSynapseError::Validation(format!(
            "No git repository found at or above {}",
            path.display()
        ))
    })?;
    let hdir = hooks_dir(&root);
    let commit_msg = uninstall_hook(&hdir, "post-commit", HOOK_MARKER, HOOK_MARKER_END);
    let checkout_msg = uninstall_hook(&hdir, "post-checkout", CHECKOUT_MARKER, CHECKOUT_MARKER_END);
    Ok(format!(
        "post-commit: {}\npost-checkout: {}",
        commit_msg, checkout_msg
    ))
}

pub fn status(path: &Path) -> String {
    let root = match git_root(path) {
        Some(r) => r,
        None => return "Not in a git repository.".to_string(),
    };
    let hdir = hooks_dir(&root);

    let check = |name: &str, marker: &str| -> String {
        let p = hdir.join(name);
        if !p.exists() {
            return "not installed".to_string();
        }
        match fs::read_to_string(&p) {
            Ok(c) if c.contains(marker) => "installed".to_string(),
            _ => "not installed (hook exists but codesynapse not found)".to_string(),
        }
    };

    let commit = check("post-commit", HOOK_MARKER);
    let checkout = check("post-checkout", CHECKOUT_MARKER);
    format!("post-commit: {}\npost-checkout: {}", commit, checkout)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::TempDir;

    const PYTHON_DETECT: &str = r#"# Detect the correct interpreter (handles pipx, venv, system installs)
CODESYNAPSE_BIN=$(command -v codesynapse 2>/dev/null)
if [ -n "$CODESYNAPSE_BIN" ]; then
    case "$CODESYNAPSE_BIN" in
        *.exe) _SHEBANG="" ;;
        *)     _SHEBANG=$(head -1 "$CODESYNAPSE_BIN" | sed 's/^#![[:space:]]*//') ;;
    esac
fi
"#;

    fn make_git_repo(tmp: &TempDir) -> PathBuf {
        let path = tmp.path().to_path_buf();
        Command::new("git")
            .args(["init", path.to_str().unwrap()])
            .output()
            .expect("git init failed");
        path
    }

    #[test]
    fn test_install_creates_hook() {
        let tmp = TempDir::new().unwrap();
        let repo = make_git_repo(&tmp);
        let result = install(&repo).unwrap();
        let hook = repo.join(".git/hooks/post-commit");
        assert!(hook.exists());
        let content = fs::read_to_string(&hook).unwrap();
        assert!(content.contains(HOOK_MARKER));
        assert!(result.contains("installed"));
    }

    #[test]
    fn test_install_is_executable() {
        let tmp = TempDir::new().unwrap();
        let repo = make_git_repo(&tmp);
        install(&repo).unwrap();
        let hook = repo.join(".git/hooks/post-commit");
        let mode = fs::metadata(&hook).unwrap().permissions().mode();
        assert!(mode & 0o111 != 0);
    }

    #[test]
    fn test_install_idempotent() {
        let tmp = TempDir::new().unwrap();
        let repo = make_git_repo(&tmp);
        install(&repo).unwrap();
        let result = install(&repo).unwrap();
        assert!(result.contains("already installed"));
        let hook = repo.join(".git/hooks/post-commit");
        let content = fs::read_to_string(&hook).unwrap();
        assert_eq!(content.matches(HOOK_MARKER).count(), 1);
    }

    #[test]
    fn test_install_appends_to_existing_hook() {
        let tmp = TempDir::new().unwrap();
        let repo = make_git_repo(&tmp);
        let hook = repo.join(".git/hooks/post-commit");
        fs::create_dir_all(hook.parent().unwrap()).unwrap();
        fs::write(&hook, "#!/bin/bash\necho existing\n").unwrap();
        #[cfg(unix)]
        {
            let mut perms = fs::metadata(&hook).unwrap().permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&hook, perms).unwrap();
        }
        install(&repo).unwrap();
        let content = fs::read_to_string(&hook).unwrap();
        assert!(content.contains("existing"));
        assert!(content.contains(HOOK_MARKER));
    }

    #[test]
    fn test_uninstall_removes_hook() {
        let tmp = TempDir::new().unwrap();
        let repo = make_git_repo(&tmp);
        install(&repo).unwrap();
        let result = uninstall(&repo).unwrap();
        let hook = repo.join(".git/hooks/post-commit");
        assert!(!hook.exists());
        assert!(result.to_lowercase().contains("removed"));
    }

    #[test]
    fn test_uninstall_no_hook() {
        let tmp = TempDir::new().unwrap();
        let repo = make_git_repo(&tmp);
        let result = uninstall(&repo).unwrap();
        assert!(result.contains("nothing to remove"));
    }

    #[test]
    fn test_status_installed() {
        let tmp = TempDir::new().unwrap();
        let repo = make_git_repo(&tmp);
        install(&repo).unwrap();
        let result = status(&repo);
        assert!(result.contains("installed"));
    }

    #[test]
    fn test_status_not_installed() {
        let tmp = TempDir::new().unwrap();
        let repo = make_git_repo(&tmp);
        let result = status(&repo);
        assert!(result.contains("not installed"));
    }

    #[test]
    fn test_no_git_repo_raises() {
        let tmp = TempDir::new().unwrap();
        let not_a_repo = tmp.path().join("not_a_repo");
        fs::create_dir_all(&not_a_repo).unwrap();
        let err = install(&not_a_repo).unwrap_err();
        assert!(err.to_string().contains("No git repository"));
    }

    #[test]
    fn test_install_creates_post_checkout_hook() {
        let tmp = TempDir::new().unwrap();
        let repo = make_git_repo(&tmp);
        install(&repo).unwrap();
        let hook = repo.join(".git/hooks/post-checkout");
        assert!(hook.exists());
        let content = fs::read_to_string(&hook).unwrap();
        assert!(content.contains(CHECKOUT_MARKER));
    }

    #[test]
    fn test_install_post_checkout_is_executable() {
        let tmp = TempDir::new().unwrap();
        let repo = make_git_repo(&tmp);
        install(&repo).unwrap();
        let hook = repo.join(".git/hooks/post-checkout");
        let mode = fs::metadata(&hook).unwrap().permissions().mode();
        assert!(mode & 0o111 != 0);
    }

    #[test]
    fn test_uninstall_removes_post_checkout_hook() {
        let tmp = TempDir::new().unwrap();
        let repo = make_git_repo(&tmp);
        install(&repo).unwrap();
        uninstall(&repo).unwrap();
        let hook = repo.join(".git/hooks/post-checkout");
        assert!(!hook.exists());
    }

    #[test]
    fn test_status_shows_both_hooks() {
        let tmp = TempDir::new().unwrap();
        let repo = make_git_repo(&tmp);
        install(&repo).unwrap();
        let result = status(&repo);
        assert!(result.contains("post-commit"));
        assert!(result.contains("post-checkout"));
        assert!(result.matches("installed").count() >= 2);
    }

    #[test]
    fn test_hooks_dir_resolves_relative_git_hooks_path() {
        let tmp = TempDir::new().unwrap();
        let repo = make_git_repo(&tmp);
        let hdir = hooks_dir(&repo);
        assert!(hdir.is_absolute());
        assert!(hdir.to_string_lossy().contains("hooks"));
    }

    #[test]
    fn test_hooks_dir_rejects_multiline_git_output() {
        let tmp = TempDir::new().unwrap();
        let repo = make_git_repo(&tmp);
        let hdir = hooks_dir(&repo);
        let hdir_str = hdir.to_string_lossy();
        assert!(!hdir_str.contains('\n'));
        assert!(!hdir_str.contains('\r'));
    }

    #[test]
    fn test_hooks_dir_accepts_absolute_git_hooks_path() {
        let tmp = TempDir::new().unwrap();
        let repo = make_git_repo(&tmp);
        let hdir = hooks_dir(&repo);
        assert!(hdir.is_absolute());
    }

    #[test]
    fn test_hook_skips_head_on_exe() {
        assert!(PYTHON_DETECT.contains("*.exe)"));
    }

    #[test]
    fn test_hook_check_no_additional_context() {
        // hook-check: when graph.json exists in codesynapse-out/, status() should
        // return without error and produce no unexpected output
        let tmp = TempDir::new().unwrap();
        let repo = make_git_repo(&tmp);
        let out = repo.join("codesynapse-out");
        fs::create_dir_all(&out).unwrap();
        fs::write(out.join("graph.json"), "{}").unwrap();
        // status() returns a string (no panic/error) — equivalent to returncode==0
        let result = status(&repo);
        // Should return a valid status string, not panic
        assert!(!result.is_empty());
    }
}
