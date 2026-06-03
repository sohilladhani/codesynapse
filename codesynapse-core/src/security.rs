use serde_json::Value;
use std::collections::HashMap;
use std::net::{Ipv4Addr, Ipv6Addr, ToSocketAddrs};
use std::path::{Path, PathBuf};

use crate::error::{CodeSynapseError, Result};

pub const MAX_FETCH_BYTES: u64 = 52_428_800;
pub const MAX_TEXT_BYTES: u64 = 10_485_760;
pub const MAX_GRAPH_FILE_BYTES: u64 = 512 * 1024 * 1024;

const MAX_LABEL_LEN: usize = 256;
const METADATA_MAX_VALUE_LEN: usize = 512;
const METADATA_MAX_LIST_ITEMS: usize = 50;

static BLOCKED_HOSTS: &[&str] = &[
    "metadata.google.internal",
    "metadata.google.com",
    "169.254.169.254",
];

fn is_private_ipv4(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    o[0] == 127
        || o[0] == 10
        || (o[0] == 172 && (16..=31).contains(&o[1]))
        || (o[0] == 192 && o[1] == 168)
        || (o[0] == 169 && o[1] == 254)
        || (o[0] == 100 && (64..=127).contains(&o[1]))
        || o[0] == 0
        || o == [255, 255, 255, 255]
}

fn is_private_ipv6(ip: Ipv6Addr) -> bool {
    let segs = ip.segments();
    if segs == [0, 0, 0, 0, 0, 0, 0, 1] {
        return true;
    }
    if (segs[0] & 0xffc0) == 0xfe80 {
        return true;
    }
    if (segs[0] & 0xfe00) == 0xfc00 {
        return true;
    }
    // IPv4-mapped: ::ffff:x.x.x.x — check the embedded IPv4
    if segs[0] == 0
        && segs[1] == 0
        && segs[2] == 0
        && segs[3] == 0
        && segs[4] == 0
        && segs[5] == 0xffff
    {
        let v4 = Ipv4Addr::new(
            (segs[6] >> 8) as u8,
            segs[6] as u8,
            (segs[7] >> 8) as u8,
            segs[7] as u8,
        );
        return is_private_ipv4(v4);
    }
    false
}

fn parse_url_parts(url: &str) -> Option<(String, String)> {
    let lower = url.to_lowercase();
    let scheme_end = lower.find("://")?;
    let scheme = lower[..scheme_end].to_string();
    let rest = &lower[scheme_end + 3..];

    let host = if rest.starts_with('[') {
        let close = rest.find(']')?;
        rest[..=close].to_string()
    } else {
        let host_end = rest.find(['/', '?', '#', ':']).unwrap_or(rest.len());
        rest[..host_end].to_string()
    };

    Some((scheme, host))
}

pub fn validate_url(url: &str) -> Result<()> {
    let (scheme, host) = parse_url_parts(url)
        .ok_or_else(|| CodeSynapseError::Validation(format!("Invalid URL: {url:?}")))?;

    if scheme != "http" && scheme != "https" {
        return Err(CodeSynapseError::Validation(format!(
            "Blocked URL scheme '{scheme}' - only http and https are allowed. Got: {url:?}"
        )));
    }

    if host.is_empty() {
        return Err(CodeSynapseError::Validation(format!(
            "URL has no host: {url:?}"
        )));
    }

    if BLOCKED_HOSTS.contains(&host.as_str()) {
        return Err(CodeSynapseError::Validation(format!(
            "Blocked cloud metadata endpoint '{host}'. Got: {url:?}"
        )));
    }

    let resolve_target = format!("{host}:80");
    let addrs = resolve_target.to_socket_addrs().map_err(|e| {
        CodeSynapseError::Validation(format!(
            "DNS resolution failed for '{host}': {e}. Got: {url:?}"
        ))
    })?;

    for addr in addrs {
        let blocked = match addr.ip() {
            std::net::IpAddr::V4(v4) => is_private_ipv4(v4),
            std::net::IpAddr::V6(v6) => is_private_ipv6(v6),
        };
        if blocked {
            return Err(CodeSynapseError::Validation(format!(
                "Blocked private/internal IP {} (resolved from '{host}'). Got: {url:?}",
                addr.ip()
            )));
        }
    }

    Ok(())
}

