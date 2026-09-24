use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

pub const TONEMAP_CHAIN: &str = "zscale=t=linear:npl=100,\
format=gbrpf32le,\
zscale=p=bt709,\
tonemap=tonemap=hable:desat=0,\
zscale=t=bt709:m=bt709:r=tv,\
format=yuv420p";

/// Count how many audio streams the video container holds.
pub fn count_audio_tracks(video: &Path) -> Result<usize> {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "a",
            "-show_entries",
            "stream=index",
            "-of",
            "csv=p=0",
        ])
        .arg(video)
        .output()
        .with_context(|| format!("Failed to run ffprobe on {:?}", video))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let count = stdout
        .lines()
        .map(|ln| ln.trim())
        .filter(|ln| !ln.is_empty())
        .count();

    Ok(count)
}

/// Peak level of a 16-bit PCM wav in dBFS. -inf for digital silence.
pub fn peak_dbfs(wav_path: &Path) -> Result<f64> {
    let reader = hound::WavReader::open(wav_path)
        .with_context(|| format!("Failed to open wav file at {:?}", wav_path))?;

    let mut peak: i32 = 0;
    for sample in reader.into_samples::<i16>() {
        let s = sample.with_context(|| "Failed reading WAV sample")? as i32;
        peak = peak.max(s.abs());
    }

    if peak == 0 {
        Ok(f64::NEG_INFINITY)
    } else {
        Ok(20.0 * ((peak as f64) / 32768.0).log10())
    }
}

/// Extract mono 16kHz PCM audio via ffmpeg.
pub fn extract_audio(video: &Path, dest: &Path, audio_track: usize) -> Result<()> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let status = Command::new("ffmpeg")
        .args([
            "-y",
            "-i",
        ])
        .arg(video)
        .args([
            "-map",
            &format!("0:a:{audio_track}"),
            "-vn",
            "-ac",
            "1",
            "-ar",
            "16000",
            "-c:a",
            "pcm_s16le",
        ])
        .arg(dest)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .with_context(|| format!("Failed to extract audio from {:?}", video))?;

    if !status.success() {
        anyhow::bail!("ffmpeg failed while extracting audio from {:?}", video);
    }

    Ok(())
}

/// Return true if the source uses a PQ (smpte2084) or HLG (arib-std-b67) transfer function.
pub fn is_hdr_source(video: &Path) -> bool {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=color_transfer",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
        ])
        .arg(video)
        .output();

    if let Ok(out) = output {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
            return s == "smpte2084" || s == "arib-std-b67";
        }
    }
    false
}

/// Determine if displayed video is portrait from ffprobe streams JSON
pub fn is_portrait_from_probe_json(parsed: &serde_json::Value) -> bool {
    let streams = match parsed.get("streams").and_then(|s| s.as_array()) {
        Some(s) if !s.is_empty() => s,
        _ => return false,
    };

    let stream = &streams[0];
    let mut w = match stream.get("width").and_then(|v| v.as_i64()) {
        Some(w) => w,
        None => return false,
    };
    let mut h = match stream.get("height").and_then(|v| v.as_i64()) {
        Some(h) => h,
        None => return false,
    };

    let mut rotation = 0.0;
    if let Some(side_data) = stream.get("side_data_list").and_then(|s| s.as_array()) {
        for item in side_data {
            if let Some(rot) = item.get("rotation").and_then(|r| r.as_f64()) {
                rotation = rot;
                break;
            }
        }
    }

    let rot_deg = (rotation.round() as i64).rem_euclid(360);
    if rot_deg == 90 || rot_deg == 270 {
        std::mem::swap(&mut w, &mut h);
    }

    h > w
}

/// Return true if the displayed video is portrait, including rotation.
pub fn is_portrait_source(video: &Path) -> bool {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height:stream_side_data=rotation",
            "-of",
            "json",
        ])
        .arg(video)
        .output();

    let out = match output {
        Ok(o) if o.status.success() => o,
        _ => return false,
    };

    let parsed: serde_json::Value = match serde_json::from_slice(&out.stdout) {
        Ok(v) => v,
        Err(_) => return false,
    };

    is_portrait_from_probe_json(&parsed)
}

/// Extract an audio segment and return an RMS envelope of length `samples`, normalized to [0, 1].
pub fn compute_envelope(
    video: &Path,
    start: f64,
    end: f64,
    samples: usize,
) -> Result<Vec<f32>> {
    let mut env = vec![0.0f32; samples];
    let tmp = tempfile::Builder::new().suffix(".wav").tempfile()?;
    let wav_path = tmp.path();

    let duration = (end - start).max(0.01);
    let status = Command::new("ffmpeg")
        .args([
            "-y",
            "-ss",
            &format!("{:.3}", start),
            "-i",
        ])
        .arg(video)
        .args([
            "-t",
            &format!("{:.3}", duration),
            "-vn",
            "-ac",
            "1",
            "-ar",
            "16000",
            "-c:a",
            "pcm_s16le",
        ])
        .arg(wav_path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    let success = match status {
        Ok(s) => s.success(),
        Err(_) => false,
    };

    if !success || !wav_path.exists() {
        return Ok(env);
    }

    let reader = match hound::WavReader::open(wav_path) {
        Ok(r) => r,
        Err(_) => return Ok(env),
    };

    let pcm_samples: Vec<f32> = reader
        .into_samples::<i16>()
        .filter_map(|s| s.ok())
        .map(|s| (s as f32) / 32768.0)
        .collect();

    if pcm_samples.is_empty() {
        return Ok(env);
    }

    let total = pcm_samples.len();
    let window = (total / samples).max(1);

    let mut max_val = 0.0f32;
    for i in 0..samples {
        let chunk_start = i * window;
        if chunk_start >= total {
            break;
        }
        let chunk_end = (chunk_start + window).min(total);
        let chunk = &pcm_samples[chunk_start..chunk_end];
        let sum_sq: f32 = chunk.iter().map(|&x| x * x).sum();
        let rms = (sum_sq / chunk.len() as f32).sqrt();
        env[i] = rms;
        if rms > max_val {
            max_val = rms;
        }
    }

    if max_val > 0.0 {
        for val in &mut env {
            *val /= max_val;
        }
    }

    Ok(env)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_peak_dbfs_silence() {
        let tmp = tempfile::Builder::new().suffix(".wav").tempfile().unwrap();
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 16000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        {
            let mut writer = hound::WavWriter::create(tmp.path(), spec).unwrap();
            for _ in 0..1000 {
                writer.write_sample(0i16).unwrap();
            }
            writer.finalize().unwrap();
        }
        let db = peak_dbfs(tmp.path()).unwrap();
        assert_eq!(db, f64::NEG_INFINITY);
    }

    #[test]
    fn test_peak_dbfs_full_scale() {
        let tmp = tempfile::Builder::new().suffix(".wav").tempfile().unwrap();
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 16000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        {
            let mut writer = hound::WavWriter::create(tmp.path(), spec).unwrap();
            writer.write_sample(32767i16).unwrap();
            writer.finalize().unwrap();
        }
        let db = peak_dbfs(tmp.path()).unwrap();
        assert!((db - 0.0).abs() < 0.01);
    }
}
