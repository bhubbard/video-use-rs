use crate::edl::{resolve_path, Edl, EdlOverlay};
use crate::fps::{parse_fps, probe_source_fps};
use crate::grade::{auto_grade_for_clip, get_preset};
use crate::loudnorm::apply_loudnorm_two_pass;
use crate::media::{is_hdr_source, is_portrait_source, TONEMAP_CHAIN};
use crate::subtitles::{
    build_master_srt_from_ranges, resolve_subtitles_path, SUB_FORCE_STYLE,
};
use crate::transcript::ScribeTranscript;
use anyhow::{bail, Context, Result};
use regex::Regex;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn resolve_grade_filter(grade_field: Option<&str>) -> String {
    let field = match grade_field {
        Some(f) if !f.trim().is_empty() => f.trim(),
        _ => return String::new(),
    };

    if field == "auto" {
        return "__AUTO__".to_string();
    }

    let ident_re = Regex::new(r"^[a-zA-Z0-9_\-]+$").unwrap();
    if ident_re.is_match(field) {
        match get_preset(field) {
            Ok(preset) => preset.to_string(),
            Err(_) => {
                println!("warning: unknown preset '{}', using as raw filter", field);
                field.to_string()
            }
        }
    } else {
        field.to_string()
    }
}

pub fn extract_segment(
    source: &Path,
    seg_start: f64,
    duration: f64,
    grade_filter: &str,
    out_path: &Path,
    preview: bool,
    draft: bool,
    rate: Option<&str>,
) -> Result<()> {
    if let Some(parent) = out_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let portrait = is_portrait_source(source);
    let scale = if draft {
        if portrait {
            "scale=-2:1280"
        } else {
            "scale=1280:-2"
        }
    } else if portrait {
        "scale=-2:1920"
    } else {
        "scale=1920:-2"
    };

    let mut vf_parts = Vec::new();
    if is_hdr_source(source) {
        vf_parts.push(TONEMAP_CHAIN.to_string());
    }
    vf_parts.push(scale.to_string());
    if !grade_filter.is_empty() {
        vf_parts.push(grade_filter.to_string());
    }
    let vf = vf_parts.join(",");

    let fade_out_start = (duration - 0.03).max(0.0);
    let af = format!(
        "afade=t=in:st=0:d=0.03,afade=t=out:st={:.3}:d=0.03",
        fade_out_start
    );

    let (preset, crf) = if draft {
        ("ultrafast", "28")
    } else if preview {
        ("medium", "22")
    } else {
        ("fast", "20")
    };

    let probed_rate = probe_source_fps(source);
    let out_rate = rate
        .map(|r| r.to_string())
        .or(probed_rate)
        .unwrap_or_else(|| "24".to_string());

    let status = Command::new("ffmpeg")
        .args([
            "-y",
            "-ss",
            &format!("{:.3}", seg_start),
            "-i",
        ])
        .arg(source)
        .args([
            "-t",
            &format!("{:.3}", duration),
            "-vf",
            &vf,
            "-af",
            &af,
            "-c:v",
            "libx264",
            "-preset",
            preset,
            "-crf",
            crf,
            "-pix_fmt",
            "yuv420p",
            "-r",
            &out_rate,
            "-c:a",
            "aac",
            "-b:a",
            "192k",
            "-ar",
            "48000",
            "-movflags",
            "+faststart",
        ])
        .arg(out_path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .with_context(|| format!("Failed to extract segment from {:?}", source))?;

    if !status.success() {
        bail!("ffmpeg segment extract failed for {:?}", source);
    }

    Ok(())
}

pub fn extract_all_segments(
    edl: &Edl,
    edit_dir: &Path,
    preview: bool,
    draft: bool,
    fps: Option<&str>,
) -> Result<Vec<PathBuf>> {
    let resolved = resolve_grade_filter(edl.grade.as_deref());
    let is_auto = resolved == "__AUTO__";

    let clips_dirname = if draft {
        "clips_draft"
    } else if preview {
        "clips_preview"
    } else {
        "clips_graded"
    };
    let clips_dir = edit_dir.join(clips_dirname);
    fs::create_dir_all(&clips_dir)?;

    let out_rate = if let Some(custom_fps) = fps {
        parse_fps(custom_fps)?
    } else if let Some(first_range) = edl.ranges.first() {
        let first_src = edl
            .resolve_source_path(&first_range.source, edit_dir)
            .unwrap_or_else(|| edit_dir.join(&first_range.source));
        probe_source_fps(&first_src).unwrap_or_else(|| "24".to_string())
    } else {
        "24".to_string()
    };

    println!(
        "extracting {} segment(s) → {}/  @ {} fps{}",
        edl.ranges.len(),
        clips_dirname,
        out_rate,
        if fps.is_some() {
            " (forced)"
        } else {
            " (from source)"
        }
    );
    if is_auto {
        println!("  (auto-grade per segment: analyzing each range)");
    }

    let mut seg_paths = Vec::new();

    for (i, r) in edl.ranges.iter().enumerate() {
        let src_path = edl
            .resolve_source_path(&r.source, edit_dir)
            .ok_or_else(|| anyhow::anyhow!("Source '{}' not defined in EDL sources", r.source))?;

        if !src_path.exists() {
            bail!("source file not found: {:?}", src_path);
        }

        let start = r.start;
        let end = r.end;
        let duration = end - start;
        let out_path = clips_dir.join(format!("seg_{:02}_{}.mp4", i, r.source));

        let seg_filter = if is_auto {
            let (filter, _stats) =
                auto_grade_for_clip(&src_path, start, Some(duration), false)?;
            filter
        } else {
            resolved.clone()
        };

        let note = r
            .beat
            .as_deref()
            .or(r.note.as_deref())
            .unwrap_or("");
        println!(
            "  [{:02}] {}  {:7.2}-{:7.2}  ({:5.2}s)  {}",
            i, r.source, start, end, duration, note
        );
        if is_auto {
            println!(
                "        grade: {}",
                if seg_filter.is_empty() {
                    "(none)"
                } else {
                    &seg_filter
                }
            );
        }

        extract_segment(
            &src_path,
            start,
            duration,
            &seg_filter,
            &out_path,
            preview,
            draft,
            Some(&out_rate),
        )?;

        seg_paths.push(out_path);
    }

    Ok(seg_paths)
}

pub fn concat_segments(
    segment_paths: &[PathBuf],
    out_path: &Path,
    edit_dir: &Path,
) -> Result<()> {
    if let Some(parent) = out_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let concat_list = edit_dir.join("_concat.txt");
    let mut lines = Vec::new();
    for p in segment_paths {
        let abs = p.canonicalize().unwrap_or_else(|_| p.clone());
        lines.push(format!("file '{}'\n", abs.display()));
    }
    fs::write(&concat_list, lines.concat().as_bytes())?;

    println!("concat → {:?}", out_path.file_name().unwrap_or_default());
    let status = Command::new("ffmpeg")
        .args([
            "-y",
            "-f",
            "concat",
            "-safe",
            "0",
            "-i",
        ])
        .arg(&concat_list)
        .args([
            "-c",
            "copy",
            "-movflags",
            "+faststart",
        ])
        .arg(out_path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .with_context(|| "Failed executing lossless concat")?;

    let _ = fs::remove_file(&concat_list);

    if !status.success() {
        bail!("ffmpeg concat failed");
    }

    Ok(())
}

pub fn build_final_composite(
    base_path: &Path,
    overlays: &[EdlOverlay],
    subtitles_path: Option<&Path>,
    out_path: &Path,
    edit_dir: &Path,
) -> Result<()> {
    if let Some(parent) = out_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let has_overlays = !overlays.is_empty();
    let has_subs = subtitles_path.map(|p| p.exists()).unwrap_or(false);

    if !has_overlays && !has_subs {
        let status = Command::new("ffmpeg")
            .args(["-y", "-i"])
            .arg(base_path)
            .args(["-c", "copy"])
            .arg(out_path)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()?;
        if !status.success() {
            bail!("Failed to copy base to final output");
        }
        return Ok(());
    }

    let mut cmd = Command::new("ffmpeg");
    cmd.arg("-y").arg("-i").arg(base_path);

    for ov in overlays {
        let ov_path = resolve_path(&ov.file, edit_dir);
        cmd.arg("-i").arg(ov_path);
    }

    let mut filter_parts = Vec::new();

    // PTS-shift every overlay so its frame 0 lands at start_in_output
    for (idx_0, ov) in overlays.iter().enumerate() {
        let idx = idx_0 + 1;
        let t = ov.start_in_output;
        filter_parts.push(format!("[{idx}:v]setpts=PTS-STARTPTS+{t:.3}/TB[a{idx}]"));
    }

    // Chain overlays on top of base
    let mut current = "[0:v]".to_string();
    for (idx_0, ov) in overlays.iter().enumerate() {
        let idx = idx_0 + 1;
        let t = ov.start_in_output;
        let end = t + ov.duration;
        let next_label = format!("[v{idx}]");
        filter_parts.push(format!(
            "{current}[a{idx}]overlay=enable='between(t,{t:.3},{end:.3})'{next_label}"
        ));
        current = next_label;
    }

    // Subtitles LAST — Rule 1
    let out_label = if let (true, Some(subs_p)) = (has_subs, subtitles_path) {
        let abs = subs_p
            .canonicalize()
            .unwrap_or_else(|_| subs_p.to_path_buf());
        let subs_str = abs.display().to_string();
        let escaped = subs_str.replace(':', r"\:").replace('\'', r"\'");
        filter_parts.push(format!(
            "{current}subtitles='{escaped}':force_style='{SUB_FORCE_STYLE}'[outv]"
        ));
        "[outv]"
    } else if has_overlays {
        filter_parts.push(format!("{current}null[outv]"));
        "[outv]"
    } else {
        "[0:v]"
    };

    let filter_complex = filter_parts.join(";");

    println!(
        "compositing → {:?}",
        out_path.file_name().unwrap_or_default()
    );
    println!(
        "  overlays: {}, subtitles: {}",
        overlays.len(),
        if has_subs { "yes" } else { "no" }
    );

    let status = cmd
        .args([
            "-filter_complex",
            &filter_complex,
            "-map",
            out_label,
            "-map",
            "0:a",
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
        .arg(out_path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .with_context(|| "Failed compositing video with overlays and subtitles")?;

    if !status.success() {
        bail!("ffmpeg compositing failed");
    }

    Ok(())
}

pub struct RenderOptions<'a> {
    pub edl_path: &'a Path,
    pub output_path: &'a Path,
    pub preview: bool,
    pub draft: bool,
    pub build_subtitles: bool,
    pub no_subtitles: bool,
    pub no_loudnorm: bool,
    pub fps: Option<&'a str>,
}

pub fn render_edl(opts: RenderOptions) -> Result<()> {
    if !opts.edl_path.exists() {
        bail!("edl not found: {:?}", opts.edl_path);
    }

    let edl = Edl::load_from_file(opts.edl_path)?;
    let edit_dir = opts
        .edl_path
        .parent()
        .unwrap_or_else(|| Path::new("."));

    // 1. Extract per-segment
    let segment_paths = extract_all_segments(
        &edl,
        edit_dir,
        opts.preview,
        opts.draft,
        opts.fps,
    )?;

    // 2. Concat → base
    let base_name = if opts.draft {
        "base_draft.mp4"
    } else if opts.preview {
        "base_preview.mp4"
    } else {
        "base.mp4"
    };
    let base_path = edit_dir.join(base_name);
    concat_segments(&segment_paths, &base_path, edit_dir)?;

    // 3. Subtitles
    let mut subs_path: Option<PathBuf> = None;
    if !opts.no_subtitles {
        if opts.build_subtitles {
            let srt_out = edit_dir.join("master.srt");
            let ranges: Vec<(String, f64, f64)> = edl
                .ranges
                .iter()
                .map(|r| (r.source.clone(), r.start, r.end))
                .collect();
            let transcripts_dir = edit_dir.join("transcripts");

            build_master_srt_from_ranges(
                &ranges,
                |src_name| {
                    let p = transcripts_dir.join(format!("{src_name}.json"));
                    if p.exists() {
                        let text = fs::read_to_string(&p)?;
                        let t: ScribeTranscript = serde_json::from_str(&text)?;
                        Ok(Some(t))
                    } else {
                        Ok(None)
                    }
                },
                &srt_out,
            )?;
            subs_path = Some(srt_out);
        } else if let Some(ref s_path_str) = edl.subtitles {
            subs_path = Some(resolve_subtitles_path(s_path_str, edit_dir)?);
        }
    }

    // 4. Overlays & Loudnorm
    let overlays = edl.overlays.unwrap_or_default();
    if opts.no_loudnorm {
        build_final_composite(
            &base_path,
            &overlays,
            subs_path.as_deref(),
            opts.output_path,
            edit_dir,
        )?;
    } else {
        let tmp_composite = opts.output_path.with_extension("prenorm.mp4");
        build_final_composite(
            &base_path,
            &overlays,
            subs_path.as_deref(),
            &tmp_composite,
            edit_dir,
        )?;
        println!("loudness normalization → social-ready (-14 LUFS / -1 dBTP / LRA 11)");
        apply_loudnorm_two_pass(&tmp_composite, opts.output_path, opts.draft)?;
        let _ = fs::remove_file(&tmp_composite);
    }

    if opts.output_path.exists() {
        let size_mb = (fs::metadata(opts.output_path)?.len() as f64) / (1024.0 * 1024.0);
        println!("\ndone: {:?} ({:.1} MB)", opts.output_path, size_mb);
    }

    Ok(())
}
