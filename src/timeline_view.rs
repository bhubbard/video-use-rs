use crate::media::compute_envelope;
use crate::transcript::{find_silences, words_in_range, ScribeTranscript};
use ab_glyph::{FontArc, PxScale};
use anyhow::{Context, Result};
use image::{imageops, ImageBuffer, Rgba};
use imageproc::drawing::{
    draw_filled_rect_mut, draw_line_segment_mut, draw_polygon_mut, draw_text_mut,
};
use imageproc::point::Point;
use imageproc::rect::Rect;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const FONT_CANDIDATES: &[&str] = &[
    "/System/Library/Fonts/Menlo.ttc",
    "/System/Library/Fonts/Helvetica.ttc",
    "/System/Library/Fonts/SFNSMono.ttf",
    "/Library/Fonts/Arial.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
    "/usr/share/fonts/truetype/liberation/LiberationMono-Regular.ttf",
    "/usr/share/fonts/TTF/DejaVuSans.ttf",
];

pub fn load_system_font() -> Option<FontArc> {
    for path in FONT_CANDIDATES {
        if let Ok(bytes) = fs::read(path) {
            if let Ok(font) = FontArc::try_from_vec(bytes) {
                return Some(font);
            }
        }
    }
    None
}

pub fn extract_frames(
    video: &Path,
    start: f64,
    end: f64,
    n: usize,
    dest_dir: &Path,
) -> Result<Vec<PathBuf>> {
    fs::create_dir_all(dest_dir)?;
    let count = n.max(1);
    let times: Vec<f64> = if count == 1 {
        vec![(start + end) / 2.0]
    } else {
        let step = (end - start) / ((count - 1) as f64);
        (0..count).map(|i| start + (i as f64) * step).collect()
    };

    let mut paths = Vec::new();
    for (i, t) in times.iter().enumerate() {
        let out = dest_dir.join(format!("f_{:03}.jpg", i));
        let status = Command::new("ffmpeg")
            .args([
                "-y",
                "-ss",
                &format!("{:.3}", t),
                "-i",
            ])
            .arg(video)
            .args([
                "-frames:v",
                "1",
                "-q:v",
                "4",
                "-vf",
                "scale=320:-2",
            ])
            .arg(&out)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .with_context(|| format!("Failed extracting frame at {:.2}s", t))?;

        if status.success() && out.exists() {
            paths.push(out);
        }
    }

    Ok(paths)
}

