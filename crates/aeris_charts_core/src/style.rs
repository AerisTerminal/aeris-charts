//! Canonical Aeris styling tokens shared by every rendering backend.

include!(concat!(env!("OUT_DIR"), "/style_tokens.rs"));

/// The canonical 1 CSS px border width in device pixels with browser border semantics: whole
/// device pixels, rounded down, never thinner than one. At DPR 1 and 1.5 this is one device
/// pixel; at DPR 2, two.
pub fn border_width_device_px(pixel_ratio: f64) -> f64 {
    (BORDER_WIDTH * pixel_ratio).floor().max(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brand_border_snaps_like_a_browser_border() {
        for (ratio, expected) in [(1.0, 1.0), (1.25, 1.0), (1.5, 1.0), (2.0, 2.0), (3.0, 3.0)] {
            assert_eq!(border_width_device_px(ratio), expected, "dpr {ratio}");
        }
    }

    #[test]
    fn canonical_dark_and_market_tokens_are_exact() {
        assert_eq!(DEFAULT_THEME_NAME, "dark");
        assert_eq!(BORDER_WIDTH, 1.0);
        assert_eq!(RADIUS_SMALL, 4.0);
        assert_eq!(RADIUS_COMPACT, 6.0);
        assert_eq!(RADIUS_DEFAULT, 8.0);
        assert_eq!(RADIUS_MEDIUM, 14.0);
        assert_eq!(RADIUS_LARGE, 999.0);
        assert_eq!(LIGHT_SURFACE_CSS, "#ffffff");
        assert_eq!(LIGHT_FOREGROUND_CSS, "#0a0a0a");
        assert_eq!(LIGHT_MUTED_CSS, "#fafafa");
        assert_eq!(LIGHT_RAISED_CSS, "#f0f0f0");
        // `--text-secondary` (#0a0a0a94) over white; passes 4.5:1 on every light surface.
        assert_eq!(LIGHT_MUTED_FOREGROUND_CSS, "#717171");
        assert_eq!(LIGHT_PRIMARY_CSS, "#006edd");
        assert_eq!(LIGHT_PRIMARY_FOREGROUND_CSS, "#ffffff");
        assert_eq!(LIGHT_PRIMARY_HOVER_CSS, "#338be4");
        assert_eq!(LIGHT_DANGER_CSS, "#f7525f");
        // `--hover-bg` (#0a0a0a14) and `--active-bg` (#0a0a0a1f) over white.
        assert_eq!(LIGHT_ACCENT_CSS, "#ececec");
        assert_eq!(LIGHT_ACTIVE_CSS, "#e1e1e1");
        // `--hover-bg` (#fafafa1f) over the dark surface.
        assert_eq!(DARK_ACCENT_CSS, "#1e1e1e");
        assert_eq!(DARK_ACTIVE_CSS, "#252525");
        // `--border` (#0a0a0a33) over white; the popover border resolves to the same token.
        assert_eq!(LIGHT_BORDER_CSS, "#cecece");
        assert_eq!(LIGHT_POPOVER_BORDER_CSS, LIGHT_BORDER_CSS);
        // `--border-secondary` (#0a0a0a33) over white, the same tone as `--border`.
        assert_eq!(LIGHT_MUTED_BORDER_CSS, LIGHT_BORDER_CSS);
        assert_eq!(LIGHT_RING_CSS, "#e0e0e0");
        assert_eq!(DARK_SURFACE_CSS, "#000000");
        assert_eq!(DARK_FOREGROUND_CSS, "#ededed");
        // `--surface-secondary` is opaque in the dark theme.
        assert_eq!(DARK_MUTED_CSS, "#171717");
        // `--surface-raised` (#fafafa14) composited over the dark surface.
        assert_eq!(DARK_RAISED_CSS, "#141414");
        assert_eq!(DARK_MUTED_FOREGROUND_CSS, "#898989");
        assert_eq!(DARK_PRIMARY_CSS, "#006edd");
        assert_eq!(DARK_PRIMARY_FOREGROUND_CSS, "#ffffff");
        assert_eq!(DARK_PRIMARY_HOVER_CSS, "#0058b1");
        assert_eq!(DARK_DANGER_CSS, LIGHT_DANGER_CSS);
        // `--warning` is the same in both themes.
        assert_eq!(LIGHT_WARNING_CSS, "#ff6900");
        assert_eq!(DARK_WARNING_CSS, LIGHT_WARNING_CSS);
        assert_eq!(DEFAULT_WARNING_RGB, (0xff, 0x69, 0x00));
        assert_eq!(DARK_BORDER_CSS, "#2c2c2c");
        // `--border-secondary` is opaque in the dark theme.
        assert_eq!(DARK_MUTED_BORDER_CSS, "#404040");
        // `--border-strong` in the light theme, alpha kept like the dark line.
        assert_eq!(LIGHT_CROSSHAIR_LINE_CSS, "#0a0a0a26");
        assert_eq!(LIGHT_CROSSHAIR_LINE_RGB, (0x0a, 0x0a, 0x0a));
        assert_eq!(LIGHT_CROSSHAIR_LINE_ALPHA, 0x26);
        assert_eq!(LIGHT_CROSSHAIR_CSS, LIGHT_CROSSHAIR_LINE_CSS);
        assert_eq!(LIGHT_CROSSHAIR_LABEL_CSS, DARK_MUTED_CSS);
        // The dark `--border` (#2c2c2c) expressed as a translucent line that lands on exactly that
        // tone over the dark surface while staying see-through over series geometry.
        assert_eq!(DARK_CROSSHAIR_LINE_CSS, "#fafafa2d");
        assert_eq!(DARK_CROSSHAIR_LINE_RGB, (0xfa, 0xfa, 0xfa));
        assert_eq!(DARK_CROSSHAIR_LINE_ALPHA, 0x2d);
        assert_eq!(DARK_CROSSHAIR_CSS, DARK_CROSSHAIR_LINE_CSS);
        assert_eq!(DARK_CROSSHAIR_LABEL_CSS, DARK_MUTED_CSS);
        assert_eq!(DEFAULT_CROSSHAIR_CSS, DARK_CROSSHAIR_LINE_CSS);
        assert_eq!(DEFAULT_CROSSHAIR_ALPHA, DARK_CROSSHAIR_LINE_ALPHA);
        assert_eq!(DARK_SEPARATOR_HOVER_CSS, DARK_ACCENT_CSS);
        assert_eq!(LIGHT_MARKET_UP_CSS, "#089981");
        assert_eq!(LIGHT_MARKET_DOWN_CSS, "#f7525f");
        assert_eq!(DARK_MARKET_UP_CSS, "#089981");
        assert_eq!(DARK_MARKET_DOWN_CSS, "#f7525f");
        assert_eq!(MARKET_UP_CSS, LIGHT_MARKET_UP_CSS);
        assert_eq!(MARKET_DOWN_CSS, LIGHT_MARKET_DOWN_CSS);
        assert_eq!(ORDER_FLOW_BUY_CSS, DARK_PRIMARY_CSS);
        assert_eq!(ORDER_FLOW_BUY_RGB, (0x00, 0x6e, 0xdd));
        assert_eq!(ORDER_FLOW_SELL_CSS, "#d9306a");
        assert_eq!(ORDER_FLOW_SELL_RGB, (0xd9, 0x30, 0x6a));
        assert_eq!(DEFAULT_SURFACE_CSS, DARK_SURFACE_CSS);
        assert_eq!(DEFAULT_BORDER_CSS, DARK_BORDER_CSS);
        assert_eq!(DEFAULT_FOREGROUND_CSS, DARK_FOREGROUND_CSS);
        assert_eq!(DEFAULT_MUTED_CSS, DARK_MUTED_CSS);
        assert_eq!(DEFAULT_SEPARATOR_HOVER_CSS, DARK_SEPARATOR_HOVER_CSS);
    }

    /// Floating-layer tokens: `--surface-popover`, `--border-popover` composited over it,
    /// `--border-popover-highlight` with its CSS alpha, and `--text-secondary` over the popover.
    #[test]
    fn popover_tokens_project_the_floating_layer_contract() {
        assert_eq!(LIGHT_POPOVER_SURFACE_CSS, "#ffffff");
        assert_eq!(LIGHT_POPOVER_BORDER_CSS, "#cecece");
        assert_eq!(LIGHT_POPOVER_HIGHLIGHT_RGB, (0xff, 0xff, 0xff));
        assert_eq!(LIGHT_POPOVER_HIGHLIGHT_ALPHA, 0xcc);
        assert_eq!(
            LIGHT_POPOVER_MUTED_FOREGROUND_CSS,
            LIGHT_MUTED_FOREGROUND_CSS
        );
        assert_eq!(DARK_POPOVER_SURFACE_CSS, "#010101");
        assert_eq!(DARK_POPOVER_BORDER_CSS, "#000000");
        assert_eq!(DARK_POPOVER_HIGHLIGHT_RGB, (0xff, 0xff, 0xff));
        assert_eq!(DARK_POPOVER_HIGHLIGHT_ALPHA, 0x1a);
        assert_eq!(DARK_POPOVER_MUTED_FOREGROUND_CSS, "#8a8a8a");
    }
}
