use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

use chrono::Utc;
use regex::Regex;

use crate::error::{CodeSynapseError, Result};

const MAX_TEXT_BYTES: usize = 10 * 1024 * 1024;
const MAX_BINARY_BYTES: usize = 50 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq)]
pub enum UrlType {
    Tweet,
    Arxiv,
    Github,
    Youtube,
    Pdf,
    Image,
    Webpage,
}

#[derive(Debug, Clone)]
pub struct IngestResult {
    pub source_url: String,
    pub file_path: PathBuf,
    pub url_type: UrlType,
}

pub fn detect_url_type(url: &str) -> UrlType {
    let lower = url.to_lowercase();
    if lower.contains("twitter.com") || lower.contains("x.com") {
        return UrlType::Tweet;
    }
    if lower.contains("arxiv.org") {
        return UrlType::Arxiv;
    }
    if lower.contains("github.com") {
        return UrlType::Github;
    }
    if lower.contains("youtube.com") || lower.contains("youtu.be") {
        return UrlType::Youtube;
    }
    let path = url_path(url).to_lowercase();
    if path.ends_with(".pdf") {
        return UrlType::Pdf;
    }
    for ext in [".png", ".jpg", ".jpeg", ".webp", ".gif"] {
        if path.ends_with(ext) {
            return UrlType::Image;
        }
    }
    UrlType::Webpage
}

fn url_path(url: &str) -> &str {
    let s = url.find("://").map_or(url, |i| &url[i + 3..]);
    let path_start = s.find('/').unwrap_or(s.len());
    let path = &s[path_start..];
    let path = path.split('?').next().unwrap_or(path);
    path.split('#').next().unwrap_or(path)
}

pub fn safe_filename(url: &str, suffix: &str) -> String {
    let without_scheme = url.find("://").map_or(url, |i| &url[i + 3..]);
    let clean = without_scheme.split('?').next().unwrap_or(without_scheme);
    let clean = clean.split('#').next().unwrap_or(clean);
    let re_nonword = Regex::new(r"[^\w\-]").unwrap();
    let name = re_nonword.replace_all(clean, "_").to_string();
    let name = name.trim_matches('_').to_string();
    let re_multi = Regex::new(r"_+").unwrap();
    let name = re_multi.replace_all(&name, "_").to_string();
    let name: String = name.chars().take(80).collect();
    format!("{name}{suffix}")
}

pub fn yaml_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        let cp = ch as u32;
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\0' => out.push_str("\\0"),
            _ if cp == 0x2028 => out.push_str("\\L"),
            _ if cp == 0x2029 => out.push_str("\\P"),
            _ if cp < 0x20 || cp == 0x7f => out.push_str(&format!("\\x{cp:02x}")),
            _ => out.push(ch),
        }
    }
    out
}

pub fn html_to_text(html: &str) -> String {
    let re_script = Regex::new(r"(?is)<script[^>]*>.*?</script>").unwrap();
    let re_style = Regex::new(r"(?is)<style[^>]*>.*?</style>").unwrap();
    let re_tags = Regex::new(r"<[^>]+>").unwrap();
    let re_ws = Regex::new(r"\s+").unwrap();
    let s = re_script.replace_all(html, " ").into_owned();
    let s = re_style.replace_all(&s, " ").into_owned();
    let s = re_tags.replace_all(&s, " ").into_owned();
    let s = re_ws.replace_all(&s, " ").into_owned();
    let s = s.trim().to_string();
    if s.chars().count() > 8000 {
        s.chars().take(8000).collect()
    } else {
        s
    }
}

pub fn extract_arxiv_id(url: &str) -> Option<String> {
    Regex::new(r"(\d{4}\.\d{4,5})")
        .unwrap()
        .captures(url)
        .map(|c| c[1].to_string())
}