pub fn validate_path(path: &Path, base: &Path) -> Result<PathBuf> {
    if !base.exists() {
        return Err(CodeSynapseError::Validation(format!(
            "Base directory does not exist: {}",
            base.display()
        )));
    }
    let base_canon = base.canonicalize()?;
    let resolved = path
        .canonicalize()
        .map_err(|_| CodeSynapseError::NotFound(format!("Path not found: {}", path.display())))?;
    if !resolved.starts_with(&base_canon) {
        return Err(CodeSynapseError::Validation(format!(
            "Path {:?} escapes the allowed directory {}",
            path,
            base_canon.display()
        )));
    }
    Ok(resolved)
}

pub fn check_file_size(path: &Path, max_bytes: u64) -> Result<()> {
    let size = match path.metadata() {
        Ok(m) => m.len(),
        Err(_) => return Ok(()),
    };
    if size > max_bytes {
        return Err(CodeSynapseError::Validation(format!(
            "File {} is {size} bytes, exceeds {max_bytes}-byte cap",
            path.display()
        )));
    }
    Ok(())
}

fn strip_control_chars(s: &str) -> String {
    s.chars().filter(|&c| c > '\x1f' && c != '\x7f').collect()
}

fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#x27;"),
            other => out.push(other),
        }
    }
    out
}

pub fn sanitize_label(text: Option<&str>) -> String {
    let text = match text {
        None => return String::new(),
        Some(t) => t,
    };
    strip_control_chars(text)
        .chars()
        .take(MAX_LABEL_LEN)
        .collect()
}

fn sanitize_metadata_str_value(value: &str) -> String {
    let text = strip_control_chars(value);
    let text = html_escape(&text);
    text.chars().take(METADATA_MAX_VALUE_LEN).collect()
}

fn sanitize_json_value(value: &Value) -> Value {
    match value {
        Value::Bool(b) => Value::Bool(*b),
        Value::Number(n) => Value::Number(n.clone()),
        Value::Null => Value::Null,
        Value::String(s) => Value::String(sanitize_metadata_str_value(s)),
        Value::Array(arr) => Value::Array(
            arr.iter()
                .take(METADATA_MAX_LIST_ITEMS)
                .map(sanitize_json_value)
                .collect(),
        ),
        Value::Object(obj) => Value::Object(
            obj.iter()
                .filter_map(|(k, v)| {
                    let ck = sanitize_metadata_str_value(k);
                    if ck.is_empty() {
                        None
                    } else {
                        Some((ck, sanitize_json_value(v)))
                    }
                })
                .collect(),
        ),
    }
}

