use video_use::grade::{get_preset, presets, PRESET_NEUTRAL_PUNCH, PRESET_SUBTLE, PRESET_WARM_CINEMATIC};
use video_use::render::resolve_grade_filter;

#[test]
fn test_presets_exist() {
    let p = presets();
    assert!(p.contains_key("subtle"));
    assert!(p.contains_key("neutral_punch"));
    assert!(p.contains_key("warm_cinematic"));
    assert!(p.contains_key("none"));

    assert_eq!(get_preset("subtle").unwrap(), PRESET_SUBTLE);
    assert_eq!(get_preset("neutral_punch").unwrap(), PRESET_NEUTRAL_PUNCH);
    assert_eq!(get_preset("warm_cinematic").unwrap(), PRESET_WARM_CINEMATIC);
    assert_eq!(get_preset("none").unwrap(), "");
    assert!(get_preset("non_existent").is_err());
}

#[test]
fn test_resolve_grade_filter() {
    assert_eq!(resolve_grade_filter(None), "");
    assert_eq!(resolve_grade_filter(Some("")), "");
    assert_eq!(resolve_grade_filter(Some("auto")), "__AUTO__");
    assert_eq!(resolve_grade_filter(Some("subtle")), PRESET_SUBTLE);
    assert_eq!(
        resolve_grade_filter(Some("eq=contrast=1.1")),
        "eq=contrast=1.1"
    );
}
