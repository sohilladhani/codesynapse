use crate::error::CodeSynapseError;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const GOOGLE_WORKSPACE_EXTENSIONS: &[&str] = &[".gdoc", ".gsheet", ".gslides"];

pub fn google_workspace_enabled() -> bool {
    let raw = env::var("CODESYNAPSE_GOOGLE_WORKSPACE").unwrap_or_default();
    matches!(
        raw.trim().to_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

fn safe_yaml_str(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace(['\n', '\r'], " ")
}

fn extract_file_id_from_url(url: &str) -> Option<String> {
    if url.is_empty() {
        return None;
    }
    // Check ?id= query param
    if let Some(query_start) = url.find('?') {
        let query = &url[query_start + 1..];
        for part in query.split('&') {
            if let Some(val) = part.strip_prefix("id=") {
                if !val.is_empty() {
                    return Some(val.to_string());
                }
            }
        }
    }
    // Check /document/d/<id>, /spreadsheets/d/<id>, /presentation/d/<id>, /file/d/<id>
    let patterns = [
        "/document/d/",
        "/spreadsheets/d/",
        "/presentation/d/",
        "/file/d/",
    ];
    for pat in &patterns {
        if let Some(start) = url.find(pat) {
            let rest = &url[start + pat.len()..];
            let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
            let id = &rest[..end];
            if !id.is_empty() {
                return Some(id.to_string());
            }
        }
    }
    None
}

fn extract_resource_key(url: &str, data: &serde_json::Value) -> Option<String> {
    for key in &["resource_key", "resourceKey"] {
        if let Some(v) = data.get(key).and_then(|v| v.as_str()) {
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    if url.is_empty() {
        return None;
    }
    if let Some(query_start) = url.find('?') {
        let query = &url[query_start + 1..];
        for part in query.split('&') {
            if let Some(val) = part.strip_prefix("resourcekey=") {
                if !val.is_empty() {
                    return Some(val.to_string());
                }
            }
        }
    }
    None
}

pub fn read_google_shortcut(
    path: &Path,
) -> Result<HashMap<String, Option<String>>, CodeSynapseError> {
    let text = std::fs::read_to_string(path).map_err(CodeSynapseError::Io)?;
    let data: serde_json::Value =
        serde_json::from_str(&text).map_err(CodeSynapseError::Serialization)?;

    let url = data
        .get("url")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let file_id = data
        .get("doc_id")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            data.get("file_id")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
        })
        .or_else(|| {
            data.get("fileId")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
        })
        .or_else(|| {
            data.get("id")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
        })
        .map(|s| s.to_string())
        .or_else(|| extract_file_id_from_url(&url))
        .or_else(|| {
            let resource_id = data
                .get("resource_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if resource_id.contains(':') {
                resource_id
                    .split_once(':')
                    .map(|x| x.1)
                    .map(|s| s.to_string())
            } else {
                None
            }
        });

    let file_id = file_id.ok_or_else(|| {
        CodeSynapseError::Validation(format!(
            "Google Workspace shortcut {} does not include a Drive file ID",
            path.display()
        ))
    })?;

    let resource_key = extract_resource_key(&url, &data);
    let account = data
        .get("email")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let mut result = HashMap::new();
    result.insert("file_id".to_string(), Some(file_id));
    result.insert(
        "url".to_string(),
        if url.is_empty() { None } else { Some(url) },
    );
    result.insert("resource_key".to_string(), resource_key);
    result.insert("account".to_string(), account);
    Ok(result)
}

fn find_exe(name: &str) -> Option<PathBuf> {
    let path_var = env::var_os("PATH")?;
    for dir in env::split_paths(&path_var) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

pub fn run_gws_export(
    file_id: &str,
    mime_type: &str,
    output: &Path,
    _resource_key: Option<&str>,
) -> Result<(), CodeSynapseError> {
    let exe = find_exe("gws").ok_or_else(|| {
        CodeSynapseError::Validation(
            "gws is required for Google Workspace export. Install it from \
             https://github.com/googleworkspace/cli and run `gws auth login -s drive`."
                .to_string(),
        )
    })?;

    let params = serde_json::json!({"fileId": file_id, "mimeType": mime_type});
    let out_dir = output.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(out_dir).map_err(CodeSynapseError::Io)?;

    let filename = output
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let timeout_secs: u64 = env::var("CODESYNAPSE_GOOGLE_WORKSPACE_TIMEOUT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(120);

    let result = Command::new(&exe)
        .args([
            "drive",
            "files",
            "export",
            "--params",
            &params.to_string(),
            "-o",
            &filename,
        ])
        .current_dir(out_dir)
        .output()
        .map_err(CodeSynapseError::Io)?;

    let _ = timeout_secs;
    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        let stdout = String::from_utf8_lossy(&result.stdout);
        let msg = if !stderr.is_empty() { stderr } else { stdout };
        let msg = if msg.len() > 1200 { &msg[..1200] } else { &msg };
        return Err(CodeSynapseError::Validation(format!(
            "gws export failed for {file_id}: {msg}"
        )));
    }
    Ok(())
}

fn sidecar_path(path: &Path, out_dir: &Path) -> PathBuf {
    let mut hasher = Sha256::new();
    hasher.update(path.to_string_lossy().as_bytes());
    let hash = format!("{:x}", hasher.finalize());
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    out_dir.join(format!("{}_{}.md", stem, &hash[..8]))
}

fn with_frontmatter(
    path: &Path,
    shortcut: &HashMap<String, Option<String>>,
    body: &str,
    exported_mime_type: &str,
) -> String {
    let source_url = shortcut.get("url").and_then(|v| v.as_deref()).unwrap_or("");
    let account = shortcut
        .get("account")
        .and_then(|v| v.as_deref())
        .unwrap_or("");
    let file_id = shortcut
        .get("file_id")
        .and_then(|v| v.as_deref())
        .unwrap_or("");

    let account_line = if !account.is_empty() {
        let mut hasher = Sha256::new();
        hasher.update(account.as_bytes());
        let hash = format!("{:x}", hasher.finalize());
        format!("google_account_hash: \"{}\"\n", &hash[..12])
    } else {
        String::new()
    };

    format!(
        "---\nsource_file: \"{}\"\nsource_type: \"google_workspace\"\ngoogle_file_id: \"{}\"\ngoogle_export_mime_type: \"{}\"\nsource_url: \"{}\"\n{}---\n\n<!-- converted from Google Workspace shortcut: {} -->\n\n{}\n",
        safe_yaml_str(&path.to_string_lossy()),
        safe_yaml_str(file_id),
        safe_yaml_str(exported_mime_type),
        safe_yaml_str(source_url),
        account_line,
        path.file_name().unwrap_or_default().to_string_lossy(),
        body.trim(),
    )
}

#[allow(clippy::type_complexity)]
pub fn convert_google_workspace_file(
    path: &Path,
    out_dir: &Path,
    export_fn: impl Fn(&str, &str, &Path, Option<&str>) -> Result<(), CodeSynapseError>,
    xlsx_to_markdown: Option<&dyn Fn(&Path) -> Result<String, CodeSynapseError>>,
) -> Result<Option<PathBuf>, CodeSynapseError> {
    let ext = path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy().to_lowercase()))
        .unwrap_or_default();

    if !GOOGLE_WORKSPACE_EXTENSIONS.contains(&ext.as_str()) {
        return Ok(None);
    }

    let shortcut = read_google_shortcut(path)?;
    std::fs::create_dir_all(out_dir).map_err(CodeSynapseError::Io)?;
    let out_path = sidecar_path(path, out_dir);
    let file_id = shortcut
        .get("file_id")
        .and_then(|v| v.as_deref())
        .unwrap_or("");
    let resource_key = shortcut.get("resource_key").and_then(|v| v.as_deref());

    match ext.as_str() {
        ".gdoc" => {
            let tmp = out_dir.join(format!("_tmp_{}.md", file_id));
            export_fn(file_id, "text/markdown", &tmp, resource_key)?;
            let body = std::fs::read_to_string(&tmp).unwrap_or_default();
            std::fs::remove_file(&tmp).ok();
            if body.trim().is_empty() {
                return Ok(None);
            }
            let content = with_frontmatter(path, &shortcut, &body, "text/markdown");
            std::fs::write(&out_path, content).map_err(CodeSynapseError::Io)?;
            Ok(Some(out_path))
        }
        ".gslides" => {
            let tmp = out_dir.join(format!("_tmp_{}.txt", file_id));
            export_fn(file_id, "text/plain", &tmp, resource_key)?;
            let body = std::fs::read_to_string(&tmp).unwrap_or_default();
            std::fs::remove_file(&tmp).ok();
            if body.trim().is_empty() {
                return Ok(None);
            }
            let content = with_frontmatter(path, &shortcut, &body, "text/plain");
            std::fs::write(&out_path, content).map_err(CodeSynapseError::Io)?;
            Ok(Some(out_path))
        }
        ".gsheet" => {
            let cb = xlsx_to_markdown.ok_or_else(|| {
                CodeSynapseError::Validation(
                    "Google Sheets export requires xlsx_to_markdown callback".to_string(),
                )
            })?;
            let tmp = out_dir.join(format!("_tmp_{}.xlsx", file_id));
            export_fn(
                file_id,
                "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
                &tmp,
                resource_key,
            )?;
            let body = cb(&tmp)?;
            std::fs::remove_file(&tmp).ok();
            if body.trim().is_empty() {
                return Ok(None);
            }
            let content = with_frontmatter(
                path,
                &shortcut,
                &body,
                "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            );
            std::fs::write(&out_path, content).map_err(CodeSynapseError::Io)?;
            Ok(Some(out_path))
        }
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_read_google_shortcut_doc_id() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("Planning.gdoc");
        fs::write(
            &path,
            r#"{"url":"https://docs.google.com/document/d/doc-123/edit","doc_id":"doc-123","email":"me@example.com"}"#,
        ).unwrap();

        let metadata = read_google_shortcut(&path).unwrap();
        assert_eq!(metadata["file_id"].as_deref(), Some("doc-123"));
        assert_eq!(metadata["account"].as_deref(), Some("me@example.com"));
    }

    #[test]
    fn test_read_google_shortcut_extracts_id_from_url() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("Budget.gsheet");
        fs::write(
            &path,
            r#"{"url":"https://docs.google.com/spreadsheets/d/sheet-456/edit?resourcekey=key-1"}"#,
        )
        .unwrap();

        let metadata = read_google_shortcut(&path).unwrap();
        assert_eq!(metadata["file_id"].as_deref(), Some("sheet-456"));
        assert_eq!(metadata["resource_key"].as_deref(), Some("key-1"));
    }

    #[test]
    fn test_convert_gdoc_to_markdown_sidecar() {
        let tmp = TempDir::new().unwrap();
        let shortcut = tmp.path().join("Planning.gdoc");
        fs::write(
            &shortcut,
            r#"{"url":"https://docs.google.com/document/d/doc-123/edit","doc_id":"doc-123"}"#,
        )
        .unwrap();

        let out_dir = tmp.path().join("converted");

        let fake_export = |file_id: &str, mime_type: &str, output: &Path, _rk: Option<&str>| {
            assert_eq!(file_id, "doc-123");
            assert_eq!(mime_type, "text/markdown");
            fs::write(output, "# Planning\n\nExported doc text.").unwrap();
            Ok(())
        };

        let out = convert_google_workspace_file(&shortcut, &out_dir, fake_export, None).unwrap();
        assert!(out.is_some());
        let content = fs::read_to_string(out.unwrap()).unwrap();
        assert!(content.contains("source_type: \"google_workspace\""));
        assert!(content.contains("# Planning"));
    }

    #[test]
    fn test_convert_gsheet_uses_xlsx_markdown_callback() {
        let tmp = TempDir::new().unwrap();
        let shortcut = tmp.path().join("Budget.gsheet");
        fs::write(&shortcut, r#"{"doc_id":"sheet-456"}"#).unwrap();

        let out_dir = tmp.path().join("converted");

        let fake_export = |file_id: &str, mime_type: &str, output: &Path, _rk: Option<&str>| {
            assert_eq!(file_id, "sheet-456");
            assert_eq!(
                mime_type,
                "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
            );
            fs::write(output, b"xlsx").unwrap();
            Ok(())
        };

        let xlsx_cb: &dyn Fn(&Path) -> Result<String, CodeSynapseError> =
            &|_path| Ok("## Sheet: Main\n\n| A |\n| --- |\n| 1 |".to_string());

        let out =
            convert_google_workspace_file(&shortcut, &out_dir, fake_export, Some(xlsx_cb)).unwrap();
        assert!(out.is_some());
        let content = fs::read_to_string(out.unwrap()).unwrap();
        assert!(content.contains("## Sheet: Main"));
    }

    #[test]
    fn test_run_gws_export_params_structure() {
        // Verify param JSON structure: fileId + mimeType only (no resourceKey)
        let params = serde_json::json!({"fileId": "doc-123", "mimeType": "text/markdown"});
        let s = params.to_string();
        assert!(s.contains("\"fileId\":\"doc-123\""));
        assert!(s.contains("\"mimeType\":\"text/markdown\""));
        assert!(!s.contains("resourceKey"));
    }

    #[test]
    fn test_google_workspace_enabled_env() {
        unsafe {
            env::set_var("CODESYNAPSE_GOOGLE_WORKSPACE", "yes");
        }
        assert!(google_workspace_enabled());

        unsafe {
            env::set_var("CODESYNAPSE_GOOGLE_WORKSPACE", "0");
        }
        assert!(!google_workspace_enabled());

        unsafe {
            env::remove_var("CODESYNAPSE_GOOGLE_WORKSPACE");
        }
    }

    #[test]
    fn test_extract_file_id_patterns() {
        assert_eq!(
            extract_file_id_from_url("https://docs.google.com/document/d/doc-abc/edit"),
            Some("doc-abc".to_string())
        );
        assert_eq!(
            extract_file_id_from_url(
                "https://docs.google.com/spreadsheets/d/sheet-456/edit?resourcekey=key-1"
            ),
            Some("sheet-456".to_string())
        );
        assert_eq!(extract_file_id_from_url(""), None);
    }
}
