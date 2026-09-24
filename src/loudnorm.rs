use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Command;

pub const LOUDNORM_I: f64 = -14.0;
pub const LOUDNORM_TP: f64 = -1.0;
pub const LOUDNORM_LRA: f64 = 11.0;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoudnormMeasurement {
    pub input_i: String,
    pub input_tp: String,
    pub input_lra: String,
    pub input_thresh: String,
    pub target_offset: String,
}

pub fn parse_loudnorm_stderr(stderr: &str) -> Option<LoudnormMeasurement> {
    let start = stderr.rfind('{')?;
    let end = stderr.rfind('}')?;
    if end <= start {
        return None;
    }

    let json_str = &stderr[start..=end];
    let val: serde_json::Value = serde_json::from_str(json_str).ok()?;

    let get_str = |key: &str| -> Option<String> {
        val.get(key).map(|v| match v {
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Number(n) => n.to_string(),
            _ => v.to_string(),
        })
    };

    Some(LoudnormMeasurement {
        input_i: get_str("input_i")?,
        input_tp: get_str("input_tp")?,
        input_lra: get_str("input_lra")?,
        input_thresh: get_str("input_thresh")?,
        target_offset: get_str("target_offset")?,
    })
}

pub fn measure_loudness(video_path: &Path) -> Result<Option<LoudnormMeasurement>> {
    let filter_str = format!(
        "loudnorm=I={:.1}:TP={:.1}:LRA={:.1}:print_format=json",
        LOUDNORM_I, LOUDNORM_TP, LOUDNORM_LRA
    );

    let output = Command::new("ffmpeg")
        .args([
            "-y",
            "-hide_banner",
            "-nostats",
            "-i",
        ])
        .arg(video_path)
        .args([
            "-af",
            &filter_str,
            "-vn",
            "-f",
            "null",
            "-",
        ])
        .output()
        .with_context(|| format!("Failed to run loudness measurement on {:?}", video_path))?;

    let stderr = String::from_utf8_lossy(&output.stderr);
    Ok(parse_loudnorm_stderr(&stderr))
}

pub fn apply_loudnorm_two_pass(
    input_path: &Path,
    output_path: &Path,
    preview: bool,
) -> Result<bool> {
    if preview {
        let filter_str = format!(
            "loudnorm=I={:.1}:TP={:.1}:LRA={:.1}",
            LOUDNORM_I, LOUDNORM_TP, LOUDNORM_LRA
        );
        println!(
            "  loudnorm (1-pass preview) → {:?}",
            output_path.file_name().unwrap_or_default()
        );
        let status = Command::new("ffmpeg")
            .args([
                "-y",
                "-hide_banner",
                "-nostats",
                "-i",
            ])
            .arg(input_path)
            .args([
                "-c:v",
                "copy",
                "-af",
                &filter_str,
                "-c:a",
                "aac",
                "-b:a",
                "192k",
                "-ar",
                "48000",
                "-movflags",
                "+faststart",
            ])
            .arg(output_path)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .with_context(|| "Failed executing 1-pass loudnorm")?;

        return Ok(status.success());
    }

    println!(
        "  loudnorm pass 1: measuring {:?}",
        input_path.file_name().unwrap_or_default()
    );
    let measurement = match measure_loudness(input_path)? {
        Some(m) => m,
        None => {
            println!("  loudnorm measurement failed — falling back to 1-pass");
            return apply_loudnorm_two_pass(input_path, output_path, true);
        }
    };

    println!(
        "    measured: I={} LUFS  TP={}  LRA={}",
        measurement.input_i, measurement.input_tp, measurement.input_lra
    );

    let filter_str = format!(
        "loudnorm=I={:.1}:TP={:.1}:LRA={:.1}:measured_I={}:measured_TP={}:measured_LRA={}:measured_thresh={}:offset={}:linear=true",
        LOUDNORM_I,
        LOUDNORM_TP,
        LOUDNORM_LRA,
        measurement.input_i,
        measurement.input_tp,
        measurement.input_lra,
        measurement.input_thresh,
        measurement.target_offset
    );

    println!(
        "  loudnorm pass 2: normalizing → {:?}",
        output_path.file_name().unwrap_or_default()
    );
    let status = Command::new("ffmpeg")
        .args([
            "-y",
            "-hide_banner",
            "-nostats",
            "-i",
        ])
        .arg(input_path)
        .args([
            "-c:v",
            "copy",
            "-af",
            &filter_str,
            "-c:a",
            "aac",
            "-b:a",
            "192k",
            "-ar",
            "48000",
            "-movflags",
            "+faststart",
        ])
        .arg(output_path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .with_context(|| "Failed executing 2-pass loudnorm")?;

    Ok(status.success())
}
