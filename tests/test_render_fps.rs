use serde_json::json;
use video_use::fps::{parse_fps, probe_fps_from_json};

#[test]
fn test_prefers_average_rate() {
    let probe = json!({
        "streams": [{
            "avg_frame_rate": "30000/1001",
            "r_frame_rate": "30/1"
        }]
    });
    assert_eq!(probe_fps_from_json(&probe), Some("30000/1001".to_string()));
}

#[test]
fn test_falls_back_to_nominal_rate() {
    let probe = json!({
        "streams": [{
            "avg_frame_rate": "0/0",
            "r_frame_rate": "60/1"
        }]
    });
    assert_eq!(probe_fps_from_json(&probe), Some("60/1".to_string()));
}

#[test]
fn test_returns_none_for_unusable_probe_output() {
    let probe = json!({"streams": []});
    assert_eq!(probe_fps_from_json(&probe), None);

    let probe2 = json!({
        "streams": [{
            "avg_frame_rate": "0/0",
            "r_frame_rate": "0/0"
        }]
    });
    assert_eq!(probe_fps_from_json(&probe2), None);
}

#[test]
fn test_fps_canonicalization_matrix() {
    let cases = [
        ("24", "24/1"),
        ("25", "25/1"),
        ("30", "30/1"),
        ("60", "60/1"),
        ("29.97", "2997/100"),
        ("59.94", "2997/50"),
        ("30000/1001", "30000/1001"),
        ("60000/1001", "60000/1001"),
    ];

    for (raw, canonical) in cases {
        let parsed = parse_fps(raw).unwrap();
        assert_eq!(parsed, canonical);
        // Idempotent
        assert_eq!(parse_fps(&parsed).unwrap(), canonical);
    }
}
