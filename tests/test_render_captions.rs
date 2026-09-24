use std::fs;
use video_use::subtitles::{build_master_srt_from_ranges, resolve_subtitles_path};
use video_use::transcript::{ScribeTranscript, ScribeWord};

fn w(text: &str, start: f64, end: f64) -> ScribeWord {
    ScribeWord::new_word(text, start, end)
}

fn real_take() -> Vec<ScribeWord> {
    vec![
        w("90%", 2.64, 3.04),
        w("of", 3.06, 3.14),
        w("what", 3.16, 3.26),
        w("a", 3.30, 3.34),
        w("web", 3.36, 3.50),
        w("agent", 3.54, 3.82),
        w("does", 3.86, 4.14),
        w("is", 4.48, 4.56),
        w("completely", 4.58, 4.90),
        w("wasted.", 4.94, 5.34),
        w("We", 6.08, 6.18),
        w("fix", 6.24, 6.42),
        w("this.", 6.48, 6.70),
    ]
}

#[test]
fn test_output_timeline_cues() {
    let tmp = tempfile::tempdir().unwrap();
    let edit = tmp.path();
    let transcripts_dir = edit.join("transcripts");
    fs::create_dir_all(&transcripts_dir).unwrap();

    let take = real_take();
    let transcript = ScribeTranscript {
        words: take.clone(),
        extra: Default::default(),
    };
    fs::write(
        transcripts_dir.join("C0103.json"),
        serde_json::to_string(&transcript).unwrap(),
    )
    .unwrap();

    let ranges = vec![("C0103".to_string(), 2.55, 6.8)];
    let out = edit.join("master.srt");

    let count = build_master_srt_from_ranges(
        &ranges,
        |src_name| {
            let p = transcripts_dir.join(format!("{src_name}.json"));
            if p.exists() {
                let content = fs::read_to_string(p)?;
                Ok(Some(serde_json::from_str(&content)?))
            } else {
                Ok(None)
            }
        },
        &out,
    )
    .unwrap();

    assert_eq!(count, 6);
    let srt_content = fs::read_to_string(&out).unwrap();
    let cues: Vec<&str> = srt_content.trim().split("\n\n").collect();
    assert_eq!(cues.len(), 6);

    let first_cue_lines: Vec<&str> = cues[0].lines().collect();
    assert_eq!(first_cue_lines[0], "1");
    assert_eq!(first_cue_lines[1], "00:00:00,090 --> 00:00:00,590");
    assert_eq!(first_cue_lines[2], "90% OF");

    let cue_texts: Vec<&str> = cues
        .iter()
        .map(|c| c.lines().nth(2).unwrap().trim())
        .collect();
    assert_eq!(
        cue_texts,
        vec![
            "90% OF",
            "WHAT A WEB",
            "AGENT DOES",
            "IS COMPLETELY",
            "WASTED.",
            "WE FIX THIS."
        ]
    );
}

#[test]
fn test_resolve_subtitles_path() {
    let tmp = tempfile::tempdir().unwrap();
    let edit = tmp.path();
    let master = edit.join("master.srt");
    fs::write(&master, "").unwrap();

    let resolved = resolve_subtitles_path("master.srt", edit).unwrap();
    assert_eq!(resolved, master);

    let missing = resolve_subtitles_path("nope.srt", edit);
    assert!(missing.is_err());
}
