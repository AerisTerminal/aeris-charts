//! Canonical Aeris styling tokens shared by every rendering backend.

include!(concat!(env!("OUT_DIR"), "/style_tokens.rs"));

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_dark_and_market_tokens_are_exact() {
        assert_eq!(DEFAULT_THEME_NAME, "dark");
        assert_eq!(BORDER_WIDTH, 0.5);
        assert_eq!(RADIUS_SMALL, 4.0);
        assert_eq!(RADIUS_DEFAULT, 8.0);
        assert_eq!(RADIUS_LARGE, 999.0);
        assert_eq!(LIGHT_SURFACE_CSS, "#ffffff");
        assert_eq!(LIGHT_FOREGROUND_CSS, "#333333");
        assert_eq!(LIGHT_MUTED_CSS, "#fafafa");
        assert_eq!(LIGHT_MUTED_FOREGROUND_CSS, "#7b7b7b");
        assert_eq!(LIGHT_PRIMARY_CSS, "#168ef7");
        assert_eq!(LIGHT_PRIMARY_FOREGROUND_CSS, "#ffffff");
        assert_eq!(LIGHT_PRIMARY_HOVER_CSS, LIGHT_PRIMARY_CSS);
        assert_eq!(LIGHT_ACCENT_CSS, "#f7f7f7");
        assert_eq!(LIGHT_BORDER_CSS, "#f1f1f1");
        assert_eq!(LIGHT_MUTED_BORDER_CSS, LIGHT_BORDER_CSS);
        assert_eq!(LIGHT_RING_CSS, "#d0d0d0");
        assert_eq!(DARK_SURFACE_CSS, "#141414");
        assert_eq!(DARK_FOREGROUND_CSS, "#f0f0f0");
        assert_eq!(DARK_MUTED_CSS, "#181818");
        assert_eq!(DARK_MUTED_FOREGROUND_CSS, "#b7b7b7");
        assert_eq!(DARK_PRIMARY_CSS, "#168ef7");
        assert_eq!(DARK_PRIMARY_FOREGROUND_CSS, "#ffffff");
        assert_eq!(DARK_PRIMARY_HOVER_CSS, DARK_PRIMARY_CSS);
        assert_eq!(DARK_BORDER_CSS, "#252525");
        assert_eq!(DARK_CROSSHAIR_CSS, DARK_BORDER_CSS);
        assert_eq!(DARK_CROSSHAIR_LABEL_CSS, DARK_BORDER_CSS);
        // Crosshair chrome is a separate semantic role and keeps the supplied pre-existing
        // dark neutral even though the light theme's primary text moved to #333333.
        assert_eq!(LIGHT_CROSSHAIR_CSS, "#141414");
        assert_eq!(LIGHT_CROSSHAIR_LABEL_CSS, "#141414");
        assert_eq!(DARK_SEPARATOR_HOVER_CSS, DARK_ACCENT_CSS);
        assert_eq!(LIGHT_MARKET_UP_CSS, "#089981");
        assert_eq!(LIGHT_MARKET_DOWN_CSS, "#f7525f");
        assert_eq!(DARK_MARKET_UP_CSS, "#7c8db0");
        assert_eq!(DARK_MARKET_DOWN_CSS, "#98615c");
        assert_eq!(MARKET_UP_CSS, LIGHT_MARKET_UP_CSS);
        assert_eq!(MARKET_DOWN_CSS, LIGHT_MARKET_DOWN_CSS);
        assert_eq!(DEFAULT_SURFACE_CSS, DARK_SURFACE_CSS);
        assert_eq!(DEFAULT_BORDER_CSS, DARK_BORDER_CSS);
        assert_eq!(DEFAULT_FOREGROUND_CSS, DARK_FOREGROUND_CSS);
        assert_eq!(DEFAULT_MUTED_CSS, DARK_MUTED_CSS);
        assert_eq!(DEFAULT_SEPARATOR_HOVER_CSS, DARK_SEPARATOR_HOVER_CSS);
    }
}