fn fetch_text(url: &str) -> Result<String> {
    let resp = ureq::get(url)
        .set("User-Agent", "Mozilla/5.0 codesynapse/1.0")
        .call()
        .map_err(|e| CodeSynapseError::Other(format!("fetch failed for {url:?}: {e}")))?;
    let mut buf = Vec::new();
    resp.into_reader()
        .take((MAX_TEXT_BYTES + 1) as u64)
        .read_to_end(&mut buf)
        .map_err(CodeSynapseError::Io)?;
    if buf.len() > MAX_TEXT_BYTES {
        return Err(CodeSynapseError::Other(format!(
            "response from {url:?} exceeds {} MiB limit",
            MAX_TEXT_BYTES / 1_048_576
        )));
    }
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

fn fetch_bytes(url: &str) -> Result<Vec<u8>> {
    let resp = ureq::get(url)
        .set("User-Agent", "Mozilla/5.0 codesynapse/1.0")
        .call()
        .map_err(|e| CodeSynapseError::Other(format!("fetch failed for {url:?}: {e}")))?;
    let mut buf = Vec::new();
    resp.into_reader()
        .take((MAX_BINARY_BYTES + 1) as u64)
        .read_to_end(&mut buf)
        .map_err(CodeSynapseError::Io)?;
    if buf.len() > MAX_BINARY_BYTES {
        return Err(CodeSynapseError::Other(format!(
            "response from {url:?} exceeds {} MiB limit",
            MAX_BINARY_BYTES / 1_048_576
        )));
    }
    Ok(buf)
}

fn html_title(html: &str) -> String {
    Regex::new(r"(?is)<title[^>]*>(.*?)</title>")
        .unwrap()
        .captures(html)
        .map(|c| {
            Regex::new(r"\s+")
                .unwrap()
                .replace_all(&c[1], " ")
                .trim()
                .to_string()
        })
        .unwrap_or_default()
}

fn fetch_webpage_content(url: &str) -> Result<(String, String)> {
    let html = fetch_text(url)?;
    let title = html_title(&html);
    let title = if title.is_empty() {
        url.to_string()
    } else {
        title
    };
    let text = html_to_text(&html);
    Ok((title, text))
}

fn fetch_arxiv_content(url: &str) -> Result<(String, String)> {
    let Some(arxiv_id) = extract_arxiv_id(url) else {
        let (title, text) = fetch_webpage_content(url)?;
        let now = Utc::now().to_rfc3339();
        let content = format!(
            "---\nsource_url: \"{}\"\ntype: webpage\ntitle: \"{}\"\ncaptured_at: {now}\n---\n\n# {title}\n\nSource: {url}\n\n---\n\n{text}\n",
            yaml_str(url),
            yaml_str(&title),
        );
        return Ok((content, safe_filename(url, ".md")));
    };

    let api_url = format!("https://export.arxiv.org/abs/{arxiv_id}");
    let (title, abstract_text, paper_authors) = match fetch_text(&api_url) {
        Ok(html) => {
            let re_tags = Regex::new(r"<[^>]+>").unwrap();
            let abstract_text = Regex::new(r#"(?is)class="abstract[^"]*"[^>]*>(.*?)</blockquote>"#)
                .unwrap()
                .captures(&html)
                .map(|c| re_tags.replace_all(&c[1], "").trim().to_string())
                .unwrap_or_default();
            let title = Regex::new(r#"(?is)class="title[^"]*"[^>]*>(.*?)</h1>"#)
                .unwrap()
                .captures(&html)
                .map(|c| re_tags.replace_all(&c[1], " ").trim().to_string())
                .unwrap_or_else(|| arxiv_id.clone());
            let paper_authors = Regex::new(r#"(?is)class="authors"[^>]*>(.*?)</div>"#)
                .unwrap()
                .captures(&html)
                .map(|c| re_tags.replace_all(&c[1], "").trim().to_string())
                .unwrap_or_default();
            (title, abstract_text, paper_authors)
        }
        Err(_) => (arxiv_id.clone(), String::new(), String::new()),
    };

    let now = Utc::now().to_rfc3339();
    let content = format!(
        "---\nsource_url: \"{}\"\narxiv_id: \"{}\"\ntype: paper\ntitle: \"{}\"\npaper_authors: \"{}\"\ncaptured_at: {now}\n---\n\n# {title}\n\n**Authors:** {paper_authors}\n**arXiv:** {arxiv_id}\n\n## Abstract\n\n{abstract_text}\n\nSource: {url}\n",
        yaml_str(url),
        yaml_str(&arxiv_id),
        yaml_str(&title),
        yaml_str(&paper_authors),
    );
    let filename = format!("arxiv_{}.md", arxiv_id.replace('.', "_"));
    Ok((content, filename))
}

fn fetch_tweet_content(url: &str) -> Result<(String, String)> {
    let oembed_url = url.replace("x.com", "twitter.com");
    let (tweet_text, tweet_author) = match ureq::get("https://publish.twitter.com/oembed")
        .query("url", &oembed_url)
        .query("omit_script", "true")
        .set("User-Agent", "Mozilla/5.0 codesynapse/1.0")
        .call()
    {
        Ok(resp) => {
            let mut buf = Vec::new();
            let _ = resp
                .into_reader()
                .take((MAX_TEXT_BYTES + 1) as u64)
                .read_to_end(&mut buf);
            match serde_json::from_slice::<serde_json::Value>(&buf) {
                Ok(data) => {
                    let html = data.get("html").and_then(|v| v.as_str()).unwrap_or("");
                    let text = Regex::new(r"<[^>]+>")
                        .unwrap()
                        .replace_all(html, "")
                        .trim()
                        .to_string();
                    let author = data
                        .get("author_name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string();
                    (text, author)
                }
                Err(_) => (
                    format!("Tweet at {url} (could not fetch content)"),
                    "unknown".to_string(),
                ),
            }
        }
        Err(_) => (
            format!("Tweet at {url} (could not fetch content)"),
            "unknown".to_string(),
        ),
    };

    let now = Utc::now().to_rfc3339();
    let content = format!(
        "---\nsource_url: \"{}\"\ntype: tweet\nauthor: \"{}\"\ncaptured_at: {now}\n---\n\n# Tweet by @{tweet_author}\n\n{tweet_text}\n\nSource: {url}\n",
        yaml_str(url),
        yaml_str(&tweet_author),
    );
    Ok((content, safe_filename(url, ".md")))
}

fn clone_github(url: &str, target_dir: &Path) -> Result<PathBuf> {
    let repo_name = url.split('/').next_back().unwrap_or("repo");
    let repo_name = repo_name.trim_end_matches(".git");
    let clone_dir = target_dir.join(repo_name);
    let status = Command::new("git")
        .args([
            "clone",
            "--depth",
            "1",
            url,
            clone_dir.to_str().unwrap_or("."),
        ])
        .status()
        .map_err(CodeSynapseError::Io)?;
    if !status.success() {
        return Err(CodeSynapseError::Other(format!(
            "git clone failed for {url:?}"
        )));
    }
    Ok(clone_dir)
}

fn unique_path(target_dir: &Path, filename: &str) -> PathBuf {
    let out = target_dir.join(filename);
    if !out.exists() {
        return out;
    }
    let p = Path::new(filename);
    let stem = p.file_stem().unwrap_or_default().to_string_lossy();
    let ext = p.extension().unwrap_or_default().to_string_lossy();
    let ext_str = if ext.is_empty() {
        String::new()
    } else {
        format!(".{ext}")
    };
    for i in 1..1000 {
        let candidate = target_dir.join(format!("{stem}_{i}{ext_str}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    target_dir.join(filename)
}

pub fn ingest(url: &str, target_dir: &Path) -> Result<IngestResult> {
    fs::create_dir_all(target_dir).map_err(CodeSynapseError::Io)?;
    match detect_url_type(url) {
        UrlType::Github => {
            let file_path = clone_github(url, target_dir)?;
            Ok(IngestResult {
                source_url: url.to_string(),
                file_path,
                url_type: UrlType::Github,
            })
        }
        UrlType::Pdf => {
            let bytes = fetch_bytes(url)?;
            let filename = safe_filename(url, ".pdf");
            let out_path = unique_path(target_dir, &filename);
            fs::write(&out_path, bytes).map_err(CodeSynapseError::Io)?;
            Ok(IngestResult {
                source_url: url.to_string(),
                file_path: out_path,
                url_type: UrlType::Pdf,
            })
        }
        UrlType::Image => {
            let suffix = url_path(url)
                .rsplit('.')
                .next()
                .map(|e| format!(".{e}"))
                .unwrap_or_else(|| ".jpg".to_string());
            let bytes = fetch_bytes(url)?;
            let filename = safe_filename(url, &suffix);
            let out_path = unique_path(target_dir, &filename);
            fs::write(&out_path, bytes).map_err(CodeSynapseError::Io)?;
            Ok(IngestResult {
                source_url: url.to_string(),
                file_path: out_path,
                url_type: UrlType::Image,
            })
        }
        UrlType::Arxiv => {
            let (content, filename) = fetch_arxiv_content(url)?;
            let out_path = unique_path(target_dir, &filename);
            fs::write(&out_path, content).map_err(CodeSynapseError::Io)?;
            Ok(IngestResult {
                source_url: url.to_string(),
                file_path: out_path,
                url_type: UrlType::Arxiv,
            })
        }
        UrlType::Tweet => {
            let (content, filename) = fetch_tweet_content(url)?;
            let out_path = unique_path(target_dir, &filename);
            fs::write(&out_path, content).map_err(CodeSynapseError::Io)?;
            Ok(IngestResult {
                source_url: url.to_string(),
                file_path: out_path,
                url_type: UrlType::Tweet,
            })
        }
        url_type @ (UrlType::Youtube | UrlType::Webpage) => {
            let (title, text) = fetch_webpage_content(url)?;
            let now = Utc::now().to_rfc3339();
            let content = format!(
                "---\nsource_url: \"{}\"\ntype: webpage\ntitle: \"{}\"\ncaptured_at: {now}\n---\n\n# {title}\n\nSource: {url}\n\n---\n\n{text}\n",
                yaml_str(url),
                yaml_str(&title),
            );
            let filename = safe_filename(url, ".md");
            let out_path = unique_path(target_dir, &filename);
            fs::write(&out_path, content).map_err(CodeSynapseError::Io)?;
            Ok(IngestResult {
                source_url: url.to_string(),
                file_path: out_path,
                url_type,
            })
        }
    }
}

pub fn save_query_result(
    question: &str,
    answer: &str,
    memory_dir: &Path,
    source_nodes: Option<&[&str]>,
) -> Result<PathBuf> {
    fs::create_dir_all(memory_dir).map_err(CodeSynapseError::Io)?;
    let now = Utc::now();
    let slug: String = question
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .take(50)
        .collect::<String>()
        .trim_end_matches('_')
        .to_string();
    let filename = format!("query_{}_{slug}.md", now.format("%Y%m%d_%H%M%S"));
    let mut lines: Vec<String> = vec![
        "---".into(),
        "type: \"query\"".into(),
        format!("date: \"{}\"", now.to_rfc3339()),
        format!("question: \"{}\"", yaml_str(question)),
        "contributor: \"codesynapse\"".into(),
    ];
    if let Some(nodes) = source_nodes {
        let nodes_str: Vec<String> = nodes.iter().take(10).map(|n| format!("\"{n}\"")).collect();
        lines.push(format!("source_nodes: [{}]", nodes_str.join(", ")));
    }
    lines.push("---".into());
    lines.push(String::new());
    lines.push(format!("# Q: {question}"));
    lines.push(String::new());
    lines.push("## Answer".into());
    lines.push(String::new());
    lines.push(answer.into());
    if let Some(nodes) = source_nodes {
        lines.push(String::new());
        lines.push("## Source Nodes".into());
        lines.push(String::new());
        for node in nodes {
            lines.push(format!("- {node}"));
        }
    }
    let content = lines.join("\n");
    let out_path = memory_dir.join(&filename);
    fs::write(&out_path, content).map_err(CodeSynapseError::Io)?;
    Ok(out_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_detect_url_type_arxiv() {
        assert_eq!(
            detect_url_type("https://arxiv.org/abs/2301.00001"),
            UrlType::Arxiv
        );
        assert_eq!(
            detect_url_type("https://arxiv.org/pdf/2301.00001"),
            UrlType::Arxiv
        );
    }

    #[test]
    fn test_detect_url_type_github() {
        assert_eq!(
            detect_url_type("https://github.com/rust-lang/rust"),
            UrlType::Github
        );
    }

    #[test]
    fn test_detect_url_type_pdf() {
        assert_eq!(
            detect_url_type("https://example.com/paper.pdf"),
            UrlType::Pdf
        );
    }

    #[test]
    fn test_detect_url_type_image() {
        assert_eq!(
            detect_url_type("https://example.com/photo.jpg"),
            UrlType::Image
        );
        assert_eq!(
            detect_url_type("https://example.com/photo.png"),
            UrlType::Image
        );
        assert_eq!(
            detect_url_type("https://example.com/img.webp"),
            UrlType::Image
        );
    }

    #[test]
    fn test_detect_url_type_tweet() {
        assert_eq!(
            detect_url_type("https://twitter.com/user/status/123"),
            UrlType::Tweet
        );
        assert_eq!(
            detect_url_type("https://x.com/user/status/123"),
            UrlType::Tweet
        );
    }

    #[test]
    fn test_detect_url_type_youtube() {
        assert_eq!(
            detect_url_type("https://youtube.com/watch?v=abc"),
            UrlType::Youtube
        );
        assert_eq!(detect_url_type("https://youtu.be/abc"), UrlType::Youtube);
    }

    #[test]
    fn test_detect_url_type_webpage() {
        assert_eq!(
            detect_url_type("https://example.com/page"),
            UrlType::Webpage
        );
        assert_eq!(detect_url_type("https://example.com/"), UrlType::Webpage);
        assert_eq!(
            detect_url_type("https://docs.rs/crate/1.0/"),
            UrlType::Webpage
        );
    }

    #[test]
    fn test_safe_filename_basic() {
        let name = safe_filename("https://example.com/path/to/page", ".md");
        assert!(name.ends_with(".md"), "name: {name}");
        assert!(!name.contains('/'), "name: {name}");
        assert!(!name.contains(':'), "name: {name}");
    }

    #[test]
    fn test_safe_filename_strips_query() {
        let a = safe_filename("https://example.com/page?foo=bar", ".md");
        let b = safe_filename("https://example.com/page", ".md");
        assert_eq!(a, b);
    }

    #[test]
    fn test_safe_filename_max_length() {
        let long_url = format!("https://example.com/{}", "a".repeat(200));
        let name = safe_filename(&long_url, ".md");
        let stem = name.strip_suffix(".md").unwrap_or(&name);
        assert!(
            stem.chars().count() <= 80,
            "stem len {} > 80",
            stem.chars().count()
        );
    }

    #[test]
    fn test_yaml_str_backslash() {
        assert_eq!(yaml_str("a\\b"), "a\\\\b");
    }

    #[test]
    fn test_yaml_str_quote() {
        assert_eq!(yaml_str(r#"say "hi""#), r#"say \"hi\""#);
    }

    #[test]
    fn test_yaml_str_newline() {
        assert_eq!(yaml_str("line1\nline2"), "line1\\nline2");
    }

    #[test]
    fn test_yaml_str_tab() {
        assert_eq!(yaml_str("col1\tcol2"), "col1\\tcol2");
    }

    #[test]
    fn test_yaml_str_control_char() {
        assert_eq!(yaml_str("\x01"), "\\x01");
        assert_eq!(yaml_str("\x7f"), "\\x7f");
    }

    #[test]
    fn test_yaml_str_unicode_separators() {
        assert_eq!(yaml_str("\u{2028}"), "\\L");
        assert_eq!(yaml_str("\u{2029}"), "\\P");
    }

    #[test]
    fn test_yaml_str_null() {
        assert_eq!(yaml_str("\0"), "\\0");
    }

    #[test]
    fn test_yaml_str_carriage_return() {
        assert_eq!(yaml_str("\r"), "\\r");
    }

    #[test]
    fn test_html_to_text_basic() {
        let text = html_to_text("<p>Hello <b>world</b></p>");
        assert!(text.contains("Hello"), "text: {text}");
        assert!(text.contains("world"), "text: {text}");
        assert!(!text.contains('<'), "text: {text}");
    }

    #[test]
    fn test_html_to_text_strips_script() {
        let html = "<p>Hello</p><script>alert('xss')</script><p>World</p>";
        let text = html_to_text(html);
        assert!(text.contains("Hello"), "text: {text}");
        assert!(text.contains("World"), "text: {text}");
        assert!(!text.contains("alert"), "text: {text}");
        assert!(!text.contains("xss"), "text: {text}");
    }

    #[test]
    fn test_html_to_text_strips_style() {
        let html = "<style>.foo { color: red }</style><p>Content</p>";
        let text = html_to_text(html);
        assert!(text.contains("Content"), "text: {text}");
        assert!(!text.contains("color"), "text: {text}");
    }

    #[test]
    fn test_html_to_text_truncates_at_8000_chars() {
        let big_text = "word ".repeat(2000);
        let html = format!("<p>{big_text}</p>");
        let text = html_to_text(&html);
        assert!(
            text.chars().count() <= 8000,
            "char count {} > 8000",
            text.chars().count()
        );
    }

    #[test]
    fn test_extract_arxiv_id_abs_url() {
        assert_eq!(
            extract_arxiv_id("https://arxiv.org/abs/2301.00001"),
            Some("2301.00001".to_string())
        );
    }

    #[test]
    fn test_extract_arxiv_id_pdf_url() {
        assert_eq!(
            extract_arxiv_id("https://arxiv.org/pdf/2305.12345"),
            Some("2305.12345".to_string())
        );
    }

    #[test]
    fn test_extract_arxiv_id_five_digits() {
        assert_eq!(
            extract_arxiv_id("https://arxiv.org/abs/2301.12345"),
            Some("2301.12345".to_string())
        );
    }

    #[test]
    fn test_extract_arxiv_id_missing() {
        assert_eq!(extract_arxiv_id("https://example.com/paper"), None);
    }

    #[test]
    fn test_ingest_result_fields() {
        let r = IngestResult {
            source_url: "https://example.com".to_string(),
            file_path: PathBuf::from("/tmp/test.md"),
            url_type: UrlType::Webpage,
        };
        assert_eq!(r.source_url, "https://example.com");
        assert_eq!(r.url_type, UrlType::Webpage);
        assert_eq!(r.file_path, PathBuf::from("/tmp/test.md"));
    }

    #[test]
    fn test_unique_path_no_conflict() {
        let dir = std::env::temp_dir();
        let path = unique_path(&dir, "codesynapse_no_such_file_xyz_99999.md");
        assert_eq!(path, dir.join("codesynapse_no_such_file_xyz_99999.md"));
    }

    #[test]
    fn test_unique_path_with_conflict() {
        let dir = std::env::temp_dir();
        let filename = "codesynapse_ingest_test_conflict.md";
        let p = dir.join(filename);
        fs::write(&p, "x").unwrap();
        let result = unique_path(&dir, filename);
        assert_ne!(result, p, "should not overwrite existing file");
        assert!(
            result
                .to_string_lossy()
                .contains("codesynapse_ingest_test_conflict_1"),
            "result: {}",
            result.display()
        );
        fs::remove_file(p).ok();
    }

    #[test]
    fn test_save_query_result_creates_file() {
        let dir = std::env::temp_dir().join("codesynapse_test_save_query_basic");
        fs::create_dir_all(&dir).unwrap();
        let path =
            save_query_result("What is a graph?", "A set of nodes and edges.", &dir, None).unwrap();
        assert!(
            path.exists(),
            "file should be created at {}",
            path.display()
        );
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("What is a graph?"), "content: {content}");
        assert!(content.contains("nodes and edges"), "content: {content}");
        assert!(content.contains("type: \"query\""), "content: {content}");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_save_query_result_with_nodes() {
        let dir = std::env::temp_dir().join("codesynapse_test_save_query_nodes");
        fs::create_dir_all(&dir).unwrap();
        let nodes = ["node_a", "node_b"];
        let path = save_query_result("Query?", "Answer.", &dir, Some(&nodes)).unwrap();
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("node_a"), "content: {content}");
        assert!(content.contains("## Source Nodes"), "content: {content}");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    #[ignore]
    fn test_ingest_arxiv_live() {
        let dir = std::env::temp_dir().join("codesynapse_test_arxiv_live");
        fs::create_dir_all(&dir).unwrap();
        let result = ingest("https://arxiv.org/abs/2301.00001", &dir).unwrap();
        assert_eq!(result.url_type, UrlType::Arxiv);
        assert!(result.file_path.exists());
        let content = fs::read_to_string(&result.file_path).unwrap();
        assert!(content.contains("arxiv_id"), "content: {content}");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    #[ignore]
    fn test_ingest_webpage_live() {
        let dir = std::env::temp_dir().join("codesynapse_test_webpage_live");
        fs::create_dir_all(&dir).unwrap();
        let result = ingest("https://example.com", &dir).unwrap();
        assert_eq!(result.url_type, UrlType::Webpage);
        assert!(result.file_path.exists());
        let content = fs::read_to_string(&result.file_path).unwrap();
        assert!(content.contains("source_url"), "content: {content}");
        fs::remove_dir_all(&dir).ok();
    }
}
