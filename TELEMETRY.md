# Telemetry

Codesynapse collects **opt-in** anonymous usage statistics.

**Telemetry is off by default.** Nothing leaves your machine until you explicitly enable it.

---

## Enabling / disabling

```bash
codesynapse telemetry on      # enable
codesynapse telemetry off     # disable + delete local queue
codesynapse telemetry status  # show current state
```

Environment overrides (take precedence over stored config):

| Variable | Effect |
|---|---|
| `DO_NOT_TRACK=1` | Always off, no config read |
| `CODESYNAPSE_TELEMETRY=0` | Off |
| `CODESYNAPSE_TELEMETRY=1` | On |
| `CODESYNAPSE_TELEMETRY_ENDPOINT=<url>` | Override ingest endpoint (self-hosters) |

---

## What is collected

### Usage rollups
Aggregated **per tool per day** — never per individual query.

| Field | Example | Notes |
|---|---|---|
| `tool` | `codesynapse_context` | Tool name only, not query content |
| `call_count` | `42` | Calls that day |
| `error_count` | `0` | Calls that returned an error |
| `saved_chars_bucket` | `10k-100k` | Coarse bucket of chars saved |
| `day` | `2026-06-20` | UTC date |

### Lifecycle events
Fired once per action: `install`, `index_complete`, `uninstall`.

`index_complete` includes:

| Field | Example | Notes |
|---|---|---|
| `languages` | `["rust","python"]` | Grammar names only |
| `node_count_bucket` | `1k-10k` | Coarse bucket |
| `embeddings_enabled` | `true` | Whether local model loaded |
| `index_duration_bucket` | `30s-2m` | Coarse bucket |

### Envelope (sent with every batch)

| Field | Example |
|---|---|
| `codesynapse_version` | `0.1.0` |
| `os` | `linux` / `macos` / `windows` |
| `arch` | `x86_64` / `aarch64` |
| `machine_id` | Random UUID, generated once on first `telemetry on` |

---

## What is never collected

- Query strings or natural-language inputs
- File paths, file names, directory names
- Symbol names, class names, function names
- Source code or any file content
- IP addresses (Cloudflare Workers does not log these)
- Exact counts (only coarse buckets)

The ingest worker validates and rejects any value that doesn't match a pre-defined allowlist before writing to the database. Free-form strings cannot enter the pipeline.

---

## How it works

1. Tool calls are counted **in memory only** — no disk I/O on the hot path.
2. On MCP server exit, in-memory counts are appended to `~/.codesynapse/telemetry-queue.jsonl`.
3. On next MCP server start, completed UTC days are sent to the ingest endpoint in a background thread. Today's partial counts stay local.
4. The queue is capped at 256 KB. If full, oldest entries are dropped first.
5. Network failures are silent — no retries, no errors surfaced to the agent.

---

## Data storage

Events are written to a Cloudflare D1 (SQLite) database hosted in the APAC region. The ingest worker is open source alongside this repository at `telemetry-worker/`.

Data is retained for 12 months, then deleted.

---

## Local files

| Path | Purpose |
|---|---|
| `~/.codesynapse/telemetry.json` | Config: enabled state + machine ID |
| `~/.codesynapse/telemetry-queue.jsonl` | Local buffer of unsent daily rollups |

Running `codesynapse telemetry off` deletes the queue file and sets `enabled: false` in the config.
