use crate::error::CodeSynapseError;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::LazyLock;

pub static VIDEO_EXTENSIONS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    [
        ".mp4", ".mov", ".webm", ".mkv", ".avi", ".m4v", ".mp3", ".wav", ".m4a", ".ogg",
    ]
    .into_iter()
    .collect()
});

static URL_PREFIXES: &[&str] = &["http://", "https://", "www."];
static DEFAULT_MODEL: &str = "base";
static TRANSCRIPTS_DIR: &str = "codesynapse-out/transcripts";
static FALLBACK_PROMPT: &str = "Use proper punctuation and paragraph breaks.";

fn model_name() -> String {
    env::var("CODESYNAPSE_WHISPER_MODEL").unwrap_or_else(|_| DEFAULT_MODEL.to_string())
}

pub fn is_url(path: &str) -> bool {
    URL_PREFIXES.iter().any(|p| path.starts_with(p))
}

fn url_hash(url: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(url.as_bytes());
    let result = hasher.finalize();
    format!("{:x}", result)[..12].to_string()
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

pub fn download_audio(url: &str, output_dir: &Path) -> Result<PathBuf, CodeSynapseError> {
    let hash = url_hash(url);

    for ext in [".m4a", ".opus", ".mp3", ".ogg", ".wav", ".webm"] {
        let candidate = output_dir.join(format!("yt_{}{}", hash, ext));
        if candidate.exists() {
            eprintln!(
                "  cached audio: {}",
                candidate.file_name().unwrap_or_default().to_string_lossy()
            );
            return Ok(candidate);
        }
    }

    std::fs::create_dir_all(output_dir).map_err(CodeSynapseError::Io)?;

    let exe = find_exe("yt-dlp").ok_or_else(|| {
        CodeSynapseError::Validation(
            "yt-dlp is required for URL download. Install: pip install yt-dlp".to_string(),
        )
    })?;

    let out_template = output_dir.join(format!("yt_{}.%(ext)s", hash));
    let out_template_str = out_template.to_string_lossy().to_string();
    let display_url = &url[..url.len().min(80)];
    eprintln!("  downloading audio: {} ...", display_url);

    let result = Command::new(&exe)
        .args([
            "-f",
            "bestaudio[ext=m4a]/bestaudio/best",
            "-o",
            &out_template_str,
            "--quiet",
            "--no-warnings",
            "--no-playlist",
            url,
        ])
        .output()
        .map_err(CodeSynapseError::Io)?;

    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        let msg = &stderr[..stderr.len().min(800)];
        return Err(CodeSynapseError::Validation(format!(
            "yt-dlp failed: {}",
            msg
        )));
    }

    for ext in [".m4a", ".opus", ".mp3", ".ogg", ".wav", ".webm"] {
        let candidate = output_dir.join(format!("yt_{}{}", hash, ext));
        if candidate.exists() {
            return Ok(candidate);
        }
    }

    if let Ok(entries) = std::fs::read_dir(output_dir) {
        let prefix = format!("yt_{}", hash);
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with(&prefix) {
                return Ok(entry.path());
            }
        }
    }

    Err(CodeSynapseError::Validation(format!(
        "yt-dlp ran but no output file found for {}",
        url
    )))
}

fn build_whisper_prompt_inner(
    god_nodes: &[serde_json::Value],
    env_override: Option<&str>,
) -> String {
    if god_nodes.is_empty() {
        return FALLBACK_PROMPT.to_string();
    }

    if let Some(ov) = env_override {
        return ov.to_string();
    }

    let labels: Vec<&str> = god_nodes
        .iter()
        .filter_map(|n| n.get("label").and_then(|v| v.as_str()))
        .filter(|s| !s.is_empty())
        .take(5)
        .collect();

    if labels.is_empty() {
        return FALLBACK_PROMPT.to_string();
    }

    let topics = labels.join(", ");
    format!(
        "Technical discussion about {}. Use proper punctuation and paragraph breaks.",
        topics
    )
}

pub fn build_whisper_prompt(god_nodes: &[serde_json::Value]) -> String {
    let env_override = env::var("CODESYNAPSE_WHISPER_PROMPT").ok();
    build_whisper_prompt_inner(god_nodes, env_override.as_deref())
}

