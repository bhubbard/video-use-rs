use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::Command;

pub const PRESET_SUBTLE: &str = "eq=contrast=1.03:saturation=0.98";
pub const PRESET_NEUTRAL_PUNCH: &str = "eq=contrast=1.06:brightness=0.0:saturation=1.0,\
curves=master='0/0 0.25/0.23 0.75/0.77 1/1'";
pub const PRESET_WARM_CINEMATIC: &str = "eq=contrast=1.12:brightness=-0.02:saturation=0.88,\
colorbalance=rs=0.02:gs=0.0:bs=-0.03:rm=0.04:gm=0.01:bm=-0.02:rh=0.08:gh=0.02:bh=-0.05,\
curves=master='0/0 0.25/0.22 0.75/0.78 1/1'";
pub const PRESET_NONE: &str = "";

pub fn presets() -> BTreeMap<&'static str, &'static str> {
    let mut m = BTreeMap::new();
    m.insert("subtle", PRESET_SUBTLE);
    m.insert("neutral_punch", PRESET_NEUTRAL_PUNCH);
    m.insert("warm_cinematic", PRESET_WARM_CINEMATIC);
    m.insert("none", PRESET_NONE);
    m
}

pub fn get_preset(name: &str) -> Result<&'static str> {
    match name {
        "subtle" => Ok(PRESET_SUBTLE),
        "neutral_punch" => Ok(PRESET_NEUTRAL_PUNCH),
        "warm_cinematic" => Ok(PRESET_WARM_CINEMATIC),
        "none" => Ok(PRESET_NONE),
        _ => {
            let available: Vec<&str> = presets().into_keys().collect();
            bail!(
                "unknown preset '{}'. Available: {}",
                name,
                available.join(", ")
            );
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoGradeStats {
    pub y_mean: f64,
    pub y_std: f64,
    pub sat_mean: f64,
}

fn sample_frame_stats(
    video: &Path,
    start: f64,
    duration: f64,
    n_samples: usize,
) -> Result<AutoGradeStats> {
    let fps = ((n_samples as f64) / duration.max(0.1)).clamp(0.5, 10.0);
    let tmp = tempfile::Builder::new().suffix(".txt").tempfile()?;
    let metadata_path = tmp.path();

    let status = Command::new("ffmpeg")
        .args([
            "-y",
            "-hide_banner",
            "-nostats",
            "-ss",
            &format!("{:.3}", start),
            "-i",
        ])
        .arg(video)
        .args([
            "-t",
            &format!("{:.3}", duration),
            "-vf",
            &format!("fps={:.2},signalstats,metadata=print:file={}", fps, metadata_path.display()),
            "-f",
            "null",
            "-",
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .with_context(|| format!("Failed to run signalstats on {:?}", video))?;

    if !status.success() {
        return Ok(AutoGradeStats {
            y_mean: 0.5,
            y_std: 0.18,
            sat_mean: 0.25,
        });
    }

    let content = fs::read_to_string(metadata_path).unwrap_or_default();
    let mut y_avgs = Vec::new();
    let mut y_mins = Vec::new();
    let mut y_maxs = Vec::new();
    let mut sat_avgs = Vec::new();
    let mut bit_depth = 8;

    let parse_val = |line: &str| -> Option<f64> {
        line.rsplit_once('=')?.1.trim().parse::<f64>().ok()
    };

    for line in content.lines() {
        let line = line.trim();
        if line.contains("lavfi.signalstats.YBITDEPTH") {
            if let Some(v) = parse_val(line) {
                bit_depth = v as i32;
            }
        } else if line.contains("lavfi.signalstats.YAVG") {
            if let Some(v) = parse_val(line) {
                y_avgs.push(v);
            }
        } else if line.contains("lavfi.signalstats.YMIN") {
            if let Some(v) = parse_val(line) {
                y_mins.push(v);
            }
        } else if line.contains("lavfi.signalstats.YMAX") {
            if let Some(v) = parse_val(line) {
                y_maxs.push(v);
            }
        } else if line.contains("lavfi.signalstats.SATAVG") {
            if let Some(v) = parse_val(line) {
                sat_avgs.push(v);
            }
        }
    }

    if y_avgs.is_empty() {
        return Ok(AutoGradeStats {
            y_mean: 0.5,
            y_std: 0.18,
            sat_mean: 0.25,
        });
    }

    let max_val = (2_i64.pow(bit_depth as u32) - 1) as f64;
    let y_mean = (y_avgs.iter().sum::<f64>() / y_avgs.len() as f64) / max_val;
    let y_range = if !y_maxs.is_empty() && !y_mins.is_empty() {
        let avg_max = y_maxs.iter().sum::<f64>() / y_maxs.len() as f64;
        let avg_min = y_mins.iter().sum::<f64>() / y_mins.len() as f64;
        (avg_max - avg_min) / max_val
    } else {
        0.7
    };
    let sat_mean = if !sat_avgs.is_empty() {
        (sat_avgs.iter().sum::<f64>() / sat_avgs.len() as f64) / max_val
    } else {
        0.25
    };

    Ok(AutoGradeStats {
        y_mean,
        y_std: y_range / 4.0,
        sat_mean,
    })
}

pub fn auto_grade_for_clip(
    video: &Path,
    start: f64,
    duration: Option<f64>,
    verbose: bool,
) -> Result<(String, AutoGradeStats)> {
    let dur = match duration {
        Some(d) => d,
        None => {
            let out = Command::new("ffprobe")
                .args([
                    "-v",
                    "error",
                    "-show_entries",
                    "format=duration",
                    "-of",
                    "default=noprint_wrappers=1:nokey=1",
                ])
                .arg(video)
                .output();
            if let Ok(o) = out {
                String::from_utf8_lossy(&o.stdout)
                    .trim()
                    .parse::<f64>()
                    .unwrap_or(10.0)
            } else {
                10.0
            }
        }
    };

    let stats = sample_frame_stats(video, start, dur, 10)?;
    let y_mean = stats.y_mean;
    let y_range = stats.y_std * 4.0;
    let sat_mean = stats.sat_mean;

    let mut contrast_adj: f64 = if y_range < 0.65 {
        let t = ((y_range - 0.50) / 0.15).clamp(0.0, 1.0);
        1.08 - 0.05 * t
    } else {
        1.03
    };

    let mut gamma_adj: f64 = if y_mean < 0.42 {
        let t = ((y_mean - 0.30) / 0.12).clamp(0.0, 1.0);
        1.10 - 0.08 * t
    } else if y_mean > 0.60 {
        0.97
    } else {
        1.0
    };

    let mut sat_adj: f64 = if sat_mean < 0.18 {
        1.04
    } else if sat_mean > 0.38 {
        0.96
    } else {
        0.98
    };

    contrast_adj = contrast_adj.clamp(0.94, 1.08);
    gamma_adj = gamma_adj.clamp(0.94, 1.10);
    sat_adj = sat_adj.clamp(0.94, 1.06);

    let mut eq_parts = Vec::new();
    if (contrast_adj - 1.0).abs() > 0.005 {
        eq_parts.push(format!("contrast={:.3}", contrast_adj));
    }
    if (gamma_adj - 1.0).abs() > 0.005 {
        eq_parts.push(format!("gamma={:.3}", gamma_adj));
    }
    if (sat_adj - 1.0).abs() > 0.005 {
        eq_parts.push(format!("saturation={:.3}", sat_adj));
    }

    let filter_string = if eq_parts.is_empty() {
        String::new()
    } else {
        format!("eq={}", eq_parts.join(":"))
    };

    if verbose {
        println!("  auto-grade stats:");
        println!(
            "    y_mean={:.3}  y_range={:.3}  sat_mean={:.3}",
            y_mean, y_range, sat_mean
        );
        println!(
            "    → contrast={:.3}  gamma={:.3}  sat={:.3}",
            contrast_adj, gamma_adj, sat_adj
        );
        println!(
            "    → filter: {}",
            if filter_string.is_empty() {
                "(empty)"
            } else {
                &filter_string
            }
        );
    }

    Ok((filter_string, stats))
}

pub fn apply_grade(input_path: &Path, output_path: &Path, filter_string: &str) -> Result<()> {
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut cmd = Command::new("ffmpeg");
    cmd.arg("-y").arg("-i").arg(input_path);

    if filter_string.is_empty() {
        cmd.args(["-c", "copy"]).arg(output_path);
    } else {
        cmd.args([
            "-vf",
            filter_string,
            "-c:v",
            "libx264",
            "-preset",
            "fast",
            "-crf",
            "18",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "copy",
            "-movflags",
            "+faststart",
        ])
        .arg(output_path);
    }

    let status = cmd
        .status()
        .with_context(|| format!("Failed to execute ffmpeg grade for {:?}", input_path))?;

    if !status.success() {
        bail!("ffmpeg grade failed on {:?}", input_path);
    }

    Ok(())
}
