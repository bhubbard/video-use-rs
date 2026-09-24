use crate::media::{count_audio_tracks, extract_audio, peak_dbfs};
use anyhow::{bail, Context, Result};
use reqwest::blocking::multipart::{Form, Part};
use reqwest::blocking::Client;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub const SCRIBE_URL: &str = "https://api.elevenlabs.io/v1/speech-to-text";

pub fn load_api_key(root_dir: Option<&Path>) -> Result<String> {
    let mut candidate_paths = Vec::new();
    if let Some(root) = root_dir {
        candidate_paths.push(root.join(".env"));
    }
    candidate_paths.push(PathBuf::from(".env"));

    for candidate in candidate_paths {
        if candidate.exists() {
            if let Ok(content) = fs::read_to_string(&candidate) {
                for line in content.lines() {
                    let trimmed = line.trim();
                    if trimmed.is_empty() || trimmed.starts_with('#') || !trimmed.contains('=') {
                        continue;
                    }
                    if let Some((k, v)) = trimmed.split_once('=') {
                        if k.trim() == "ELEVENLABS_API_KEY" {
                            let clean = v.trim().trim_matches('"').trim_matches('\'');
                            if !clean.is_empty() {
                                return Ok(clean.to_string());
                            }
                        }
                    }
                }
            }
        }
    }

    if let Ok(env_val) = std::env::var("ELEVENLABS_API_KEY") {
        let clean = env_val.trim().trim_matches('"').trim_matches('\'');
        if !clean.is_empty() {
            return Ok(clean.to_string());
        }
    }

    bail!("ELEVENLABS_API_KEY not found in .env or environment")
}

pub fn transcript_path(edit_dir: &Path, video: &Path, audio_track: usize) -> PathBuf {
    let suffix = if audio_track == 0 {
        String::new()
    } else {
        format!(".track{audio_track}")
    };
    let stem = video
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("video");
    edit_dir
        .join("transcripts")
        .join(format!("{}{}.json", stem, suffix))
}

pub fn call_scribe(
    audio_path: &Path,
    api_key: &str,
    language: Option<&str>,
    num_speakers: Option<usize>,
) -> Result<serde_json::Value> {
    let client = Client::builder()
        .timeout(Duration::from_secs(1800))
        .build()?;

    let audio_bytes = fs::read(audio_path)
        .with_context(|| format!("Failed to read audio file at {:?}", audio_path))?;
    let file_name = audio_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("audio.wav")
        .to_string();

    let part = Part::bytes(audio_bytes)
        .file_name(file_name)
        .mime_str("audio/wav")?;

    let mut form = Form::new()
        .text("model_id", "scribe_v1")
        .text("diarize", "true")
        .text("tag_audio_events", "true")
        .text("timestamps_granularity", "word")
        .part("file", part);

    if let Some(lang) = language {
        form = form.text("language_code", lang.to_string());
    }
    if let Some(num) = num_speakers {
        form = form.text("num_speakers", num.to_string());
    }

    let resp = client
        .post(SCRIBE_URL)
        .header("xi-api-key", api_key)
        .multipart(form)
        .send()
        .context("Failed sending request to ElevenLabs Scribe")?;

    let status = resp.status();
    let body = resp.text().unwrap_or_default();

    if !status.is_success() {
        let snippet: String = body.chars().take(500).collect();
        bail!("Scribe returned {}: {}", status, snippet);
    }

    let parsed: serde_json::Value = serde_json::from_str(&body)
        .with_context(|| "Failed parsing JSON response from ElevenLabs Scribe")?;

    Ok(parsed)
}

