//! Canonical Nucleus styling tokens shared by every rendering backend.

include!(concat!(env!("OUT_DIR"), "/style_tokens.rs"));

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_dark_and_market_tokens_are_exact() {
        assert_eq!(DEFAULT_THEME_NAME, "dark");
        assert_eq!(DARK_SURFACE_CSS, "#0c0c0c");
        assert_eq!(DARK_BORDER_CSS, "#1e1e1e");
        assert_eq!(DARK_AXIS_TEXT_CSS, "#f5f5f5");
        assert_eq!(DARK_CROSSHAIR_CSS, "#2e2e2e");
        assert_eq!(DARK_SEPARATOR_HOVER_CSS, "rgba(46, 46, 46, 0.2)");
        assert_eq!(MARKET_UP_CSS, "#089981");
        assert_eq!(MARKET_DOWN_CSS, "#f7525f");
        assert_eq!(DEFAULT_SURFACE_CSS, DARK_SURFACE_CSS);
        assert_eq!(DEFAULT_BORDER_CSS, DARK_BORDER_CSS);
        assert_eq!(DEFAULT_AXIS_TEXT_CSS, DARK_AXIS_TEXT_CSS);
        assert_eq!(DEFAULT_CROSSHAIR_CSS, DARK_CROSSHAIR_CSS);
        assert_eq!(DEFAULT_SEPARATOR_HOVER_CSS, DARK_SEPARATOR_HOVER_CSS);
    }
}
