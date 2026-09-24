# video-use-rs

A fast, production-grade Rust port of [browser-use/video-use](https://github.com/browser-use/video-use) — the conversation-driven video editing toolkit.

`video-use-rs` replaces the original Python helper scripts with a high-performance, single-binary CLI and reusable Rust library (`video_use`), providing complete support for the video-use editing pipeline.

---

## Features

- **Transcribe (ElevenLabs Scribe)**:
  - Single-file (`transcribe`) and parallel batch transcription (`transcribe-batch`) with worker thread pool.
  - Automatic detection and selection of audio tracks (`--audio-track`), checking peak dBFS to prevent paying for silent tracks.
  - Transcript caching per source file.
- **Transcript Packing (`pack-transcripts`)**:
  - Groups word-level Scribe JSON tokens into phrase-level markdown lines (`takes_packed.md`).
  - Breaks on silence gaps ($\ge 0.5\text{s}$) or speaker diarization changes.
  - Prepends `[start-end]` aligned timecode ranges and speaker tags (`S0`, `S1`).
- **Caption & Subtitle Engine**:
  - Word chunking algorithm matching natural speech boundaries (punctuation breaks, gaps $\ge 0.3\text{s}$, 2-word targets, minimum duration $0.35\text{s}$).
  - `build_master_srt` mapping word timestamps to output-timeline offsets across all cuts in the EDL.
  - Subtitles applied **LAST** in the compositing filter chain with platform safe-zone styling (`MarginV=90`, Helvetica 18 Bold).
- **Per-Segment Extract & Lossless Concat**:
  - Extracts cut ranges with 30ms audio fades at both edges (`afade=t=in:st=0:d=0.03,afade=t=out:st={dur-0.03}:d=0.03`) to eliminate audible pops.
  - Preserves native portrait orientation (swapping coded dimensions when display-matrix rotation side-data is $90^\circ$ or $270^\circ$).
  - Automatic HDR $\to$ SDR tone mapping (`zscale + tonemap` chain for smpte2084 and arib-std-b67 sources).
  - Exact rational frame rate probing and matching (`parse_fps` / `probe_source_fps`).
  - Lossless `-c copy` concatenation via the ffmpeg concat demuxer.
- **Color Grading (`grade`)**:
  - Built-in presets: `subtle`, `neutral_punch`, `warm_cinematic`, `none`.
  - Data-driven **Auto Mode**: samples clip frames via ffmpeg `signalstats`, detects native 8-bit / 10-bit bit depth, computes luma mean, range, and saturation, and produces bounded adjustments ($\pm 8\%$).
- **Two-Pass Loudness Normalization**:
  - Target social-media standards: **$-14\text{ LUFS}$ integrated, $-1\text{ dBTP}$ true peak, $\text{LRA } 11$**.
  - Pass 1 measurement parsing with Pass 2 linear normalization; 1-pass fast preview mode.
- **Visual Drill-Down (`timeline-view`)**:
  - Generates a composite PNG with an evenly spaced filmstrip, waveform audio envelope, shaded silence gaps ($\ge 400\text{ms}$), word label ticks, and a time ruler.
  - Rendered entirely in Rust using `image`, `imageproc`, and `ab_glyph`.

---

## Installation

### Prerequisites

- [Rust & Cargo](https://rustup.rs/) (1.75+)
- `ffmpeg` & `ffprobe` (version 5.0+ recommended)

```bash
# macOS
brew install ffmpeg

# Debian/Ubuntu
sudo apt-get update && sudo apt-get install -y ffmpeg
```

### Building from Source

```bash
git clone https://github.com/brandon-hubbard/video-use-rs # or local clone
cd video-use-rs
cargo build --release
```

The compiled binary will be located at `target/release/video-use`. You can copy or symlink it into your `$PATH` (e.g. `~/.cargo/bin/` or `/usr/local/bin/`).

---

## CLI Usage

### 1. Transcribe Footage

Transcribe a single clip:
```bash
video-use transcribe footage.mp4
# Or specify language, speakers, and audio track
video-use transcribe footage.mp4 --language en --num-speakers 2 --audio-track 0
```

Batch transcribe an entire folder with parallel workers:
```bash
video-use transcribe-batch /path/to/videos/ --workers 4
```

Transcripts are cached in `<videos_dir>/edit/transcripts/<name>.json`.

### 2. Pack Transcripts

Convert all raw Scribe JSON transcripts into the LLM-friendly phrase-level markdown view:
```bash
video-use pack-transcripts --edit-dir ./edit
# Custom silence threshold (default 0.5s)
video-use pack-transcripts --edit-dir ./edit --silence-threshold 0.4 -o ./edit/takes_packed.md
```

### 3. Visual Timeline Drill-Down

Generate a filmstrip + waveform composite PNG for any time window:
```bash
video-use timeline-view footage.mp4 12.5 18.0 -o ./edit/verify/window.png --n-frames 10
```

### 4. Color Grade

List available presets:
```bash
video-use grade --list-presets
```

Analyze clip stats and preview auto-grade filter:
```bash
video-use grade --analyze footage.mp4
```

Apply a preset or auto grade:
```bash
# Auto mode (data-driven correction)
video-use grade input.mp4 -o graded.mp4

# Named preset
video-use grade input.mp4 -o graded.mp4 --preset warm_cinematic
```

### 5. Render EDL

Render the final video from an `edl.json` specification:
```bash
video-use render ./edit/edl.json -o ./edit/final.mp4 --build-subtitles
```

Options:
- `--preview`: Render at 1080p medium CRF 22 (faster preview for QA)
- `--draft`: Render at 720p ultrafast CRF 28 (cut-point verification)
- `--build-subtitles`: Build `master.srt` automatically from transcripts and cut offsets
- `--no-subtitles`: Omit subtitle burning
- `--no-loudnorm`: Skip two-pass audio loudness normalization
- `--fps <rate>`: Force output frame rate (e.g. `30`, `60`, `30000/1001`)

---

## EDL Schema

`edl.json` defines cut decisions, overlays, and color grade:

```json
{
  "sources": {
    "C0103": "C0103.mp4",
    "C0104": "C0104.mp4"
  },
  "ranges": [
    {
      "source": "C0103",
      "start": 2.52,
      "end": 6.74,
      "beat": "intro hook"
    },
    {
      "source": "C0104",
      "start": 10.10,
      "end": 14.80,
      "beat": "problem statement"
    }
  ],
  "grade": "auto",
  "overlays": [
    {
      "file": "animations/slot_0/render.mp4",
      "start_in_output": 3.5,
      "duration": 2.0
    }
  ],
  "subtitles": "master.srt"
}
```

---

## Rust Library API

`video-use-rs` can be embedded directly into other Rust applications:

```rust
use video_use::{
    render_edl, RenderOptions,
    group_into_phrases, ScribeTranscript,
    auto_grade_for_clip, parse_fps,
};
use std::path::Path;

fn main() -> anyhow::Result<()> {
    // Canonicalize frame rates
    let fps = parse_fps("29.97")?;
    assert_eq!(fps, "2997/100");

    // Render an edit decision list
    render_edl(RenderOptions {
        edl_path: Path::new("edit/edl.json"),
        output_path: Path::new("edit/final.mp4"),
        preview: false,
        draft: false,
        build_subtitles: true,
        no_subtitles: false,
        no_loudnorm: false,
        fps: None,
    })?;

    Ok(())
}
```

---

## Testing & Code Coverage

### Run Tests

Run the full test suite across all targets:

```bash
cargo test
```

All 37 unit and integration tests (covering EDL deserialization, rational FPS parsing, orientation detection, Scribe path resolution, loudness normalization, audio peak checks, and caption chunking) are verified.

### Code Coverage

Code coverage is powered by [`cargo-llvm-cov`](https://github.com/taiki-e/cargo-llvm-cov) leveraging LLVM source-based code coverage.

1. **Install toolchain components and cargo-llvm-cov**:
   ```bash
   rustup component add llvm-tools-preview
   cargo install cargo-llvm-cov --locked
   ```

2. **Run code coverage in terminal**:
   ```bash
   cargo llvm-cov
   ```

3. **Generate an interactive HTML coverage report**:
   ```bash
   cargo llvm-cov --html --open
   ```

4. **Export LCOV for CI**:
   ```bash
   cargo llvm-cov --lcov --output-path lcov.info
   ```

Automated CI runs on every push and pull request via GitHub Actions ([`.github/workflows/ci.yml`](.github/workflows/ci.yml)), executing tests, generating coverage summaries in the job summary, and uploading HTML and LCOV coverage artifacts.

---

## Performance Benchmarks (Rust vs Python)

Empirical benchmarks comparing `video-use-rs` against the original Python `video-use` were conducted on Apple Silicon under identical inputs and workloads. See the full report in [BENCHMARKS.md](BENCHMARKS.md).

### 1. CLI Invocation Latency & Startup Overhead

*Measured over 15 trials per subcommand with resident memory tracking.*

| Subcommand | Original Python | `video-use-rs` (Rust) | Speedup | Python RSS | Rust RSS | Memory Reduction |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: |
| `grade (--help)` | 235.63 ms | **9.48 ms** | **24.9x faster** | 21.3 MB | **6.9 MB** | **3.1x lower** |
| `timeline-view (--help)` | 133.08 ms | **9.24 ms** | **14.4x faster** | 35.5 MB | **6.9 MB** | **5.2x lower** |
| `render (--help)` | 75.83 ms | **9.15 ms** | **8.3x faster** | 22.7 MB | **6.9 MB** | **3.3x lower** |
| `pack-transcripts (--help)` | 57.32 ms | **7.87 ms** | **7.3x faster** | 19.9 MB | **6.9 MB** | **2.9x lower** |

### 2. Scribe Transcript Processing & Silence Segmentation

*Parsing multi-take word timestamp JSON, detecting $\ge 0.5\text{s}$ silences & speaker changes, and generating phrase markdown.*

| Workload | Python Time | Rust Time | Speedup | Python Throughput | Rust Throughput | Rust RSS |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: |
| **10 clips (10,000 words)** | 73.69 ms | **16.27 ms** | **4.5x faster** | 135,699 words/s | **614,590 words/s** | 8.3 MB |
| **50 clips (50,000 words)** | 120.91 ms | **38.32 ms** | **3.2x faster** | 413,546 words/s | **1,304,876 words/s** | 9.8 MB |
| **100 clips (150,000 words)** | 244.70 ms | **93.24 ms** | **2.6x faster** | 613,000 words/s | **1,608,684 words/s** | 13.0 MB |

### 3. Media Processing & Drill-Down Operations

| Operation | Python Time | Rust Time | Speedup | Parity / Accuracy |
| :--- | :---: | :---: | :---: | :---: |
| **Timeline View (10-frame composite PNG)**<br>*(Frame extraction, 16kHz PCM WAV decoding, windowed RMS, composite)* | 1,041.04 ms | **643.30 ms** | **1.6x faster** | Identical visual filmstrip & audio waveform |
| **Auto Color-Grade (20-frame signalstats)** | 213.87 ms | **210.00 ms** | **1.0x** *(ffmpeg bound)* | Exact match down to 17 decimal places |
| **EDL Render Engine (2 cuts + 30ms fades)** | 685.29 ms | **741.83 ms** | **1.0x** *(ffmpeg bound)* | Exact lossless concat stream parity |

### Key Architectural Takeaways

1. **Zero-Overhead Agent Invocations**: In iterative agent loops (Claude Code, Antigravity, Codex), Python's import overhead (`numpy`, `PIL`, `requests`) adds **130ms – 235ms** of latency per tool call. Rust launches in **7ms – 9ms** (**up to 25x faster**).
2. **Massive Parsing Throughput**: Rust processes **>1.6 million words per second** of Scribe speech JSON via `serde_json`, processing hours of raw footage in milliseconds.
3. **Drastic Memory Savings**: Rust maintains a baseline memory footprint of **6.9 MB** (up to **5.2x lower** than Python's 35.5 MB), enabling safe concurrent execution across multiple worker threads.
4. **100% Mathematical Parity**: Audio loudness normalization, signalstats histogram luminance/contrast formulas, and frame rate rational conversions match Python with zero regression.

### Reproducing Benchmarks

To re-run the benchmark suite locally:

```bash
cargo build --release
python3 benches/benchmark.py
```

---

## License

MIT License. See [LICENSE](LICENSE) for details.
