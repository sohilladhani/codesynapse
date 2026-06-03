use std::collections::HashMap;
use std::io::Write as IoWrite;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

pub const ENDPOINT: &str = "https://codesynapse-telemetry.sohilladhani.workers.dev/v1/events";
const SCHEMA_VERSION: u32 = 1;
const MAX_QUEUE_BYTES: u64 = 256 * 1024;

// ── config ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TelemetryConfig {
    pub enabled: bool,
    pub machine_id: String,
    pub consent_source: String,
    pub updated_at: String,
}

#[derive(Debug)]
pub struct TelemetryStatus {
    pub enabled: bool,
    pub decided_by: &'static str,
    pub machine_id: Option<String>,
    pub config_path: PathBuf,
}

// ── in-memory aggregation ────────────────────────────────────────────────────

#[derive(Default)]
struct CountEntry {
    calls: u64,
    errors: u64,
    saved_chars: u64,
}

#[derive(Default)]
struct Inner {
    // key: (utc_day, tool_name)
    counts: HashMap<(String, String), CountEntry>,
    lifecycle: Vec<serde_json::Value>,
}

// ── public handle ────────────────────────────────────────────────────────────

pub struct Telemetry {
    global_dir: PathBuf,
    inner: Mutex<Inner>,
}

impl Telemetry {
    pub fn new(global_dir: PathBuf) -> Self {
        Self {
            global_dir,
            inner: Mutex::new(Inner::default()),
        }
    }

    // ── consent ──────────────────────────────────────────────────────────────

    pub fn status(&self) -> TelemetryStatus {
        let config = self.read_config();
        let machine_id = config.as_ref().map(|c| c.machine_id.clone());
        let config_path = self.config_path();

        if env_flag("DO_NOT_TRACK") {
            return TelemetryStatus {
                enabled: false,
                decided_by: "DO_NOT_TRACK",
                machine_id,
                config_path,
            };
        }
        if let Ok(v) = std::env::var("CODESYNAPSE_TELEMETRY") {
            let on = !matches!(v.as_str(), "0" | "false" | "off");
            return TelemetryStatus {
                enabled: on,
                decided_by: "CODESYNAPSE_TELEMETRY",
                machine_id,
                config_path,
            };
        }
        match config {
            Some(c) => TelemetryStatus {
                enabled: c.enabled,
                decided_by: "config",
                machine_id: Some(c.machine_id),
                config_path,
            },
            None => TelemetryStatus {
                enabled: false,
                decided_by: "default",
                machine_id: None,
                config_path,
            },
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.status().enabled
    }

    pub fn set_enabled(&self, enabled: bool) {
        let existing = self.read_config();
        let machine_id = existing
            .as_ref()
            .map(|c| c.machine_id.clone())
            .unwrap_or_else(new_uuid);
        let config = TelemetryConfig {
            enabled,
            machine_id,
            consent_source: "cli".into(),
            updated_at: utc_now_iso(),
        };
        self.write_config(&config);
        if !enabled {
            let _ = std::fs::remove_file(self.queue_path());
        }
    }

    // ── recording (hot path — just in-memory) ────────────────────────────────

    pub fn record_usage(&self, tool: &str, saved_chars: usize, ok: bool) {
        if !self.is_enabled() {
            return;
        }
        let day = utc_day();
        let mut inner = self.inner.lock().unwrap();
        let entry = inner.counts.entry((day, tool.to_string())).or_default();
        entry.calls += 1;
        if !ok {
            entry.errors += 1;
        }
        entry.saved_chars += saved_chars as u64;
    }

    pub fn record_lifecycle(&self, event: &str, props: serde_json::Value) {
        if !self.is_enabled() {
            return;
        }
        let line = serde_json::json!({
            "type": "lifecycle",
            "event": event,
            "ts": utc_now_iso(),
            "props": props,
        });
        self.inner.lock().unwrap().lifecycle.push(line);
    }

    // ── persist (sync, call at process exit) ────────────────────────────────

    pub fn persist_sync(&self) {
        if !self.is_enabled() {
            return;
        }
        let mut inner = self.inner.lock().unwrap();
        if inner.counts.is_empty() && inner.lifecycle.is_empty() {
            return;
        }

        let mut lines: Vec<serde_json::Value> = Vec::new();

        for ((day, tool), entry) in inner.counts.drain() {
            lines.push(serde_json::json!({
                "type": "rollup",
                "day": day,
                "tool": tool,
                "calls": entry.calls,
                "errors": entry.errors,
                "saved_chars": entry.saved_chars,
            }));
        }
        for ev in inner.lifecycle.drain(..) {
            lines.push(ev);
        }

        let _ = self.append_to_queue(&lines);
    }

    // ── flush (background thread, fire-and-forget) ───────────────────────────

    pub fn flush_bg(&self) {
        if !self.is_enabled() {
            return;
        }
        let config = match self.read_config() {
            Some(c) if c.enabled => c,
            _ => return,
        };
        let queue_path = self.queue_path();
        if !queue_path.exists() {
            return;
        }

        let endpoint = std::env::var("CODESYNAPSE_TELEMETRY_ENDPOINT")
            .unwrap_or_else(|_| ENDPOINT.to_string());

        std::thread::spawn(move || {
            let _ = do_flush(&queue_path, &config.machine_id, &endpoint);
        });
    }

    // ── paths ────────────────────────────────────────────────────────────────

    fn config_path(&self) -> PathBuf {
        self.global_dir.join("telemetry.json")
    }

    fn queue_path(&self) -> PathBuf {
        self.global_dir.join("telemetry-queue.jsonl")
    }

    fn read_config(&self) -> Option<TelemetryConfig> {
        let text = std::fs::read_to_string(self.config_path()).ok()?;
        serde_json::from_str(&text).ok()
    }

    fn write_config(&self, config: &TelemetryConfig) {
        let _ = std::fs::create_dir_all(&self.global_dir);
        if let Ok(text) = serde_json::to_string_pretty(config) {
            let _ = std::fs::write(self.config_path(), text + "\n");
        }
    }

    fn append_to_queue(&self, lines: &[serde_json::Value]) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.global_dir)?;
        let path = self.queue_path();

