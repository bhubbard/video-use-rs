use std::fs;
use std::path::Path;
use video_use::scribe::{find_videos, load_api_key, transcript_path};

#[test]
fn test_transcript_path_suffixes() {
    let edit = Path::new("/videos/edit");
    let video = Path::new("/videos/takes/C0103.MP4");

    let track0 = transcript_path(edit, video, 0);
    assert_eq!(track0, Path::new("/videos/edit/transcripts/C0103.json"));

    let track1 = transcript_path(edit, video, 1);
    assert_eq!(track1, Path::new("/videos/edit/transcripts/C0103.track1.json"));

    let track2 = transcript_path(edit, video, 2);
    assert_eq!(track2, Path::new("/videos/edit/transcripts/C0103.track2.json"));
}

#[test]
fn test_find_videos_filter_extensions() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();

    fs::write(dir.join("clip1.mp4"), "").unwrap();
    fs::write(dir.join("clip2.MOV"), "").unwrap();
    fs::write(dir.join("clip3.mkv"), "").unwrap();
    fs::write(dir.join("clip4.avi"), "").unwrap();
    fs::write(dir.join("clip5.m4v"), "").unwrap();
    fs::write(dir.join("notes.txt"), "").unwrap();
    fs::write(dir.join("image.jpg"), "").unwrap();

    let found = find_videos(dir).unwrap();
    assert_eq!(found.len(), 5);

    let filenames: Vec<String> = found
        .iter()
        .map(|p| p.file_name().unwrap().to_str().unwrap().to_string())
        .collect();

    assert!(filenames.contains(&"clip1.mp4".to_string()));
    assert!(filenames.contains(&"clip2.MOV".to_string()));
    assert!(filenames.contains(&"clip3.mkv".to_string()));
    assert!(filenames.contains(&"clip4.avi".to_string()));
    assert!(filenames.contains(&"clip5.m4v".to_string()));
    assert!(!filenames.contains(&"notes.txt".to_string()));
}

#[test]
fn test_load_api_key_from_env_file() {
    let tmp = tempfile::tempdir().unwrap();
    let env_file = tmp.path().join(".env");
    fs::write(
        &env_file,
        "# Comment line\n\nELEVENLABS_API_KEY=\"sk_test_1234567890\"\nOTHER_VAR=abc",
    )
    .unwrap();

    let key = load_api_key(Some(tmp.path())).unwrap();
    assert_eq!(key, "sk_test_1234567890");
}
