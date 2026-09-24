use video_use::loudnorm::parse_loudnorm_stderr;

#[test]
fn test_parse_loudnorm_stderr() {
    let stderr = r#"
[Parsed_loudnorm_0 @ 0x123456] 
{
    "input_i" : "-22.45",
    "input_tp" : "-1.20",
    "input_lra" : "8.40",
    "input_thresh" : "-33.10",
    "output_i" : "-14.02",
    "output_tp" : "-1.00",
    "output_lra" : "7.10",
    "output_thresh" : "-24.50",
    "normalization_type" : "dynamic",
    "target_offset" : "0.02"
}
frame=  100 fps=0.0 q=-1.0 Lsize=N/A
"#;

    let measurement = parse_loudnorm_stderr(stderr).unwrap();
    assert_eq!(measurement.input_i, "-22.45");
    assert_eq!(measurement.input_tp, "-1.20");
    assert_eq!(measurement.input_lra, "8.40");
    assert_eq!(measurement.input_thresh, "-33.10");
    assert_eq!(measurement.target_offset, "0.02");

    // Invalid stderr without JSON
    assert!(parse_loudnorm_stderr("some error message").is_none());
    assert!(parse_loudnorm_stderr("{ broken json").is_none());
}
