//! Canonical Nucleus styling tokens shared by every rendering backend.

include!(concat!(env!("OUT_DIR"), "/style_tokens.rs"));

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_dark_and_market_tokens_are_exact() {
        assert_eq!(DEFAULT_THEME_NAME, "dark");
        assert_eq!(DARK_SURFACE_CSS, "#070a0f");
        assert_eq!(DARK_FOREGROUND_CSS, "#fafafa");
        assert_eq!(DARK_MUTED_CSS, "#0c1115");
        assert_eq!(DARK_MUTED_FOREGROUND_CSS, "#9da3aa");
        assert_eq!(DARK_BORDER_CSS, "#16191f");
        assert_eq!(DARK_CROSSHAIR_CSS, DARK_BORDER_CSS);
        assert_eq!(DARK_CROSSHAIR_LABEL_CSS, DARK_MUTED_CSS);
        assert_eq!(LIGHT_CROSSHAIR_CSS, LIGHT_FOREGROUND_CSS);
        assert_eq!(LIGHT_CROSSHAIR_LABEL_CSS, LIGHT_FOREGROUND_CSS);
        assert_eq!(DARK_SEPARATOR_HOVER_CSS, DARK_ACCENT_CSS);
        assert_eq!(MARKET_UP_CSS, "#089981");
        assert_eq!(MARKET_DOWN_CSS, "#f7525f");
        assert_eq!(DEFAULT_SURFACE_CSS, DARK_SURFACE_CSS);
        assert_eq!(DEFAULT_BORDER_CSS, DARK_BORDER_CSS);
        assert_eq!(DEFAULT_FOREGROUND_CSS, DARK_FOREGROUND_CSS);
        assert_eq!(DEFAULT_MUTED_CSS, DARK_MUTED_CSS);
        assert_eq!(DEFAULT_SEPARATOR_HOVER_CSS, DARK_SEPARATOR_HOVER_CSS);
    }
}