        // Cap at MAX_QUEUE_BYTES — drop oldest lines if needed.
        let payload = lines
            .iter()
            .filter_map(|l| serde_json::to_string(l).ok())
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";

        let existing_len = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        if existing_len + payload.len() as u64 > MAX_QUEUE_BYTES {
            let existing = std::fs::read_to_string(&path).unwrap_or_default();
            let combined = existing + &payload;
            let trimmed = trim_queue(&combined, MAX_QUEUE_BYTES as usize);
            return std::fs::write(&path, trimmed);
        }

        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        f.write_all(payload.as_bytes())?;
        Ok(())
    }
}

// ── flush logic (runs in background thread) ──────────────────────────────────

fn do_flush(queue_path: &Path, machine_id: &str, endpoint: &str) -> Option<()> {
    let text = std::fs::read_to_string(queue_path).ok()?;
    let today = utc_day();

    let mut sendable: Vec<serde_json::Value> = Vec::new();
    let mut keep: Vec<serde_json::Value> = Vec::new();

    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let is_lifecycle = v.get("type").and_then(|t| t.as_str()) == Some("lifecycle");
        let day = v.get("day").and_then(|d| d.as_str()).unwrap_or("");

        if is_lifecycle || (!day.is_empty() && day < today.as_str()) {
            sendable.push(v);
        } else {
            keep.push(v);
        }
    }

    if sendable.is_empty() {
        return Some(());
    }

    let events: Vec<serde_json::Value> = sendable
        .iter()
        .map(|v| {
            if v.get("type").and_then(|t| t.as_str()) == Some("rollup") {
                let tool = v.get("tool").and_then(|t| t.as_str()).unwrap_or("");
                let calls = v.get("calls").and_then(|c| c.as_u64()).unwrap_or(0);
                let errors = v.get("errors").and_then(|e| e.as_u64()).unwrap_or(0);
                let saved = v.get("saved_chars").and_then(|s| s.as_u64()).unwrap_or(0);
                let day = v.get("day").and_then(|d| d.as_str()).unwrap_or("");
                serde_json::json!({
                    "event": "usage_rollup",
                    "day": day,
                    "tool": tool,
                    "call_count": calls,
                    "error_count": errors,
                    "saved_chars_bucket": bucket_chars(saved),
                })
            } else {
                let event = v.get("event").and_then(|e| e.as_str()).unwrap_or("unknown");
                let ts = v.get("ts").and_then(|t| t.as_str()).unwrap_or("");
                let props = v.get("props").cloned().unwrap_or(serde_json::json!({}));
                serde_json::json!({
                    "event": event,
                    "ts": ts,
                    "props": props,
                })
            }
        })
        .collect();

    let envelope = serde_json::json!({
        "schema_version": SCHEMA_VERSION,
        "machine_id": machine_id,
        "codesynapse_version": env!("CARGO_PKG_VERSION"),
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "events": events,
    });

    let body = serde_json::to_string(&envelope).ok()?;
    let result = ureq::post(endpoint)
        .set("Content-Type", "application/json")
        .timeout(std::time::Duration::from_secs(5))
        .send_string(&body);

    // Any response or error → remove sent entries from queue.
    // We treat failure as "drop and move on" to avoid retrying indefinitely.
    let _ = result;

    let new_queue = keep
        .iter()
        .filter_map(|v| serde_json::to_string(v).ok())
        .collect::<Vec<_>>()
        .join("\n");

    if new_queue.is_empty() {
        let _ = std::fs::remove_file(queue_path);
    } else {
        let _ = std::fs::write(queue_path, new_queue + "\n");
    }

    Some(())
}