pub fn render_timeline(
    video: &Path,
    start: f64,
    end: f64,
    out_path: &Path,
    n_frames: usize,
    transcript_path: Option<&Path>,
) -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let tmp_dir = tmp.path();

    println!(
        "extracting {} frames from {:.2}s to {:.2}s",
        n_frames, start, end
    );
    let frame_paths = extract_frames(video, start, end, n_frames, tmp_dir)?;

    let font = load_system_font();

    // Canvas layout metrics
    let mut canvas_width = 1920u32;
    let frame_h = 180u32;
    let filmstrip_y = 50i32;
    let filmstrip_h = frame_h as i32;
    let wave_y = filmstrip_y + filmstrip_h + 20;
    let wave_h = 220i32;
    let label_y = wave_y + wave_h + 10;
    let canvas_height = (label_y + 60) as u32;

    // Load and resize thumbnails
    let mut imgs = Vec::new();
    for fp in &frame_paths {
        if let Ok(dyn_img) = image::open(fp) {
            let (w, h) = (dyn_img.width(), dyn_img.height());
            if h > 0 {
                let aspect = (w as f32) / (h as f32);
                let new_w = ((frame_h as f32) * aspect).round().max(1.0) as u32;
                let resized = dyn_img.resize_exact(new_w, frame_h, imageops::FilterType::Lanczos3);
                imgs.push(resized.to_rgba8());
            }
        }
    }

    let total_frame_w: u32 = if !imgs.is_empty() {
        imgs.iter().map(|img| img.width()).sum::<u32>() + ((imgs.len() - 1) as u32) * 4
    } else {
        0
    };

    let content_w = 1400.max(total_frame_w);
    canvas_width = canvas_width.max(content_w + 100);

    let bg_color = Rgba([18, 18, 22, 255]);
    let fg_color = Rgba([235, 235, 235, 255]);
    let dim_color = Rgba([110, 110, 120, 255]);
    let silence_color = Rgba([50, 80, 120, 200]);
    let wave_color = Rgba([140, 180, 255, 255]);
    let wave_poly_color = Rgba([140, 180, 255, 60]);

    let mut canvas: ImageBuffer<Rgba<u8>, Vec<u8>> =
        ImageBuffer::from_pixel(canvas_width, canvas_height, bg_color);

    // Header text
    let video_name = video
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("video");
    let header_text = format!(
        "{}   {:.2}s → {:.2}s   ({:.2}s, {} frames)",
        video_name,
        start,
        end,
        end - start,
        imgs.len()
    );

    if let Some(ref f) = font {
        draw_text_mut(
            &mut canvas,
            fg_color,
            50,
            12,
            PxScale::from(22.0),
            f,
            &header_text,
        );
    }

    // Filmstrip layout
    let strip_width = canvas_width - 100;
    let draw_width: u32;

    if total_frame_w <= strip_width {
        let mut cursor = 50u32;
        for img in &imgs {
            imageops::overlay(&mut canvas, img, cursor as i64, filmstrip_y as i64);
            cursor += img.width() + 4;
        }
        draw_width = cursor.saturating_sub(50);
    } else {
        let scale = (strip_width as f32) / (total_frame_w as f32);
        let new_h = ((frame_h as f32) * scale).round() as u32;
        let mut cursor = 50u32;
        for img in &imgs {
            let new_w = ((img.width() as f32) * scale).round() as u32;
            let scaled = imageops::resize(img, new_w, new_h, imageops::FilterType::Lanczos3);
            let y_off = filmstrip_y + ((filmstrip_h - new_h as i32) / 2);
            imageops::overlay(&mut canvas, &scaled, cursor as i64, y_off as i64);
            cursor += new_w + 2.max((4.0 * scale) as u32);
        }
        draw_width = cursor.saturating_sub(50);
    }

    let strip_x0 = 50i32;
    let strip_x1 = (50 + draw_width) as i32;
    let strip_span = (strip_x1 - strip_x0).max(1) as f64;

    let time_to_x = |t: f64| -> i32 {
        let frac = (t - start) / (end - start).max(1e-6);
        strip_x0 + (frac * strip_span).round() as i32
    };

    // Waveform background box
    draw_filled_rect_mut(
        &mut canvas,
        Rect::at(strip_x0, wave_y).of_size(draw_width, wave_h as u32),
        Rgba([28, 28, 34, 255]),
    );

    // Transcript words and silence gaps
    let words = if let Some(tr_path) = transcript_path {
        if tr_path.exists() {
            let text = fs::read_to_string(tr_path).unwrap_or_default();
            let tr: ScribeTranscript = serde_json::from_str(&text).unwrap_or_default();
            words_in_range(&tr.words, start, end)
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };

    let silences = if !words.is_empty() {
        find_silences(&words, start, end, 0.4)
    } else {
        Vec::new()
    };

    for (sa, sb) in &silences {
        let xa = time_to_x(*sa);
        let xb = time_to_x(*sb);
        let w = (xb - xa).max(1) as u32;
        draw_filled_rect_mut(
            &mut canvas,
            Rect::at(xa, wave_y).of_size(w, wave_h as u32),
            silence_color,
        );
    }

    // Audio envelope
    let env_samples = (strip_span as usize).max(200);
    let env = compute_envelope(video, start, end, env_samples).unwrap_or_else(|_| vec![0.0; env_samples]);

    let mid_y = wave_y + wave_h / 2;
    let max_amp = (wave_h / 2 - 8) as f32;

    let mut points_top: Vec<Point<i32>> = Vec::new();
    let mut points_bot: Vec<Point<i32>> = Vec::new();

    for (i, &v) in env.iter().enumerate() {
        let xi = strip_x0 + ((i as f64 * strip_span) / (env.len() - 1).max(1) as f64) as i32;
        let a = (v * max_amp).round() as i32;
        points_top.push(Point::new(xi, mid_y - a));
        points_bot.push(Point::new(xi, mid_y + a));
    }

    if points_top.len() >= 2 {
        let mut poly = points_top.clone();
        for p in points_bot.iter().rev() {
            poly.push(*p);
        }
        draw_polygon_mut(&mut canvas, &poly, wave_poly_color);

        for w_top in points_top.windows(2) {
            draw_line_segment_mut(
                &mut canvas,
                (w_top[0].x as f32, w_top[0].y as f32),
                (w_top[1].x as f32, w_top[1].y as f32),
                wave_color,
            );
        }
        for w_bot in points_bot.windows(2) {
            draw_line_segment_mut(
                &mut canvas,
                (w_bot[0].x as f32, w_bot[0].y as f32),
                (w_bot[1].x as f32, w_bot[1].y as f32),
                wave_color,
            );
        }
    }

    // Word labels above waveform
    if let Some(ref f) = font {
        let mut last_label_x = -9999i32;
        for w in &words {
            if w.word_type != "word" {
                continue;
            }
            let (ws, we) = match (w.start, w.end) {
                (Some(s), Some(e)) => (s, e),
                _ => continue,
            };
            let text = w.text.as_deref().unwrap_or("").trim();
            if text.is_empty() || (we - ws) < 0.05 {
                continue;
            }

            let cx = (time_to_x(ws) + time_to_x(we)) / 2;
            if cx - last_label_x < 28 {
                continue;
            }

            // Small tick
            draw_line_segment_mut(
                &mut canvas,
                (cx as f32, (wave_y - 4) as f32),
                (cx as f32, wave_y as f32),
                dim_color,
            );

            // Label
            draw_text_mut(
                &mut canvas,
                fg_color,
                cx + 2,
                wave_y - 18,
                PxScale::from(12.0),
                f,
                text,
            );
            last_label_x = cx;
        }

        // Time ruler below waveform
        let ruler_y = wave_y + wave_h + 2;
        let n_ticks = 6;
        for i in 0..=n_ticks {
            let frac = (i as f64) / (n_ticks as f64);
            let t = start + frac * (end - start);
            let xi = strip_x0 + (frac * strip_span) as i32;

            draw_line_segment_mut(
                &mut canvas,
                (xi as f32, ruler_y as f32),
                (xi as f32, (ruler_y + 6) as f32),
                dim_color,
            );

            draw_text_mut(
                &mut canvas,
                dim_color,
                xi - 20,
                ruler_y + 8,
                PxScale::from(14.0),
                f,
                &format!("{:.2}s", t),
            );
        }

        // Silence legend
        if !silences.is_empty() {
            let legend_txt = format!(
                "shaded bands = silences ≥ 400ms ({} gap(s))",
                silences.len()
            );
            draw_text_mut(
                &mut canvas,
                dim_color,
                strip_x0,
                label_y + 30,
                PxScale::from(14.0),
                f,
                &legend_txt,
            );
        }
    }

    if let Some(parent) = out_path.parent() {
        fs::create_dir_all(parent)?;
    }

    canvas
        .save(out_path)
        .with_context(|| format!("Failed to save timeline view PNG to {:?}", out_path))?;

    if let Ok(meta) = fs::metadata(out_path) {
        println!("saved: {:?} ({} KB)", out_path, meta.len() / 1024);
    }

    Ok(())
}
