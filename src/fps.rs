use anyhow::{bail, Result};
use num_rational::Ratio;
use regex::Regex;
use std::path::Path;
use std::process::Command;
use std::str::FromStr;

const MAX_COMPONENT: i64 = 2_147_483_647;

/// Validate and canonicalize an ffmpeg frame rate.
/// Accepts integers ("60"), decimals ("29.97"), or rationals ("30000/1001").
/// Outputs reduced fraction "num/den" where both components <= 2_147_483_647 (AVRational limit).
pub fn parse_fps(value: &str) -> Result<String> {
    let text = value.trim();
    if text.is_empty() || text.len() > 32 {
        bail!("FPS must be a positive number or rational, e.g. 30 or 30000/1001");
    }

    let pattern = Regex::new(r"^(?:[0-9]+(?:\.[0-9]+)?|[0-9]+/[0-9]+)$").unwrap();
    if !pattern.is_match(text) {
        bail!("FPS must be a positive number or rational, e.g. 30 or 30000/1001");
    }

    let ratio: Ratio<i64> = if let Some((num_s, den_s)) = text.split_once('/') {
        let num: i64 = match i64::from_str(num_s) {
            Ok(n) => n,
            Err(_) => bail!("FPS precision or magnitude is too large"),
        };
        let den: i64 = match i64::from_str(den_s) {
            Ok(d) => d,
            Err(_) => bail!("FPS precision or magnitude is too large"),
        };
        if den == 0 {
            bail!("FPS must be greater than zero");
        }
        Ratio::new(num, den)
    } else if let Some((int_s, frac_s)) = text.split_once('.') {
        // Decimal e.g. 29.97 -> 2997 / 100
        let dec_places = frac_s.len();
        if dec_places > 9 {
            // Very high decimal precision exceeds bounds
            bail!("FPS precision or magnitude is too large");
        }
        let den = 10_i64.pow(dec_places as u32);
        let int_val: i64 = int_s
            .parse()
            .map_err(|_| anyhow::anyhow!("FPS precision or magnitude is too large"))?;
        let frac_val: i64 = frac_s
            .parse()
            .map_err(|_| anyhow::anyhow!("FPS precision or magnitude is too large"))?;
        let num = int_val
            .checked_mul(den)
            .and_then(|v| v.checked_add(frac_val))
            .ok_or_else(|| anyhow::anyhow!("FPS precision or magnitude is too large"))?;
        Ratio::new(num, den)
    } else {
        let num: i64 = text
            .parse()
            .map_err(|_| anyhow::anyhow!("FPS precision or magnitude is too large"))?;
        Ratio::new(num, 1)
    };

    if *ratio.numer() <= 0 || *ratio.denom() <= 0 {
        bail!("FPS must be greater than zero");
    }

    if *ratio.numer() > MAX_COMPONENT || *ratio.denom() > MAX_COMPONENT {
        bail!("FPS precision or magnitude is too large");
    }

    Ok(format!("{}/{}", ratio.numer(), ratio.denom()))
}

/// Probe source FPS from ffprobe streams JSON
pub fn probe_fps_from_json(parsed: &serde_json::Value) -> Option<String> {
    let streams = parsed.get("streams")?.as_array()?;
    if streams.is_empty() {
        return None;
    }

    let stream = &streams[0];
    for field in &["avg_frame_rate", "r_frame_rate"] {
        if let Some(val) = stream.get(field).and_then(|v| v.as_str()) {
            if val != "0/0" && !val.is_empty() {
                if let Ok(canonical) = parse_fps(val) {
                    return Some(canonical);
                }
            }
        }
    }

    None
}

/// Probe source FPS preferring avg_frame_rate with fallback to r_frame_rate.
pub fn probe_source_fps(video: &Path) -> Option<String> {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=avg_frame_rate,r_frame_rate",
            "-of",
            "json",
        ])
        .arg(video)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    probe_fps_from_json(&parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_accepts_integer_decimal_and_rational_rates() {
        let cases = [
            ("60", "60/1"),
            ("29.97", "2997/100"),
            ("30000/1001", "30000/1001"),
            ("24", "24/1"),
            ("23.976", "2997/125"), // 23976/1000 reduces to 2997/125
        ];
        for (input, expected) in cases {
            assert_eq!(parse_fps(input).unwrap(), expected);
        }
    }

    #[test]
    fn test_canonical_rates_are_idempotent() {
        for val in &["60", "29.97", "30000/1001"] {
            let canonical = parse_fps(val).unwrap();
            assert_eq!(parse_fps(&canonical).unwrap(), canonical);
        }
    }

    #[test]
    fn test_rejects_invalid_or_non_positive_rates() {
        let invalid = [
            "",
            "nope",
            "0",
            "-24",
            "1/0",
            "1e3",
            "1_000",
            "0.12345678901234567890",
            "111111111111111111111111111111111", // > 32 chars
        ];
        for val in invalid {
            assert!(parse_fps(val).is_err(), "Expected error for {:?}", val);
        }
    }
}