pub fn transcribe_one(
    video: &Path,
    edit_dir: &Path,
    api_key: &str,
    language: Option<&str>,
    num_speakers: Option<usize>,
    verbose: bool,
    audio_track: usize,
) -> Result<PathBuf> {
    let transcripts_dir = edit_dir.join("transcripts");
    fs::create_dir_all(&transcripts_dir)?;
    let out_path = transcript_path(edit_dir, video, audio_track);

    let video_name = video
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("video");
    let video_stem = video
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("video");

    if out_path.exists() {
        if verbose {
            println!("cached: {:?}", out_path.file_name().unwrap_or_default());
        }
        return Ok(out_path);
    }

    if verbose {
        println!("  extracting audio from {}", video_name);
    }

    let n_tracks = count_audio_tracks(video).unwrap_or(1);
    if n_tracks > 1 && verbose {
        println!(
            "  note: {} has {} audio tracks, using track {} (--audio-track to change)",
            video_name,
            n_tracks,
            audio_track + 1
        );
    }

    let t0 = Instant::now();
    let tmp_dir = tempfile::tempdir()?;
    let audio_file = tmp_dir.path().join(format!("{}.wav", video_stem));

    extract_audio(video, &audio_file, audio_track)?;

    let peak = peak_dbfs(&audio_file)?;
    if peak < -60.0 {
        let other_tracks: Vec<String> = (0..n_tracks)
            .filter(|&i| i != audio_track)
            .map(|i| i.to_string())
            .collect();
        let suggestion = if n_tracks > 1 {
            format!(
                "The file has {} audio tracks; try --audio-track {}.",
                n_tracks,
                other_tracks.join(" or ")
            )
        } else {
            "Check the source audio.".to_string()
        };
        bail!(
            "track {} of {} is silent (peak {:.1} dBFS) - not uploading. {}",
            audio_track + 1,
            video_name,
            peak,
            suggestion
        );
    }

    let size_mb = (fs::metadata(&audio_file)?.len() as f64) / (1024.0 * 1024.0);
    if verbose {
        println!("  uploading {}.wav ({:.1} MB)", video_stem, size_mb);
    }

    let payload = call_scribe(&audio_file, api_key, language, num_speakers)?;

    let formatted = serde_json::to_string_pretty(&payload)?;
    fs::write(&out_path, formatted.as_bytes())?;
    let dt = t0.elapsed().as_secs_f64();

    if verbose {
        let kb = (fs::metadata(&out_path)?.len() as f64) / 1024.0;
        println!(
            "  saved: {:?} ({:.1} KB) in {:.1}s",
            out_path.file_name().unwrap_or_default(),
            kb,
            dt
        );
        if let Some(words) = payload.get("words").and_then(|w| w.as_array()) {
            println!("    words: {}", words.len());
        }
    }

    Ok(out_path)
}

pub fn find_videos(videos_dir: &Path) -> Result<Vec<PathBuf>> {
    let exts: HashSet<&'static str> = [
        "mp4", "MP4", "mov", "MOV", "mkv", "MKV", "avi", "AVI", "m4v", "M4V",
    ]
    .into_iter()
    .collect();

    let mut videos = Vec::new();
    for entry in fs::read_dir(videos_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
                if exts.contains(ext) {
                    videos.push(path);
                }
            }
        }
    }
    videos.sort();
    Ok(videos)
}

pub fn transcribe_batch(
    videos_dir: &Path,
    edit_dir: Option<&Path>,
    workers: usize,
    language: Option<&str>,
    num_speakers: Option<usize>,
    audio_track: usize,
) -> Result<()> {
    let edit_dir_buf = edit_dir
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| videos_dir.join("edit"));
    let transcripts_dir = edit_dir_buf.join("transcripts");
    fs::create_dir_all(&transcripts_dir)?;

    let videos = find_videos(videos_dir)?;
    if videos.is_empty() {
        bail!("no videos found in {:?}", videos_dir);
    }

    let mut already_cached = Vec::new();
    let mut pending = Vec::new();

    for v in &videos {
        if transcript_path(&edit_dir_buf, v, audio_track).exists() {
            already_cached.push(v.clone());
        } else {
            pending.push(v.clone());
        }
    }

    println!(
        "found {} videos ({} cached, {} to transcribe)",
        videos.len(),
        already_cached.len(),
        pending.len()
    );

    if pending.is_empty() {
        println!("nothing to do");
        return Ok(());
    }

    let api_key = load_api_key(Some(videos_dir))?;
    println!(
        "transcribing {} files with {} parallel workers",
        pending.len(),
        workers
    );
    let t0 = Instant::now();

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(workers)
        .build()?;

    use std::sync::{Arc, Mutex};
    let errors: Arc<Mutex<Vec<(PathBuf, String)>>> = Arc::new(Mutex::new(Vec::new()));

    pool.scope(|s| {
        for v in &pending {
            let v = v.clone();
            let edit_dir_buf = edit_dir_buf.clone();
            let api_key = api_key.clone();
            let errors = Arc::clone(&errors);

            s.spawn(move |_| {
                let stem = v.file_stem().and_then(|s| s.to_str()).unwrap_or("video");
                match transcribe_one(
                    &v,
                    &edit_dir_buf,
                    &api_key,
                    language,
                    num_speakers,
                    false,
                    audio_track,
                ) {
                    Ok(out) => {
                        println!(
                            "  + {}  →  {:?}",
                            stem,
                            out.file_name().unwrap_or_default()
                        );
                    }
                    Err(e) => {
                        println!("  x {}  FAILED: {}", stem, e);
                        errors.lock().unwrap().push((v, e.to_string()));
                    }
                }
            });
        }
    });

    let dt = t0.elapsed().as_secs_f64();
    println!("\ndone in {:.1}s", dt);

    let errs = errors.lock().unwrap().clone();
    if !errs.is_empty() {
        println!("{} failures:", errs.len());
        for (v, msg) in errs {
            println!(
                "  {}: {}",
                v.file_name().and_then(|s| s.to_str()).unwrap_or("?"),
                msg
            );
        }
        bail!("batch transcription had errors");
    }

    Ok(())
}
