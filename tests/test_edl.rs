use std::fs;
use std::path::Path;
use video_use::edl::{resolve_path, Edl};

#[test]
fn test_edl_deserialization() {
    let json_data = r#"{
        "sources": {
            "take1": "takes/take1.mp4",
            "take2": "/absolute/path/take2.mp4"
        },
        "ranges": [
            {
                "source": "take1",
                "start": 1.25,
                "end": 5.50,
                "beat": "hook",
                "note": "first take"
            }
        ],
        "grade": "auto",
        "overlays": [
            {
                "file": "overlay.mp4",
                "start_in_output": 2.0,
                "duration": 1.5
            }
        ],
        "subtitles": "subtitles.srt"
    }"#;

    let tmp = tempfile::tempdir().unwrap();
    let edl_file = tmp.path().join("edl.json");
    fs::write(&edl_file, json_data).unwrap();

    let edl = Edl::load_from_file(&edl_file).unwrap();
    assert_eq!(edl.sources.len(), 2);
    assert_eq!(edl.ranges.len(), 1);
    assert_eq!(edl.ranges[0].source, "take1");
    assert_eq!(edl.ranges[0].start, 1.25);
    assert_eq!(edl.ranges[0].end, 5.50);
    assert_eq!(edl.ranges[0].beat.as_deref(), Some("hook"));
    assert_eq!(edl.ranges[0].note.as_deref(), Some("first take"));
    assert_eq!(edl.grade.as_deref(), Some("auto"));
    assert_eq!(edl.subtitles.as_deref(), Some("subtitles.srt"));

    let overlays = edl.overlays.as_ref().unwrap();
    assert_eq!(overlays.len(), 1);
    assert_eq!(overlays[0].file, "overlay.mp4");
    assert_eq!(overlays[0].start_in_output, 2.0);
    assert_eq!(overlays[0].duration, 1.5);

    // Resolve paths
    let edit_dir = Path::new("/project/edit");
    let rel_p = edl.resolve_source_path("take1", edit_dir).unwrap();
    assert_eq!(rel_p, Path::new("/project/edit/takes/take1.mp4"));

    let abs_p = edl.resolve_source_path("take2", edit_dir).unwrap();
    assert_eq!(abs_p, Path::new("/absolute/path/take2.mp4"));

    assert!(edl.resolve_source_path("non_existent", edit_dir).is_none());

    // Resolve path utility
    assert_eq!(
        resolve_path("sub.srt", edit_dir),
        Path::new("/project/edit/sub.srt")
    );
    assert_eq!(
        resolve_path("/root/sub.srt", edit_dir),
        Path::new("/root/sub.srt")
    );
}
