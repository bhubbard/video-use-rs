---
name: video-use
description: Edit any video by conversation using the fast Rust port of video-use. Transcribe, cut, color grade, generate overlay animations, burn subtitles — for talking heads, montages, tutorials, travel, interviews. Production-correctness rules are hard; everything else is artistic freedom.
---

# Video Use (Rust Port)

Fast conversation-driven video editing toolkit built in Rust.

## Core Binary

All operations are executed via the single `video-use` binary:

```bash
video-use <COMMAND> [OPTIONS]
```

### Commands

- **`video-use transcribe <video> [OPTIONS]`** — Single-file Scribe call with track selection and silent track checks.
- **`video-use transcribe-batch <videos_dir> [--workers 4]`** — Parallel batch transcription of raw footage.
- **`video-use pack-transcripts --edit-dir <dir>`** — Packs `transcripts/*.json` into `takes_packed.md` breaking on silences $\ge 0.5\text{s}$ or speaker change.
- **`video-use timeline-view <video> <start> <end> [-o out.png]`** — Filmstrip + waveform composite PNG for visual inspection at decision points.
- **`video-use grade <input> -o <output> [--preset <name> | --filter <raw>]`** — Color grading with presets (`subtle`, `neutral_punch`, `warm_cinematic`) or data-driven auto mode.
- **`video-use render <edl.json> -o <out.mp4> [--preview | --draft] [--build-subtitles]`** — Complete render pipeline: per-segment extract $\to$ concat $\to$ overlays $\to$ subtitles LAST $\to$ 2-pass loudness normalization.

## Hard Rules (Production Correctness)

1. **Subtitles are applied LAST in the filter chain**, after every overlay.
2. **Per-segment extract $\to$ lossless `-c copy` concat**, avoiding re-encoding full clips when overlays are added.
3. **30ms audio fades at every segment boundary** (`afade=t=in:st=0:d=0.03,afade=t=out:st={dur-0.03}:d=0.03`) to prevent pops.
4. **Overlays use `setpts=PTS-STARTPTS+T/TB`** to align frame 0 with window start.
5. **Master SRT uses output-timeline offsets**: `output_time = word.start - segment_start + segment_offset`.
6. **Never cut inside a word.** Snap cut edges to word boundaries from the transcript.
7. **Pad cut edges** (working window: 30–200ms).
8. **Word-level verbatim ASR only.**
9. **Cache transcripts per source.**
10. **All session outputs in `<videos_dir>/edit/`.**
