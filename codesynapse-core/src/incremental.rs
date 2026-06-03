use crate::cache::FileCache;
use crate::error::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const MANIFEST_FILE: &str = "manifest.json";

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Manifest {
    pub files: HashMap<String, String>,
}

impl Manifest {
    pub fn load(output_dir: &Path) -> Option<Self> {
        let path = output_dir.join(MANIFEST_FILE);
        let text = std::fs::read_to_string(&path).ok()?;
        serde_json::from_str(&text).ok()
    }

    pub fn save(&self, output_dir: &Path) -> Result<()> {
        std::fs::create_dir_all(output_dir)?;
        let path = output_dir.join(MANIFEST_FILE);
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;
        Ok(())
    }
}

pub struct IncrementalScan {
    pub is_incremental: bool,
    pub changed_files: Vec<PathBuf>,
    pub unchanged_files: Vec<PathBuf>,
}

pub struct IncrementalBuilder {
    output_dir: PathBuf,
}

impl IncrementalBuilder {
    pub fn new(output_dir: PathBuf) -> Self {
        Self { output_dir }
    }

    pub fn scan(&self, files: &[PathBuf]) -> IncrementalScan {
        let manifest = Manifest::load(&self.output_dir);
        let is_incremental = manifest.is_some();
        let prev = manifest.unwrap_or_default();

        let mut changed = Vec::new();
        let mut unchanged = Vec::new();

        for path in files {
            let key = path.to_string_lossy().to_string();
            let current_hash = std::fs::read(path)
                .map(|b| FileCache::compute_hash(&b))
                .unwrap_or_default();
            match prev.files.get(&key) {
                Some(h) if h == &current_hash => unchanged.push(path.clone()),
                _ => changed.push(path.clone()),
            }
        }

        IncrementalScan {
            is_incremental,
            changed_files: changed,
            unchanged_files: unchanged,
        }
    }

    pub fn write_manifest(&self, files: &[PathBuf]) -> Result<()> {
        let mut manifest = Manifest::default();
        for path in files {
            let key = path.to_string_lossy().to_string();
            let hash = std::fs::read(path)
                .map(|b| FileCache::compute_hash(&b))
                .unwrap_or_default();
            manifest.files.insert(key, hash);
        }
        manifest.save(&self.output_dir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write_file(dir: &Path, name: &str, content: &str) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, content).unwrap();
        p
    }

    #[test]
    fn incremental_manifest_written() {
        let tmp = tempdir().unwrap();
        let out = tmp.path().join("out");
        std::fs::create_dir_all(&out).unwrap();

        let src = tmp.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        let f = write_file(&src, "foo.py", "def foo(): pass");

        let builder = IncrementalBuilder::new(out.clone());
        assert!(!out.join("manifest.json").exists());

        builder.write_manifest(&[f]).unwrap();

        assert!(out.join("manifest.json").exists());
        let m = Manifest::load(&out).unwrap();
        assert_eq!(m.files.len(), 1);
    }

    #[test]
    fn incremental_mode_detected_via_manifest() {
        let tmp = tempdir().unwrap();
        let out = tmp.path().join("out");
        std::fs::create_dir_all(&out).unwrap();

        let src = tmp.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        let f = write_file(&src, "intro.py", "x = 1");

        let builder = IncrementalBuilder::new(out.clone());
        builder.write_manifest(std::slice::from_ref(&f)).unwrap();

        let scan = builder.scan(&[f]);
        assert!(scan.is_incremental);
        assert_eq!(scan.unchanged_files.len(), 1);
        assert!(scan.changed_files.is_empty());
    }

    #[test]
    fn incremental_no_manifest_full_scan() {
        let tmp = tempdir().unwrap();
        let out = tmp.path().join("out");
        std::fs::create_dir_all(&out).unwrap();

        let src = tmp.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        let f = write_file(&src, "api.py", "def api(): pass");

        let builder = IncrementalBuilder::new(out.clone());
        let scan = builder.scan(&[f]);

        assert!(!scan.is_incremental);
        assert_eq!(scan.changed_files.len(), 1);
        assert!(scan.unchanged_files.is_empty());
    }
}
