use video_use::timeline_view::load_system_font;

#[test]
fn test_load_system_font_resolves() {
    let font = load_system_font();
    // On macOS and Linux desktop/servers with standard fonts, this resolves
    if cfg!(target_os = "macos") {
        assert!(font.is_some(), "Expected macOS system font to resolve");
    }
}
