//! Canonical Aeris styling tokens shared by every rendering backend.

include!(concat!(env!("OUT_DIR"), "/style_tokens.rs"));

/// `--border-width` in device pixels with browser border semantics: whole device pixels, rounded
/// down, never thinner than one. At DPR 1 and 2 this is one device pixel; at DPR 4, two.
pub fn border_width_device_px(pixel_ratio: f64) -> f64 {
    (BORDER_WIDTH * pixel_ratio).floor().max(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brand_border_snaps_like_a_browser_border() {
        for (ratio, expected) in [(1.0, 1.0), (1.5, 1.0), (2.0, 1.0), (3.0, 1.0), (4.0, 2.0)] {
            assert_eq!(border_width_device_px(ratio), expected, "dpr {ratio}");
        }
    }

    #[test]
    fn canonical_dark_and_market_tokens_are_exact() {
        assert_eq!(DEFAULT_THEME_NAME, "dark");
        assert_eq!(BORDER_WIDTH, 0.5);
        assert_eq!(RADIUS_SMALL, 4.0);
        assert_eq!(RADIUS_DEFAULT, 8.0);
        assert_eq!(RADIUS_MEDIUM, 12.0);
        assert_eq!(RADIUS_LARGE, 999.0);
        assert_eq!(LIGHT_SURFACE_CSS, "#ffffff");
        assert_eq!(LIGHT_FOREGROUND_CSS, "#333333");
        assert_eq!(LIGHT_MUTED_CSS, "#fafafa");
        assert_eq!(LIGHT_MUTED_FOREGROUND_CSS, "#7b7b7b");
        assert_eq!(LIGHT_PRIMARY_CSS, "#168ef7");
        assert_eq!(LIGHT_PRIMARY_FOREGROUND_CSS, "#ffffff");
        assert_eq!(LIGHT_PRIMARY_HOVER_CSS, LIGHT_PRIMARY_CSS);
        assert_eq!(LIGHT_DANGER_CSS, "#fb3748");
        assert_eq!(LIGHT_ACCENT_CSS, "#f0f0f0");
        assert_eq!(LIGHT_ACTIVE_CSS, "#f0f0f0");
        // Opaque equivalents of --hover-bg (#f0f0f00d) and --active-bg (#f0f0f024) over #141414.
        assert_eq!(DARK_ACCENT_CSS, "#1f1f1f");
        assert_eq!(DARK_ACTIVE_CSS, "#333333");
        assert_eq!(LIGHT_BORDER_CSS, "#e7e9e6");
        assert_eq!(LIGHT_MUTED_BORDER_CSS, LIGHT_BORDER_CSS);
        assert_eq!(LIGHT_RING_CSS, "#d0d0d0");
        assert_eq!(DARK_SURFACE_CSS, "#141414");
        assert_eq!(DARK_FOREGROUND_CSS, "#f0f0f0");
        assert_eq!(DARK_MUTED_CSS, "#181818");
        assert_eq!(DARK_MUTED_FOREGROUND_CSS, "#aeaeb2");
        assert_eq!(DARK_PRIMARY_CSS, "#168ef7");
        assert_eq!(DARK_PRIMARY_FOREGROUND_CSS, "#ffffff");
        assert_eq!(DARK_PRIMARY_HOVER_CSS, DARK_PRIMARY_CSS);
        assert_eq!(DARK_DANGER_CSS, LIGHT_DANGER_CSS);
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
