use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdlRange {
    pub source: String,
    pub start: f64,
    pub end: f64,
    #[serde(default)]
    pub beat: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdlOverlay {
    pub file: String,
    pub start_in_output: f64,
    pub duration: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edl {
    #[serde(default)]
    pub sources: BTreeMap<String, String>,
    #[serde(default)]
    pub ranges: Vec<EdlRange>,
    #[serde(default)]
    pub grade: Option<String>,
    #[serde(default)]
    pub overlays: Option<Vec<EdlOverlay>>,
    #[serde(default)]
    pub subtitles: Option<String>,
}

impl Edl {
    pub fn load_from_file(path: &Path) -> Result<Self> {
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read EDL at {:?}", path))?;
        let edl: Self = serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse EDL JSON at {:?}", path))?;
        Ok(edl)
    }

    pub fn resolve_source_path(&self, source_name: &str, edit_dir: &Path) -> Option<PathBuf> {
        let p_str = self.sources.get(source_name)?;
        let p = Path::new(p_str);
        if p.is_absolute() {
            Some(p.to_path_buf())
        } else {
            Some(edit_dir.join(p))
        }
    }
}

pub fn resolve_path(maybe_path: &str, base: &Path) -> PathBuf {
    let p = Path::new(maybe_path);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        base.join(p)
    }
}
