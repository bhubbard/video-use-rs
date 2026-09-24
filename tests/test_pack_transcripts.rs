use std::fs;
use video_use::transcript::{pack_transcripts, render_markdown, Phrase};

#[test]
fn test_render_markdown_format() {
    let entries = vec![
        (
            "C0103".to_string(),
            43.0,
            vec![
                Phrase {
                    start: 2.52,
                    end: 5.36,
                    text: "Ninety percent of what a web agent does is completely wasted.".to_string(),
                    speaker_id: Some("speaker_0".to_string()),
                },
                Phrase {
                    start: 6.08,
                    end: 6.74,
                    text: "We fixed this.".to_string(),
                    speaker_id: Some("0".to_string()),
                },
            ],
        ),
        ("C0104".to_string(), 0.0, vec![]),
    ];

    let md = render_markdown(&entries, 0.5);
    assert!(md.contains("# Packed transcripts"));
    assert!(md.contains("## C0103  (duration: 43.0s, 2 phrases)"));
    assert!(md.contains("  [002.52-005.36] S0 Ninety percent of what a web agent does is completely wasted."));
    assert!(md.contains("  [006.08-006.74] S0 We fixed this."));
    assert!(md.contains("## C0104  (duration: 0.0s, 0 phrases)"));
    assert!(md.contains("  _no speech detected_"));
}

#[test]
fn test_pack_transcripts_from_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let edit_dir = tmp.path();
    let tr_dir = edit_dir.join("transcripts");
    fs::create_dir_all(&tr_dir).unwrap();

    let tr_json = r#"{
        "words": [
            {"type": "word", "text": "Hello", "start": 0.5, "end": 1.0, "speaker_id": "speaker_1"},
            {"type": "word", "text": "world", "start": 1.1, "end": 1.5, "speaker_id": "speaker_1"}
        ]
    }"#;
    fs::write(tr_dir.join("clip1.json"), tr_json).unwrap();

    let out_file = pack_transcripts(edit_dir, 0.5, None).unwrap();
    assert!(out_file.exists());
    let md = fs::read_to_string(&out_file).unwrap();
    assert!(md.contains("## clip1"));
    assert!(md.contains("Hello world"));
    assert!(md.contains("S1"));
}