// ── helpers ───────────────────────────────────────────────────────────────────

fn utc_day() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let days = secs / 86400;
    // Convert day count since epoch to YYYY-MM-DD.
    // Using a simple algorithm (Tomohiko Sakamoto / civil date).
    let z = days as i64 + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{:04}-{:02}-{:02}", y, m, d)
}

fn utc_now_iso() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let day = utc_day();
    let h = (secs % 86400) / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    format!("{}T{:02}:{:02}:{:02}Z", day, h, m, s)
}

fn new_uuid() -> String {
    // UUID v4 using random bytes from /dev/urandom or a fallback.
    let mut bytes = [0u8; 16];
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        use std::io::Read;
        let _ = f.read_exact(&mut bytes);
    } else {
        // Fallback: mix of timestamp + pid.
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .subsec_nanos();
        let pid = std::process::id();
        bytes[..4].copy_from_slice(&ts.to_le_bytes());
        bytes[4..8].copy_from_slice(&pid.to_le_bytes());
    }
    // Set version (4) and variant bits.
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3],
        bytes[4], bytes[5],
        bytes[6], bytes[7],
        bytes[8], bytes[9],
        bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
    )
}

fn env_flag(name: &str) -> bool {
    !matches!(
        std::env::var(name).ok().as_deref(),
        Some("") | Some("0") | Some("false") | None
    )
}

fn trim_queue(combined: &str, max_bytes: usize) -> String {
    if combined.len() <= max_bytes {
        return combined.to_string();
    }
    let excess = combined.len() - max_bytes;
    let trimmed = &combined[excess..];
    // Skip partial first line.
    if let Some(pos) = trimmed.find('\n') {
        trimmed[pos + 1..].to_string()
    } else {
        String::new()
    }
}

pub fn bucket_chars(n: u64) -> &'static str {
    if n < 1_000 {
        "<1k"
    } else if n < 10_000 {
        "1k-10k"
    } else if n < 100_000 {
        "10k-100k"
    } else {
        "100k+"
    }
}

pub fn bucket_count(n: usize) -> &'static str {
    if n < 100 {
        "<100"
    } else if n < 1_000 {
        "100-1k"
    } else if n < 10_000 {
        "1k-10k"
    } else if n < 100_000 {
        "10k-100k"
    } else {
        "100k+"
    }
}

pub fn bucket_duration_ms(ms: u64) -> &'static str {
    if ms < 10_000 {
        "<10s"
    } else if ms < 60_000 {
        "10-60s"
    } else if ms < 300_000 {
        "1-5m"
    } else {
        "5m+"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_day_format() {
        let d = utc_day();
        assert_eq!(d.len(), 10);
        assert!(d.contains('-'));
    }

    #[test]
    fn uuid_format() {
        let u = new_uuid();
        let parts: Vec<&str> = u.split('-').collect();
        assert_eq!(parts.len(), 5);
        assert_eq!(parts[0].len(), 8);
    }

    #[test]
    fn bucket_chars_boundaries() {
        assert_eq!(bucket_chars(0), "<1k");
        assert_eq!(bucket_chars(999), "<1k");
        assert_eq!(bucket_chars(1_000), "1k-10k");
        assert_eq!(bucket_chars(100_000), "100k+");
    }

    #[test]
    fn disabled_by_default_no_config() {
        let dir = tempfile::tempdir().unwrap();
        let t = Telemetry::new(dir.path().to_path_buf());
        assert!(!t.is_enabled());
    }

    #[test]
    fn set_enabled_writes_config() {
        let dir = tempfile::tempdir().unwrap();
        let t = Telemetry::new(dir.path().to_path_buf());
        t.set_enabled(true);
        assert!(t.is_enabled());
        let config = t.read_config().unwrap();
        assert!(config.enabled);
        assert_eq!(config.consent_source, "cli");
    }

    #[test]
    fn record_and_persist() {
        let dir = tempfile::tempdir().unwrap();
        let t = Telemetry::new(dir.path().to_path_buf());
        t.set_enabled(true);
        t.record_usage("codesynapse_context", 5000, true);
        t.record_usage("codesynapse_context", 3000, false);
        t.persist_sync();
        let queue = std::fs::read_to_string(t.queue_path()).unwrap();
        assert!(queue.contains("codesynapse_context"));
        assert!(queue.contains("\"calls\":2"));
        assert!(queue.contains("\"errors\":1"));
    }

    #[test]
    fn disabled_no_record() {
        let dir = tempfile::tempdir().unwrap();
        let t = Telemetry::new(dir.path().to_path_buf());
        // default disabled — record_usage is a no-op
        t.record_usage("codesynapse_context", 5000, true);
        t.persist_sync();
        assert!(!t.queue_path().exists());
    }
}