pub fn sanitize_metadata(metadata: Option<&HashMap<String, Value>>) -> HashMap<String, Value> {
    let Some(meta) = metadata else {
        return HashMap::new();
    };
    let mut result = HashMap::new();
    for (key, value) in meta {
        let ck = sanitize_metadata_str_value(key);
        if ck.is_empty() {
            continue;
        }
        result.insert(ck, sanitize_json_value(value));
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_validate_url_allows_https() {
        assert!(validate_url("https://example.com/page").is_ok());
    }

    #[test]
    fn test_validate_url_blocks_file_scheme() {
        let e = validate_url("file:///etc/passwd").unwrap_err();
        assert!(e.to_string().contains("Blocked URL scheme"));
    }

    #[test]
    fn test_validate_url_blocks_ftp_scheme() {
        let e = validate_url("ftp://example.com/file").unwrap_err();
        assert!(e.to_string().contains("Blocked URL scheme"));
    }

    #[test]
    fn test_validate_url_blocks_data_scheme() {
        let e = validate_url("data:text/html,<h1>hi</h1>").unwrap_err();
        let msg = e.to_string();
        assert!(msg.contains("Blocked URL scheme") || msg.contains("Invalid URL"));
    }

    #[test]
    fn test_validate_url_blocks_loopback_ipv4() {
        let e = validate_url("http://127.0.0.1/secret").unwrap_err();
        assert!(e.to_string().contains("Blocked private/internal IP"));
    }

    #[test]
    fn test_validate_url_blocks_loopback_alt() {
        let e = validate_url("http://127.0.0.2/secret").unwrap_err();
        assert!(e.to_string().contains("Blocked private/internal IP"));
    }

    #[test]
    fn test_validate_url_blocks_private_10() {
        let e = validate_url("http://10.0.0.1/").unwrap_err();
        assert!(e.to_string().contains("Blocked private/internal IP"));
    }

    #[test]
    fn test_validate_url_blocks_private_172() {
        let e = validate_url("http://172.16.0.1/").unwrap_err();
        assert!(e.to_string().contains("Blocked private/internal IP"));
    }

    #[test]
    fn test_validate_url_blocks_private_192() {
        let e = validate_url("http://192.168.1.1/").unwrap_err();
        assert!(e.to_string().contains("Blocked private/internal IP"));
    }

    #[test]
    fn test_validate_url_blocks_link_local() {
        let e = validate_url("http://169.254.0.1/").unwrap_err();
        assert!(e.to_string().contains("Blocked private/internal IP"));
    }

    #[test]
    fn test_validate_url_blocks_cgn() {
        let e = validate_url("http://100.64.0.1/").unwrap_err();
        assert!(e.to_string().contains("Blocked private/internal IP"));
    }

    #[test]
    fn test_validate_url_blocks_cloud_metadata_hostname() {
        let e = validate_url("http://metadata.google.internal/").unwrap_err();
        let msg = e.to_string();
        assert!(
            msg.contains("Blocked cloud metadata endpoint")
                || msg.contains("Blocked private/internal IP")
                || msg.contains("DNS resolution failed")
        );
    }

    #[test]
    fn test_validate_url_blocks_aws_metadata_ip() {
        let e = validate_url("http://169.254.169.254/latest/meta-data/").unwrap_err();
        assert!(e.to_string().contains("Blocked"));
    }

    #[test]
    fn test_validate_url_blocks_ipv6_loopback() {
        let e = validate_url("http://[::1]/").unwrap_err();
        let msg = e.to_string();
        assert!(msg.contains("Blocked") || msg.contains("DNS"));
    }

    #[test]
    fn test_is_private_ipv4_loopback() {
        assert!(is_private_ipv4(Ipv4Addr::new(127, 0, 0, 1)));
        assert!(is_private_ipv4(Ipv4Addr::new(127, 255, 255, 254)));
    }

    #[test]
    fn test_is_private_ipv4_rfc1918() {
        assert!(is_private_ipv4(Ipv4Addr::new(10, 0, 0, 1)));
        assert!(is_private_ipv4(Ipv4Addr::new(172, 16, 0, 1)));
        assert!(is_private_ipv4(Ipv4Addr::new(172, 31, 255, 255)));
        assert!(is_private_ipv4(Ipv4Addr::new(192, 168, 0, 1)));
    }

    #[test]
    fn test_is_private_ipv4_not_private() {
        assert!(!is_private_ipv4(Ipv4Addr::new(8, 8, 8, 8)));
        assert!(!is_private_ipv4(Ipv4Addr::new(172, 32, 0, 1)));
        assert!(!is_private_ipv4(Ipv4Addr::new(1, 1, 1, 1)));
    }

    #[test]
    fn test_is_private_ipv4_cgn() {
        assert!(is_private_ipv4(Ipv4Addr::new(100, 64, 0, 1)));
        assert!(is_private_ipv4(Ipv4Addr::new(100, 127, 255, 255)));
        assert!(!is_private_ipv4(Ipv4Addr::new(100, 128, 0, 1)));
    }

    #[test]
    fn test_is_private_ipv6_loopback() {
        assert!(is_private_ipv6(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1)));
    }

    #[test]
    fn test_is_private_ipv6_link_local() {
        assert!(is_private_ipv6(Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 1)));
    }

    #[test]
    fn test_is_private_ipv6_ula() {
        assert!(is_private_ipv6(Ipv6Addr::new(0xfc00, 0, 0, 0, 0, 0, 0, 1)));
        assert!(is_private_ipv6(Ipv6Addr::new(0xfd00, 0, 0, 0, 0, 0, 0, 1)));
    }

    #[test]
    fn test_is_private_ipv6_mapped_private_v4() {
        // ::ffff:192.168.1.1
        let ip = Ipv6Addr::new(0, 0, 0, 0, 0, 0xffff, 0xc0a8, 0x0101);
        assert!(is_private_ipv6(ip));
    }

    #[test]
    fn test_validate_path_allows_valid() {
        let dir = std::env::temp_dir().join("codesynapse_sec_valid");
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("data.json");
        fs::write(&file, b"{}").unwrap();
        let result = validate_path(&file, &dir);
        assert!(result.is_ok());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_validate_path_blocks_traversal() {
        let dir = std::env::temp_dir().join("codesynapse_sec_trav");
        fs::create_dir_all(&dir).unwrap();
        let outside_file = std::env::temp_dir().join("codesynapse_sec_trav_outside.txt");
        fs::write(&outside_file, b"x").unwrap();
        let result = validate_path(&outside_file, &dir);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("escapes"));
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_file(&outside_file);
    }

    #[test]
    fn test_validate_path_nonexistent_base() {
        let base = Path::new("/nonexistent_codesynapse_base_dir_test");
        let path = Path::new("/nonexistent_codesynapse_base_dir_test/file.json");
        let result = validate_path(path, base);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("does not exist"));
    }

    #[test]
    fn test_validate_path_file_not_found() {
        let dir = std::env::temp_dir().join("codesynapse_sec_notfound");
        fs::create_dir_all(&dir).unwrap();
        let missing = dir.join("missing.json");
        let result = validate_path(&missing, &dir);
        assert!(result.is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_check_file_size_allows_small() {
        let tmp = std::env::temp_dir().join("codesynapse_sec_size_small.txt");
        fs::write(&tmp, b"hello").unwrap();
        assert!(check_file_size(&tmp, 1000).is_ok());
        let _ = fs::remove_file(&tmp);
    }

    #[test]
    fn test_check_file_size_blocks_large() {
        let tmp = std::env::temp_dir().join("codesynapse_sec_size_large.txt");
        fs::write(&tmp, b"hello world").unwrap();
        let result = check_file_size(&tmp, 5);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("exceeds"));
        let _ = fs::remove_file(&tmp);
    }

    #[test]
    fn test_check_file_size_missing_file_ok() {
        let path = Path::new("/nonexistent_codesynapse_file_size.txt");
        assert!(check_file_size(path, 100).is_ok());
    }

    #[test]
    fn test_sanitize_label_none() {
        assert_eq!(sanitize_label(None), "");
    }

    #[test]
    fn test_sanitize_label_preserves_normal() {
        assert_eq!(sanitize_label(Some("hello world")), "hello world");
    }

    #[test]
    fn test_sanitize_label_strips_nul() {
        assert_eq!(sanitize_label(Some("foo\x00bar")), "foobar");
    }

    #[test]
    fn test_sanitize_label_strips_control_chars() {
        assert_eq!(sanitize_label(Some("foo\x01\x1fbar\x7f")), "foobar");
    }

    #[test]
    fn test_sanitize_label_caps_length() {
        let long = "a".repeat(300);
        let result = sanitize_label(Some(&long));
        assert_eq!(result.chars().count(), MAX_LABEL_LEN);
    }

    #[test]
    fn test_sanitize_label_empty() {
        assert_eq!(sanitize_label(Some("")), "");
    }

    #[test]
    fn test_sanitize_metadata_none() {
        assert!(sanitize_metadata(None).is_empty());
    }

    #[test]
    fn test_sanitize_metadata_preserves_simple_strings() {
        let mut m = HashMap::new();
        m.insert("key".to_string(), Value::String("value".to_string()));
        let result = sanitize_metadata(Some(&m));
        assert_eq!(result["key"], Value::String("value".to_string()));
    }

    #[test]
    fn test_sanitize_metadata_strips_control_chars_in_values() {
        let mut m = HashMap::new();
        m.insert("k".to_string(), Value::String("foo\x00bar".to_string()));
        let result = sanitize_metadata(Some(&m));
        assert_eq!(result["k"], Value::String("foobar".to_string()));
    }

    #[test]
    fn test_sanitize_metadata_html_escapes_strings() {
        let mut m = HashMap::new();
        m.insert(
            "k".to_string(),
            Value::String("<script>alert(1)</script>".to_string()),
        );
        let result = sanitize_metadata(Some(&m));
        let s = result["k"].as_str().unwrap();
        assert!(!s.contains('<'));
        assert!(s.contains("&lt;"));
    }

    #[test]
    fn test_sanitize_metadata_caps_value_length() {
        let mut m = HashMap::new();
        m.insert("k".to_string(), Value::String("x".repeat(600)));
        let result = sanitize_metadata(Some(&m));
        let s = result["k"].as_str().unwrap();
        assert!(s.chars().count() <= METADATA_MAX_VALUE_LEN);
    }

    #[test]
    fn test_sanitize_metadata_preserves_bool() {
        let mut m = HashMap::new();
        m.insert("k".to_string(), Value::Bool(true));
        let result = sanitize_metadata(Some(&m));
        assert_eq!(result["k"], Value::Bool(true));
    }

    #[test]
    fn test_sanitize_metadata_preserves_null() {
        let mut m = HashMap::new();
        m.insert("k".to_string(), Value::Null);
        let result = sanitize_metadata(Some(&m));
        assert_eq!(result["k"], Value::Null);
    }

    #[test]
    fn test_sanitize_metadata_preserves_number() {
        let mut m = HashMap::new();
        m.insert("k".to_string(), serde_json::json!(42));
        let result = sanitize_metadata(Some(&m));
        assert_eq!(result["k"], serde_json::json!(42));
    }

    #[test]
    fn test_sanitize_metadata_caps_list_items() {
        let mut m = HashMap::new();
        let arr: Vec<Value> = (0..100).map(|i| serde_json::json!(i)).collect();
        m.insert("k".to_string(), Value::Array(arr));
        let result = sanitize_metadata(Some(&m));
        assert_eq!(
            result["k"].as_array().unwrap().len(),
            METADATA_MAX_LIST_ITEMS
        );
    }

    #[test]
    fn test_sanitize_metadata_drops_empty_key() {
        let mut m = HashMap::new();
        m.insert("\x00\x01".to_string(), Value::String("val".to_string()));
        let result = sanitize_metadata(Some(&m));
        assert!(result.is_empty());
    }

    #[test]
    fn test_sanitize_metadata_nested_dict() {
        let mut inner = serde_json::Map::new();
        inner.insert("nested".to_string(), Value::String("<bad>".to_string()));
        let mut m = HashMap::new();
        m.insert("outer".to_string(), Value::Object(inner));
        let result = sanitize_metadata(Some(&m));
        let outer = result["outer"].as_object().unwrap();
        let s = outer["nested"].as_str().unwrap();
        assert!(!s.contains('<'));
        assert!(s.contains("&lt;"));
    }
}