fn run_whisper(
    audio_path: &Path,
    model: &str,
    initial_prompt: Option<&str>,
) -> Result<String, CodeSynapseError> {
    let exe = find_exe("faster-whisper")
        .or_else(|| find_exe("whisper"))
        .ok_or_else(|| {
            CodeSynapseError::Validation(
                "faster-whisper or whisper CLI required. \
                 Install: pip install faster-whisper"
                    .to_string(),
            )
        })?;

    let mut cmd = Command::new(&exe);
    cmd.arg(audio_path.to_string_lossy().as_ref());
    cmd.args(["--model", model]);
    if let Some(prompt) = initial_prompt {
        cmd.args(["--initial_prompt", prompt]);
    }
    cmd.args(["--output_format", "txt"]);
    let out_dir = audio_path.parent().unwrap_or(Path::new("."));
    cmd.arg("--output_dir").arg(out_dir);

    let result = cmd.output().map_err(CodeSynapseError::Io)?;
    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        let msg = &stderr[..stderr.len().min(800)];
        return Err(CodeSynapseError::Validation(format!(
            "whisper failed: {}",
            msg
        )));
    }

    let txt_path = out_dir.join(format!(
        "{}.txt",
        audio_path.file_stem().unwrap_or_default().to_string_lossy()
    ));
    if txt_path.exists() {
        return std::fs::read_to_string(&txt_path).map_err(CodeSynapseError::Io);
    }

    Ok(String::from_utf8_lossy(&result.stdout).to_string())
}

pub fn transcribe_with(
    video_path: &Path,
    output_dir: Option<&Path>,
    initial_prompt: Option<&str>,
    force: bool,
    whisper_fn: impl Fn(&Path, &str, Option<&str>) -> Result<String, CodeSynapseError>,
) -> Result<PathBuf, CodeSynapseError> {
    let out_dir = output_dir
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from(TRANSCRIPTS_DIR));
    std::fs::create_dir_all(&out_dir).map_err(CodeSynapseError::Io)?;

    let audio_path = if is_url(&video_path.to_string_lossy()) {
        download_audio(&video_path.to_string_lossy(), &out_dir.join("downloads"))?
    } else {
        video_path.to_path_buf()
    };

    let stem = audio_path.file_stem().unwrap_or_default().to_string_lossy();
    let transcript_path = out_dir.join(format!("{}.txt", stem));

    if transcript_path.exists() && !force {
        return Ok(transcript_path);
    }

    let model = model_name();
    let prompt = initial_prompt.unwrap_or(FALLBACK_PROMPT);
    eprintln!(
        "  transcribing {} (model={}) ...",
        audio_path.file_name().unwrap_or_default().to_string_lossy(),
        model
    );

    let text = whisper_fn(&audio_path, &model, Some(prompt))?;
    let lines: Vec<&str> = text
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect();
    let transcript = lines.join("\n");

    std::fs::write(&transcript_path, &transcript).map_err(CodeSynapseError::Io)?;
    eprintln!(
        "  transcript saved -> {} ({} segments)",
        transcript_path.display(),
        lines.len()
    );

    Ok(transcript_path)
}

pub fn transcribe(
    video_path: &Path,
    output_dir: Option<&Path>,
    initial_prompt: Option<&str>,
    force: bool,
) -> Result<PathBuf, CodeSynapseError> {
    transcribe_with(video_path, output_dir, initial_prompt, force, run_whisper)
}

