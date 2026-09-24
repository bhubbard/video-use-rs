use anyhow::{bail, Result};
use clap::{Args, Parser, Subcommand};
use std::path::{Path, PathBuf};
use video_use::grade::{apply_grade, auto_grade_for_clip, get_preset, presets};
use video_use::render::{render_edl, RenderOptions};
use video_use::scribe::{load_api_key, transcribe_batch, transcribe_one};
use video_use::timeline_view::render_timeline;
use video_use::transcript::pack_transcripts;

#[derive(Parser)]
#[command(name = "video-use")]
#[command(about = "Fast conversation-driven video editing toolkit in Rust", version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Transcribe a single video using ElevenLabs Scribe
    Transcribe(TranscribeArgs),

    /// Batch-transcribe every video in a directory with parallel workers
    TranscribeBatch(TranscribeBatchArgs),

    /// Pack Scribe JSON transcripts into takes_packed.md
    PackTranscripts(PackTranscriptsArgs),

    /// Apply or inspect color grading presets and data-driven auto grade
    Grade(GradeArgs),

    /// Render a video from an EDL (extract -> concat -> overlays -> subtitles -> loudnorm)
    Render(RenderArgs),

    /// Generate a filmstrip + waveform composite PNG for a time range
    TimelineView(TimelineViewArgs),
}

#[derive(Args)]
struct TranscribeArgs {
    /// Path to video file
    video: PathBuf,

    /// Edit output directory (default: <video_parent>/edit)
    #[arg(long)]
    edit_dir: Option<PathBuf>,

    /// Optional ISO language code (e.g. 'en')
    #[arg(long)]
    language: Option<String>,

    /// Optional number of speakers when known
    #[arg(long)]
    num_speakers: Option<usize>,

    /// Zero-based audio track to transcribe (OBS: 0 = game, 1 = mic)
    #[arg(long, default_value_t = 0)]
    audio_track: usize,
}

#[derive(Args)]
struct TranscribeBatchArgs {
    /// Directory containing source videos
    videos_dir: PathBuf,

    /// Edit output directory (default: <videos_dir>/edit)
    #[arg(long)]
    edit_dir: Option<PathBuf>,

    /// Number of parallel workers
    #[arg(long, default_value_t = 4)]
    workers: usize,

    /// Optional ISO language code
    #[arg(long)]
    language: Option<String>,

    /// Optional number of speakers when known
    #[arg(long)]
    num_speakers: Option<usize>,

    /// Zero-based audio track to transcribe
    #[arg(long, default_value_t = 0)]
    audio_track: usize,
}

#[derive(Args)]
struct PackTranscriptsArgs {
    /// Edit directory containing transcripts/
    #[arg(long)]
    edit_dir: PathBuf,

    /// Break phrases on silences >= this (seconds)
    #[arg(long, default_value_t = 0.5)]
    silence_threshold: f64,

    /// Output path (default: <edit-dir>/takes_packed.md)
    #[arg(short, long)]
    output: Option<PathBuf>,
}

#[derive(Args)]
struct GradeArgs {
    /// Input video file
    input: Option<PathBuf>,

    /// Output video file
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Grade preset name (e.g. subtle, neutral_punch, warm_cinematic, none)
    #[arg(long)]
    preset: Option<String>,

    /// Raw ffmpeg filter string (overrides --preset)
    #[arg(long)]
    filter: Option<String>,

    /// Analyze a clip and print auto-grade filter and stats (no output written)
    #[arg(long)]
    analyze: Option<PathBuf>,

    /// Print the filter string for a preset and exit
    #[arg(long)]
    print_preset: Option<String>,

    /// List available presets and exit
    #[arg(long)]
    list_presets: bool,
}

#[derive(Args)]
struct RenderArgs {
    /// Path to edl.json
    edl: PathBuf,

    /// Output video path
    #[arg(short, long)]
    output: PathBuf,

    /// Preview mode: 1080p, medium CRF 22 (evaluable for QC)
    #[arg(long)]
    preview: bool,

    /// Draft mode: 720p, ultrafast CRF 28 (cut-point check only)
    #[arg(long)]
    draft: bool,

    /// Build master.srt from transcripts + EDL offsets before compositing
    #[arg(long)]
    build_subtitles: bool,

    /// Skip subtitles even if EDL references one
    #[arg(long)]
    no_subtitles: bool,

    /// Skip audio loudness normalization
    #[arg(long)]
    no_loudnorm: bool,

    /// Output frame rate (e.g. 30, 30000/1001, 24)
    #[arg(long)]
    fps: Option<String>,
}

#[derive(Args)]
struct TimelineViewArgs {
    /// Source video
    video: PathBuf,

    /// Start time in seconds
    start: f64,

    /// End time in seconds
    end: f64,

    /// Output PNG path
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Number of frames in the filmstrip
    #[arg(long, default_value_t = 10)]
    n_frames: usize,

