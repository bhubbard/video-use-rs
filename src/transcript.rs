use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

fn default_word_type() -> String {
    "word".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScribeWord {
    #[serde(rename = "type", default = "default_word_type")]
    pub word_type: String,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub start: Option<f64>,
    #[serde(default)]
    pub end: Option<f64>,
    #[serde(default)]
    pub speaker_id: Option<serde_json::Value>,
}

impl ScribeWord {
    pub fn new_word(text: impl Into<String>, start: f64, end: f64) -> Self {
        Self {
            word_type: "word".to_string(),
            text: Some(text.into()),
            start: Some(start),
            end: Some(end),
            speaker_id: None,
        }
    }

    pub fn speaker_str(&self) -> Option<String> {
        self.speaker_id.as_ref().map(|v| match v {
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Number(n) => n.to_string(),
            _ => v.to_string(),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ScribeTranscript {
    #[serde(default)]
    pub words: Vec<ScribeWord>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Phrase {
    pub start: f64,
    pub end: f64,
    pub text: String,
    pub speaker_id: Option<String>,
}

pub fn format_time(seconds: f64) -> String {
    format!("{:06.2}", seconds)
}

pub fn format_duration(seconds: f64) -> String {
    if seconds < 60.0 {
        format!("{:.1}s", seconds)
    } else {
        let m = (seconds / 60.0).floor() as u64;
        let s = seconds - (m as f64 * 60.0);
        format!("{}m {:04.1}s", m, s)
    }
}

pub fn group_into_phrases(words: &[ScribeWord], silence_threshold: f64) -> Vec<Phrase> {
    let mut phrases: Vec<Phrase> = Vec::new();
    let mut current_words: Vec<ScribeWord> = Vec::new();
    let mut current_start: Option<f64> = None;
    let mut current_speaker: Option<String> = None;

    let flush = |current_words: &mut Vec<ScribeWord>,
                 current_start: &mut Option<f64>,
                 current_speaker: &mut Option<String>,
                 phrases: &mut Vec<Phrase>| {
        if current_words.is_empty() {
            return;
        }

        let mut text_parts: Vec<String> = Vec::new();
        for w in current_words.iter() {
            let t = &w.word_type;
            let raw = w.text.as_deref().unwrap_or("").trim();
            if raw.is_empty() {
                continue;
            }
            if t == "audio_event" {
                if !raw.starts_with('(') {
                    text_parts.push(format!("({raw})"));
                } else {
                    text_parts.push(raw.to_string());
                }
            } else {
                text_parts.push(raw.to_string());
            }
        }

        if text_parts.is_empty() {
            current_words.clear();
            *current_start = None;
            *current_speaker = None;
            return;
        }

        let mut text = text_parts.join(" ");
        text = text
            .replace(" ,", ",")
            .replace(" .", ".")
            .replace(" ?", "?")
            .replace(" !", "!");

        let last_word = current_words.last().unwrap();
        let end_time = last_word.end.unwrap_or_else(|| {
            last_word.start.unwrap_or(current_start.unwrap_or(0.0))
        });

        phrases.push(Phrase {
            start: current_start.unwrap_or(0.0),
            end: end_time,
            text,
            speaker_id: current_speaker.take(),
        });

        current_words.clear();
        *current_start = None;
    };

    let mut prev_end: Option<f64> = None;

    for w in words {
        let t = &w.word_type;
        if t == "spacing" {
            if let (Some(start), Some(end)) = (w.start, w.end) {
                let gap = end - start;
                if gap >= silence_threshold {
                    flush(&mut current_words, &mut current_start, &mut current_speaker, &mut phrases);
                }
            }
            continue;
        }

        let start = match w.start {
            Some(s) => s,
            None => continue,
        };
        let speaker = w.speaker_str();

        if let (Some(cur_spk), Some(ref spk)) = (&current_speaker, &speaker) {
            if cur_spk != spk {
                flush(&mut current_words, &mut current_start, &mut current_speaker, &mut phrases);
            }
        }

        if let Some(p_end) = prev_end {
            if start - p_end >= silence_threshold {
                flush(&mut current_words, &mut current_start, &mut current_speaker, &mut phrases);
            }
        }

        if current_start.is_none() {
            current_start = Some(start);
            current_speaker = speaker;
        }

        prev_end = Some(w.end.unwrap_or(start));
        current_words.push(w.clone());
    }

    flush(&mut current_words, &mut current_start, &mut current_speaker, &mut phrases);

    phrases
}

pub fn pack_one_file(json_path: &Path, silence_threshold: f64) -> Result<(String, f64, Vec<Phrase>)> {
    let content = fs::read_to_string(json_path)
        .with_context(|| format!("Failed to read transcript at {:?}", json_path))?;
    let transcript: ScribeTranscript = serde_json::from_str(&content)
        .with_context(|| format!("Failed to parse transcript JSON at {:?}", json_path))?;

    let phrases = group_into_phrases(&transcript.words, silence_threshold);
    let duration = if let (Some(first), Some(last)) = (phrases.first(), phrases.last()) {
        last.end - first.start
    } else {
        0.0
    };

    let stem = json_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();

    Ok((stem, duration, phrases))
}

pub fn render_markdown(entries: &[(String, f64, Vec<Phrase>)], silence_threshold: f64) -> String {
    let mut lines = Vec::new();
    lines.push("# Packed transcripts".to_string());
    lines.push(String::new());
    lines.push(format!(
        "Phrase-level, grouped on silences ≥ {:.1}s or speaker change.",
        silence_threshold
    ));
    lines.push("Use `[start-end]` ranges to address cuts in the EDL.".to_string());
    lines.push(String::new());

    for (name, duration, phrases) in entries {
        lines.push(format!(
            "## {}  (duration: {}, {} phrases)",
            name,
            format_duration(*duration),
            phrases.len()
        ));
        if phrases.is_empty() {
            lines.push("  _no speech detected_".to_string());
            lines.push(String::new());
            continue;
        }
        for p in phrases {
            let spk_tag = if let Some(ref spk) = p.speaker_id {
                let spk_str = if let Some(stripped) = spk.strip_prefix("speaker_") {
                    stripped
                } else {
                    spk.as_str()
                };
                format!(" S{spk_str}")
            } else {
                String::new()
            };
            lines.push(format!(
                "  [{}-{}]{} {}",
                format_time(p.start),
                format_time(p.end),
                spk_tag,
                p.text
            ));
        }
        lines.push(String::new());
    }

    lines.join("\n")
}

pub fn pack_transcripts(
    edit_dir: &Path,
    silence_threshold: f64,
    output: Option<&Path>,
) -> Result<PathBuf> {
    let transcripts_dir = edit_dir.join("transcripts");
    if !transcripts_dir.is_dir() {
        anyhow::bail!("no transcripts directory at {:?}", transcripts_dir);
    }

    let mut json_files: Vec<PathBuf> = fs::read_dir(&transcripts_dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("json"))
        .collect();

    json_files.sort();

    if json_files.is_empty() {
        anyhow::bail!("no .json files in {:?}", transcripts_dir);
    }

    let mut entries = Vec::new();
    for file in &json_files {
        entries.push(pack_one_file(file, silence_threshold)?);
    }

    let markdown = render_markdown(&entries, silence_threshold);
    let out_path = output
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| edit_dir.join("takes_packed.md"));

    if let Some(parent) = out_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&out_path, markdown.as_bytes())?;

    let total_phrases: usize = entries.iter().map(|e| e.2.len()).sum();
    let total_duration: f64 = entries.iter().map(|e| e.1).sum();
    let kb = fs::metadata(&out_path)?.len() as f64 / 1024.0;

    println!("packed {} transcripts → {:?}", entries.len(), out_path);
    println!(
        "  {} phrases, {} total runtime",
        total_phrases,
        format_duration(total_duration)
    );
    println!("  {:.1} KB", kb);

    Ok(out_path)
}

pub fn words_in_range(words: &[ScribeWord], start: f64, end: f64) -> Vec<ScribeWord> {
    let mut out = Vec::new();
    for w in words {
        if w.word_type != "word" {
            continue;
        }
        let ws = match w.start {
            Some(s) => s,
            None => continue,
        };
        let we = match w.end {
            Some(e) => e,
            None => continue,
        };
        if we <= start || ws >= end {
            continue;
        }
        out.push(w.clone());
    }
    out
}

pub fn find_silences(
    words: &[ScribeWord],
    start: f64,
    end: f64,
    threshold: f64,
) -> Vec<(f64, f64)> {
    let mut gaps = Vec::new();
    let mut prev_end = start;
    for w in words {
        if w.word_type == "spacing" {
            continue;
        }
        let ws = w.start.unwrap_or(start).max(start);
        if ws - prev_end >= threshold {
            gaps.push((prev_end, ws));
        }
        let we = w.end.unwrap_or(ws);
        prev_end = prev_end.max(we);
    }
    if end - prev_end >= threshold {
        gaps.push((prev_end, end));
    }
    gaps
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_time() {
        assert_eq!(format_time(2.5), "002.50");
        assert_eq!(format_time(43.123), "043.12");
        assert_eq!(format_time(125.0), "125.00");
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(43.0), "43.0s");
        assert_eq!(format_duration(65.4), "1m 05.4s");
        assert_eq!(format_duration(120.0), "2m 00.0s");
    }

    #[test]
    fn test_group_into_phrases_silence_and_speakers() {
        let words = vec![
            ScribeWord {
                word_type: "word".to_string(),
                text: Some("Hello".to_string()),
                start: Some(1.0),
                end: Some(1.5),
                speaker_id: Some(serde_json::json!("speaker_0")),
            },
            ScribeWord {
                word_type: "word".to_string(),
                text: Some("world".to_string()),
                start: Some(1.6),
                end: Some(2.0),
                speaker_id: Some(serde_json::json!("speaker_0")),
            },
            ScribeWord {
                word_type: "spacing".to_string(),
                text: Some(" ".to_string()),
                start: Some(2.0),
                end: Some(3.0), // 1.0s gap >= 0.5s threshold
                speaker_id: None,
            },
            ScribeWord {
                word_type: "word".to_string(),
                text: Some("How".to_string()),
                start: Some(3.0),
                end: Some(3.3),
                speaker_id: Some(serde_json::json!("speaker_1")),
            },
            ScribeWord {
                word_type: "word".to_string(),
                text: Some("are you ?".to_string()),
                start: Some(3.4),
                end: Some(3.8),
                speaker_id: Some(serde_json::json!("speaker_1")),
            },
        ];

        let phrases = group_into_phrases(&words, 0.5);
        assert_eq!(phrases.len(), 2);
        assert_eq!(phrases[0].text, "Hello world");
        assert_eq!(phrases[0].start, 1.0);
        assert_eq!(phrases[0].end, 2.0);
        assert_eq!(phrases[0].speaker_id, Some("speaker_0".to_string()));

        assert_eq!(phrases[1].text, "How are you?");
        assert_eq!(phrases[1].start, 3.0);
        assert_eq!(phrases[1].end, 3.8);
        assert_eq!(phrases[1].speaker_id, Some("speaker_1".to_string()));
    }
}