pub fn transcribe_all(
    video_files: &[String],
    output_dir: Option<&Path>,
    initial_prompt: Option<&str>,
) -> Vec<String> {
    if video_files.is_empty() {
        return vec![];
    }

    video_files
        .iter()
        .filter_map(
            |vf| match transcribe(Path::new(vf), output_dir, initial_prompt, false) {
                Ok(p) => Some(p.to_string_lossy().to_string()),
                Err(e) => {
                    eprintln!("  warning: could not transcribe {}: {}", vf, e);
                    None
                }
            },
        )
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn tmp() -> TempDir {
        tempfile::tempdir().unwrap()
    }

    // VIDEO_EXTENSIONS

    #[test]
    fn test_video_extensions_set() {
        assert!(VIDEO_EXTENSIONS.contains(".mp4"));
        assert!(VIDEO_EXTENSIONS.contains(".mp3"));
        assert!(VIDEO_EXTENSIONS.contains(".wav"));
        assert!(VIDEO_EXTENSIONS.contains(".mov"));
        assert!(!VIDEO_EXTENSIONS.contains(".py"));
        assert!(!VIDEO_EXTENSIONS.contains(".rs"));
    }

    // build_whisper_prompt

    #[test]
    fn test_build_whisper_prompt_no_nodes() {
        let prompt = build_whisper_prompt(&[]);
        assert!(!prompt.is_empty());
        assert!(prompt.to_lowercase().contains("punctuation"));
    }

    #[test]
    fn test_build_whisper_prompt_env_override() {
        let nodes = vec![
            serde_json::json!({"label": "Python"}),
            serde_json::json!({"label": "FastAPI"}),
        ];
        let prompt = build_whisper_prompt_inner(&nodes, Some("Custom domain hint."));
        assert_eq!(prompt, "Custom domain hint.");
    }

    #[test]
    fn test_build_whisper_prompt_returns_topic_string() {
        let nodes = vec![
            serde_json::json!({"label": "neural networks"}),
            serde_json::json!({"label": "transformers"}),
            serde_json::json!({"label": "attention"}),
        ];
        let prompt = build_whisper_prompt_inner(&nodes, None);
        let lower = prompt.to_lowercase();
        assert!(lower.contains("neural networks") || lower.contains("transformers"));
        assert!(lower.contains("punctuation"));
    }

    #[test]
    fn test_build_whisper_prompt_nodes_without_labels() {
        let nodes = vec![
            serde_json::json!({"id": "1"}),
            serde_json::json!({"id": "2", "label": ""}),
        ];
        let prompt = build_whisper_prompt_inner(&nodes, None);
        assert!(!prompt.is_empty());
    }

    // transcribe (via transcribe_with with injectable fn)

    #[test]
    fn test_transcribe_uses_cache() {
        let dir = tmp();
        let video = dir.path().join("lecture.mp4");
        fs::write(&video, b"fake").unwrap();
        let out_dir = dir.path().join("transcripts");
        fs::create_dir_all(&out_dir).unwrap();
        let cached = out_dir.join("lecture.txt");
        fs::write(&cached, "Cached transcript content.").unwrap();

        let called = std::cell::Cell::new(false);
        let result = transcribe_with(&video, Some(&out_dir), None, false, |_, _, _| {
            called.set(true);
            Ok("should not be called".to_string())
        })
        .unwrap();

        assert_eq!(result, cached);
        assert!(!called.get(), "whisper should not run when cache hit");
    }

    #[test]
    fn test_transcribe_force_reruns() {
        let dir = tmp();
        let video = dir.path().join("talk.mp4");
        fs::write(&video, b"fake").unwrap();
        let out_dir = dir.path().join("transcripts");
        fs::create_dir_all(&out_dir).unwrap();
        fs::write(out_dir.join("talk.txt"), "Old transcript.").unwrap();

        let result = transcribe_with(&video, Some(&out_dir), None, true, |_, _, _| {
            Ok("New transcript segment.".to_string())
        })
        .unwrap();

        assert_eq!(
            fs::read_to_string(&result).unwrap(),
            "New transcript segment."
        );
    }

    #[test]
    fn test_transcribe_missing_faster_whisper() {
        let dir = tmp();
        let video = dir.path().join("clip.mp4");
        fs::write(&video, b"fake").unwrap();
        let out_dir = dir.path().join("out");

        let result = transcribe_with(&video, Some(&out_dir), None, false, |_, _, _| {
            Err(CodeSynapseError::Validation(
                "faster-whisper not installed".to_string(),
            ))
        });

        assert!(result.is_err());
    }

    // transcribe_all

    #[test]
    fn test_transcribe_all_empty() {
        let results = transcribe_all(&[], None, None);
        assert!(results.is_empty());
    }

    #[test]
    fn test_transcribe_all_uses_cache() {
        let dir = tmp();
        let video = dir.path().join("lecture.mp4");
        fs::write(&video, b"fake").unwrap();
        let out_dir = dir.path().join("transcripts");
        fs::create_dir_all(&out_dir).unwrap();
        let cached = out_dir.join("lecture.txt");
        fs::write(&cached, "Cached.").unwrap();

        let results = transcribe_all(&[video.to_string_lossy().to_string()], Some(&out_dir), None);
        assert_eq!(results.len(), 1);
        assert!(results[0].contains("lecture.txt"));
    }

    #[test]
    fn test_transcribe_all_skips_failed() {
        let dir = tmp();
        let video = dir.path().join("broken.mp4");
        fs::write(&video, b"fake").unwrap();
        let out_dir = dir.path().join("out");

        // No cache exists and transcribe will try real whisper (which won't be found in test env)
        // transcribe_all catches errors and returns empty
        let results = transcribe_all(&[video.to_string_lossy().to_string()], Some(&out_dir), None);
        assert!(results.is_empty());
    }
}
