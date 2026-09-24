use serde_json::json;
use video_use::media::is_portrait_from_probe_json;

#[test]
fn test_native_portrait_dimensions() {
    let probe = json!({
        "streams": [{
            "width": 1080,
            "height": 1920
        }]
    });
    assert!(is_portrait_from_probe_json(&probe));
}

#[test]
fn test_native_landscape_dimensions() {
    let probe = json!({
        "streams": [{
            "width": 1920,
            "height": 1080
        }]
    });
    assert!(!is_portrait_from_probe_json(&probe));
}

#[test]
fn test_side_data_rotation_turns_coded_landscape_into_portrait() {
    let probe = json!({
        "streams": [{
            "width": 1920,
            "height": 1080,
            "side_data_list": [{"rotation": -90.0}]
        }]
    });
    assert!(is_portrait_from_probe_json(&probe));
}

#[test]
fn test_plain_rotation_tag_without_side_data_is_ignored() {
    let probe = json!({
        "streams": [{
            "width": 1920,
            "height": 1080,
            "tags": {"rotate": "270"}
        }]
    });
    assert!(!is_portrait_from_probe_json(&probe));
}

#[test]
fn test_rotation_can_turn_coded_portrait_into_landscape() {
    let probe = json!({
        "streams": [{
            "width": 1080,
            "height": 1920,
            "side_data_list": [{"rotation": 90.0}]
        }]
    });
    assert!(!is_portrait_from_probe_json(&probe));
}

#[test]
fn test_invalid_probe_output_falls_back_to_landscape() {
    let probe = json!({"streams": []});
    assert!(!is_portrait_from_probe_json(&probe));

    let probe2 = json!({"not_streams": 123});
    assert!(!is_portrait_from_probe_json(&probe2));
}