    /// Path to transcript.json for word labels + silence shading
    #[arg(long)]
    transcript: Option<PathBuf>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Transcribe(args) => {
            let video = args.video.canonicalize().unwrap_or(args.video.clone());
            if !video.exists() {
                bail!("video not found: {:?}", video);
            }
            let edit_dir = args
                .edit_dir
                .unwrap_or_else(|| video.parent().unwrap_or_else(|| Path::new(".")).join("edit"));
            let api_key = load_api_key(video.parent())?;
            transcribe_one(
                &video,
                &edit_dir,
                &api_key,
                args.language.as_deref(),
                args.num_speakers,
                true,
                args.audio_track,
            )?;
        }
        Commands::TranscribeBatch(args) => {
            let v_dir = args.videos_dir.canonicalize().unwrap_or(args.videos_dir);
            transcribe_batch(
                &v_dir,
                args.edit_dir.as_deref(),
                args.workers,
                args.language.as_deref(),
                args.num_speakers,
                args.audio_track,
            )?;
        }
        Commands::PackTranscripts(args) => {
            let edit_dir = args.edit_dir.canonicalize().unwrap_or(args.edit_dir);
            pack_transcripts(&edit_dir, args.silence_threshold, args.output.as_deref())?;
        }
        Commands::Grade(args) => {
            if args.list_presets {
                for (name, f) in presets() {
                    println!("{}:", name);
                    if f.is_empty() {
                        println!("  (no filter)");
                    } else {
                        println!("  {}", f);
                    }
                    println!();
                }
                return Ok(());
            }

            if let Some(p_name) = args.print_preset {
                let filter = get_preset(&p_name)?;
                println!("{}", filter);
                return Ok(());
            }

            if let Some(analyze_path) = args.analyze {
                if !analyze_path.exists() {
                    bail!("input not found: {:?}", analyze_path);
                }
                let (filter, stats) = auto_grade_for_clip(&analyze_path, 0.0, None, true)?;
                println!(
                    "\nfilter: {}",
                    if filter.is_empty() { "(none)" } else { &filter }
                );
                println!(
                    "stats:  {}",
                    serde_json::to_string_pretty(&stats).unwrap_or_default()
                );
                return Ok(());
            }

            let input = match args.input {
                Some(i) => i,
                None => bail!("input and -o/--output are required unless using --analyze, --print-preset, or --list-presets"),
            };
            let output = match args.output {
                Some(o) => o,
                None => bail!("-o/--output is required"),
            };

            if !input.exists() {
                bail!("input not found: {:?}", input);
            }

            let filter_string = if let Some(raw_f) = args.filter {
                raw_f
            } else if let Some(p_name) = args.preset {
                get_preset(&p_name)?.to_string()
            } else {
                let (auto_f, _) = auto_grade_for_clip(&input, 0.0, None, true)?;
                auto_f
            };

            println!(
                "grading {:?} → {:?}",
                input.file_name().unwrap_or_default(),
                output.file_name().unwrap_or_default()
            );
            if filter_string.is_empty() {
                println!("  filter: (none — copy)");
            } else {
                let display_f: String = filter_string.chars().take(120).collect();
                println!(
                    "  filter: {}{}",
                    display_f,
                    if filter_string.len() > 120 { "..." } else { "" }
                );
            }

            apply_grade(&input, &output, &filter_string)?;
            println!("done: {:?}", output);
        }
        Commands::Render(args) => {
            render_edl(RenderOptions {
                edl_path: &args.edl,
                output_path: &args.output,
                preview: args.preview,
                draft: args.draft,
                build_subtitles: args.build_subtitles,
                no_subtitles: args.no_subtitles,
                no_loudnorm: args.no_loudnorm,
                fps: args.fps.as_deref(),
            })?;
        }
        Commands::TimelineView(args) => {
            if args.end <= args.start {
                bail!("end must be > start");
            }
            if !args.video.exists() {
                bail!("video not found: {:?}", args.video);
            }

            let transcript_p = args.transcript.or_else(|| {
                let auto = args
                    .video
                    .parent()
                    .unwrap_or_else(|| Path::new("."))
                    .join("edit")
                    .join("transcripts")
                    .join(format!(
                        "{}.json",
                        args.video.file_stem().and_then(|s| s.to_str()).unwrap_or("")
                    ));
                if auto.exists() {
                    Some(auto)
                } else {
                    None
                }
            });

            let out_path = args.output.unwrap_or_else(|| {
                let verify_dir = args
                    .video
                    .parent()
                    .unwrap_or_else(|| Path::new("."))
                    .join("edit")
                    .join("verify");
                let _ = std::fs::create_dir_all(&verify_dir);
                let stem = args.video.file_stem().and_then(|s| s.to_str()).unwrap_or("video");
                verify_dir.join(format!("{}_{:.2}-{:.2}.png", stem, args.start, args.end))
            });

            render_timeline(
                &args.video,
                args.start,
                args.end,
                &out_path,
                args.n_frames,
                transcript_p.as_deref(),
            )?;
        }
    }

    Ok(())
}
