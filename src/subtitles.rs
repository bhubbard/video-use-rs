use crate::transcript::{words_in_range, ScribeTranscript, ScribeWord};
use anyhow::{bail, Context, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

pub const CHUNK_WORDS: usize = 2;
pub const CHUNK_MAX_WORDS: usize = 3;
pub const CHUNK_MIN_S: f64 = 0.35;
pub const CHUNK_PAUSE_S: f64 = 0.3;

pub const SUB_FORCE_STYLE: &str = "FontName=Helvetica,FontSize=18,Bold=1,\
PrimaryColour=&H00FFFFFF,OutlineColour=&H00000000,BackColour=&H00000000,\
BorderStyle=1,Outline=2,Shadow=0,\
Alignment=2,MarginV=90";

const PUNCT_BREAK: [char; 6] = ['.', ',', '!', '?', ';', ':'];

pub fn chunk_words(words: &[ScribeWord]) -> Vec<Vec<ScribeWord>> {
    let non_empty: Vec<ScribeWord> = words
        .iter()
        .filter(|w| {
            w.text
                .as_deref()
                .map(|t| !t.trim().is_empty())
                .unwrap_or(false)
        })
        .cloned()
        .collect();

    let mut chunks: Vec<Vec<ScribeWord>> = Vec::new();
    let mut current: Vec<ScribeWord> = Vec::new();

    for i in 0..non_empty.len() {
        let w = &non_empty[i];
        current.push(w.clone());

        let text = w.text.as_deref().unwrap_or("").trim();
        let last_char = text.chars().last();

        let nxt = non_empty.get(i + 1);
        let gap = if let (Some(n), Some(w_end)) = (nxt, w.end) {
            n.start.unwrap_or(w_end) - w_end
        } else {
            0.0
        };

        let current_start = current[0].start.unwrap_or(0.0);
        let current_end = w.end.unwrap_or(current_start);
        let dur = current_end - current_start;

        let ends_with_punct = last_char.map(|c| PUNCT_BREAK.contains(&c)).unwrap_or(false);

        if nxt.is_none()
            || ends_with_punct
            || gap >= CHUNK_PAUSE_S
            || current.len() >= CHUNK_MAX_WORDS
            || (current.len() >= CHUNK_WORDS && dur >= CHUNK_MIN_S)
        {
            chunks.push(std::mem::take(&mut current));
        }
    }

    chunks
}

pub fn srt_timestamp(seconds: f64) -> String {
    let total_ms = (seconds * 1000.0).round() as i64;
    let total_ms = total_ms.max(0);
    let h = total_ms / 3_600_000;
    let rem = total_ms % 3_600_000;
    let m = rem / 60_000;
    let rem2 = rem % 60_000;
    let s = rem2 / 1_000;
    let ms = rem2 % 1_000;
    format!("{:02}:{:02}:{:02},{:03}", h, m, s, ms)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SrtCue {
    pub start: f64,
    pub end: f64,
    pub text: String,
}

pub fn build_master_srt_from_ranges<F>(
    ranges: &[(String, f64, f64)],
    transcript_loader: F,
    out_path: &Path,
) -> Result<usize>
where
    F: Fn(&str) -> Result<Option<ScribeTranscript>>,
{
    let whitespace_re = Regex::new(r"\s+").unwrap();
    let mut entries: Vec<SrtCue> = Vec::new();
    let mut seg_offset = 0.0;

    for (src_name, seg_start, seg_end) in ranges {
        let seg_duration = seg_end - seg_start;
        let transcript = match transcript_loader(src_name)? {
            Some(t) => t,
            None => {
                println!(
                    "  no transcript for {}, skipping captions for this segment",
                    src_name
                );
                seg_offset += seg_duration;
                continue;
            }
        };

        let words_in_seg = words_in_range(&transcript.words, *seg_start, *seg_end);

        for chunk in chunk_words(&words_in_seg) {
            let local_start = chunk
                .first()
                .and_then(|w| w.start)
                .unwrap_or(*seg_start)
                .max(*seg_start);
            let local_end = chunk
                .last()
                .and_then(|w| w.end)
                .unwrap_or(*seg_end)
                .min(*seg_end);

            let out_start = (local_start - seg_start).max(0.0) + seg_offset;
            let mut out_end = (local_end - seg_start).max(0.0) + seg_offset;
            if out_end <= out_start {
                out_end = out_start + 0.4;
            }

            let text_parts: Vec<&str> = chunk
                .iter()
                .filter_map(|w| w.text.as_deref().map(|s| s.trim()))
                .filter(|s| !s.is_empty())
                .collect();
            let raw_text = text_parts.join(" ");
            let cleaned = whitespace_re.replace_all(&raw_text, " ").trim().to_string();
            let stripped = cleaned.trim_end_matches([',', ';', ':']);
            let upper = stripped.to_uppercase();

            entries.push(SrtCue {
                start: out_start,
                end: out_end,
                text: upper,
            });
        }

        seg_offset += seg_duration;
    }

    entries.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap());

    let mut lines = Vec::new();
    for (i, cue) in entries.iter().enumerate() {
        lines.push(format!("{}", i + 1));
        lines.push(format!(
            "{} --> {}",
            srt_timestamp(cue.start),
            srt_timestamp(cue.end)
        ));
        lines.push(cue.text.clone());
        lines.push(String::new());
    }

    if let Some(parent) = out_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(out_path, lines.join("\n").as_bytes())
        .with_context(|| format!("Failed to write master SRT to {:?}", out_path))?;

    println!("master SRT → {:?} ({} cues)", out_path, entries.len());
    Ok(entries.len())
}

pub fn resolve_subtitles_path(maybe_path: &str, edit_dir: &Path) -> Result<PathBuf> {
    let p = Path::new(maybe_path);
    let mut candidates = Vec::new();

    if p.is_absolute() {
        candidates.push(p.to_path_buf());
    } else {
        candidates.push(edit_dir.join(p));
        if let Ok(cwd) = std::env::current_dir() {
            candidates.push(cwd.join(p));
        }
    }

    for c in &candidates {
        if c.exists() {
            return Ok(c.clone());
        }
    }

    let tried: Vec<String> = candidates.iter().map(|c| format!("{:?}", c)).collect();
    bail!(
        "subtitles file in EDL not found (tried {}). Fix the path or pass --no-subtitles.",
        tried.join(", ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(text: &str, start: f64, end: f64) -> ScribeWord {
        ScribeWord::new_word(text, start, end)
    }

    fn texts(chunks: &[Vec<ScribeWord>]) -> Vec<String> {
        chunks
            .iter()
            .map(|c| {
                c.iter()
                    .map(|x| x.text.as_deref().unwrap_or("").trim())
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect()
    }

    // Scribe word timings of a real take: "90% of what a web agent does is completely wasted. We fix this."
    fn real_take() -> Vec<ScribeWord> {
        vec![
            w("90%", 2.64, 3.04),
            w("of", 3.06, 3.14),
            w("what", 3.16, 3.26),
            w("a", 3.30, 3.34),
            w("web", 3.36, 3.50),
            w("agent", 3.54, 3.82),
            w("does", 3.86, 4.14),
            w("is", 4.48, 4.56),
            w("completely", 4.58, 4.90),
            w("wasted.", 4.94, 5.34),
            w("We", 6.08, 6.18),
            w("fix", 6.24, 6.42),
            w("this.", 6.48, 6.70),
        ]
    }

    #[test]
    fn test_chunk_words_real_take() {
        let take = real_take();
        let chunks = chunk_words(&take);
        assert_eq!(
            texts(&chunks),
            vec![
                "90% of",
                "what a web",
                "agent does",
                "is completely",
                "wasted.",
                "We fix this."
            ]
        );
    }

    #[test]
    fn test_pause_splits_across_words() {
        let take = real_take();
        let chunks = texts(&chunk_words(&take));
        for c in &chunks {
            assert!(!c.to_lowercase().contains("does is"));
        }
    }

    #[test]
    fn test_no_multiword_cue_flashes() {
        let take = real_take();
        for c in chunk_words(&take) {
            if c.len() > 1 && c.len() < CHUNK_MAX_WORDS {
                let dur = c.last().unwrap().end.unwrap() - c.first().unwrap().start.unwrap();
                assert!(dur >= CHUNK_MIN_S, "Duration {:.3} < {:.3}", dur, CHUNK_MIN_S);
            }
        }
    }

    #[test]
    fn test_punctuation_still_breaks() {
        let words = vec![w("Hi,", 0.0, 0.1), w("there", 0.12, 0.2)];
        assert_eq!(texts(&chunk_words(&words)), vec!["Hi,", "there"]);
    }

    #[test]
    fn test_caps_at_max_words() {
        let mut fast = Vec::new();
        for i in 0..7 {
            fast.push(w(&format!("w{i}"), i as f64 * 0.05, i as f64 * 0.05 + 0.04));
        }
        let chunks = chunk_words(&fast);
        for c in chunks {
            assert!(c.len() <= CHUNK_MAX_WORDS);
        }
    }

    #[test]
    fn test_skips_empty_words() {
        let words = vec![w(" ", 0.0, 0.1), w("ok", 0.1, 0.5)];
        assert_eq!(texts(&chunk_words(&words)), vec!["ok"]);
    }

    #[test]
    fn test_srt_timestamp_format() {
        assert_eq!(srt_timestamp(0.090), "00:00:00,090");
        assert_eq!(srt_timestamp(0.590), "00:00:00,590");
        assert_eq!(srt_timestamp(65.123), "00:01:05,123");
        assert_eq!(srt_timestamp(3665.456), "01:01:05,456");
    }
}
