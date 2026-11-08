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
        // `--text-secondary` (#0a0a0a94) over white.
        assert_eq!(LIGHT_MUTED_FOREGROUND_CSS, "#717171");
        assert_eq!(LIGHT_PRIMARY_CSS, "#006edd");
        assert_eq!(LIGHT_PRIMARY_FOREGROUND_CSS, "#ffffff");
        assert_eq!(LIGHT_PRIMARY_HOVER_CSS, "#338be4");
        assert_eq!(LIGHT_DANGER_CSS, "#f7525f");
        // `--hover-bg` (#0a0a0a14) and `--active-bg` (#0a0a0a1f) over white.
        assert_eq!(LIGHT_ACCENT_CSS, "#ececec");
        assert_eq!(LIGHT_ACTIVE_CSS, "#e1e1e1");
        // `--hover-bg` (#fafafa1f) and `--active-bg` (#fafafa26) over the dark surface.
        assert_eq!(DARK_ACCENT_CSS, "#1e1e1e");
        assert_eq!(DARK_ACTIVE_CSS, "#252525");
        // `--border` (#0a0a0a33) over white.
        assert_eq!(LIGHT_BORDER_CSS, "#cecece");
        // `--border-secondary` (#0a0a0a33) over white.
        assert_eq!(LIGHT_MUTED_BORDER_CSS, LIGHT_BORDER_CSS);
        assert_eq!(LIGHT_RING_CSS, "#e0e0e0");
        assert_eq!(DARK_SURFACE_CSS, "#000000");
        assert_eq!(DARK_FOREGROUND_CSS, "#c1c1c1");
        // `--surface-secondary` is opaque in the dark theme.
        assert_eq!(DARK_MUTED_CSS, "#171717");
        // The raised tile tone (#fafafa14) over the dark surface.
        assert_eq!(DARK_RAISED_CSS, "#141414");
        // `--text-secondary` (#fafafa8c) over the dark surface.
        assert_eq!(DARK_MUTED_FOREGROUND_CSS, "#898989");
        assert_eq!(DARK_PRIMARY_CSS, "#006edd");
        assert_eq!(DARK_PRIMARY_FOREGROUND_CSS, "#ffffff");
        // `--primary-hover` (primary at 80%) over the dark surface.
        assert_eq!(DARK_PRIMARY_HOVER_CSS, "#0058b1");
        assert_eq!(DARK_DANGER_CSS, LIGHT_DANGER_CSS);
        // `--warning` is the same in both themes.
        assert_eq!(LIGHT_WARNING_CSS, "#ff6900");
        assert_eq!(DARK_WARNING_CSS, LIGHT_WARNING_CSS);
        assert_eq!(DEFAULT_WARNING_RGB, (0xff, 0x69, 0x00));
        // The dark `--border` and `--border-secondary` are opaque.
        assert_eq!(DARK_BORDER_CSS, "#2c2c2c");
        assert_eq!(DARK_MUTED_BORDER_CSS, "#404040");
        // Axis tick text is the primary text token in both themes.
        assert_eq!(LIGHT_AXIS_TEXT_CSS, LIGHT_FOREGROUND_CSS);
        assert_eq!(DARK_AXIS_TEXT_CSS, DARK_FOREGROUND_CSS);
        // The light crosshair keeps its translucent line, alpha kept like the dark line.
        assert_eq!(LIGHT_CROSSHAIR_LINE_CSS, "#0a0a0a26");
        assert_eq!(LIGHT_CROSSHAIR_LINE_RGB, (0x0a, 0x0a, 0x0a));
        assert_eq!(LIGHT_CROSSHAIR_LINE_ALPHA, 0x26);
        assert_eq!(LIGHT_CROSSHAIR_CSS, LIGHT_CROSSHAIR_LINE_CSS);
        assert_eq!(LIGHT_CROSSHAIR_LABEL_CSS, DARK_MUTED_CSS);
        // Translucent so the line stays see-through over series geometry, at the alpha that lands
        // on the dark `--border` tone (#2c2c2c) over the dark surface.
        assert_eq!(DARK_CROSSHAIR_LINE_CSS, "#fafafa2d");
        assert_eq!(DARK_CROSSHAIR_LINE_RGB, (0xfa, 0xfa, 0xfa));
        assert_eq!(DARK_CROSSHAIR_LINE_ALPHA, 0x2d);
        assert_eq!(DARK_CROSSHAIR_CSS, DARK_CROSSHAIR_LINE_CSS);
        assert_eq!(DARK_CROSSHAIR_LABEL_CSS, DARK_MUTED_CSS);
        assert_eq!(DEFAULT_CROSSHAIR_CSS, DARK_CROSSHAIR_LINE_CSS);
        assert_eq!(DEFAULT_CROSSHAIR_ALPHA, DARK_CROSSHAIR_LINE_ALPHA);
        assert_eq!(DARK_SEPARATOR_HOVER_CSS, DARK_ACCENT_CSS);
        // Chart `--bullish` / `--bearish` are their own pair, distinct from platform gain/loss.
        assert_eq!(LIGHT_MARKET_UP_CSS, "#0b9981");
        assert_eq!(LIGHT_MARKET_DOWN_CSS, "#f23645");
        assert_eq!(DARK_MARKET_UP_CSS, LIGHT_MARKET_UP_CSS);
        assert_eq!(DARK_MARKET_DOWN_CSS, LIGHT_MARKET_DOWN_CSS);
        assert_eq!(LIGHT_POSITIVE_CSS, "#089981");
        assert_eq!(LIGHT_NEGATIVE_CSS, "#f7525f");
        assert_eq!(DARK_POSITIVE_CSS, LIGHT_POSITIVE_CSS);
        assert_eq!(DARK_NEGATIVE_CSS, LIGHT_NEGATIVE_CSS);
        assert_eq!(LIGHT_POSITIVE_SUBTLE_CSS, "#dcf5f0");
        assert_eq!(LIGHT_NEGATIVE_SUBTLE_CSS, "#ffe2e2");
        assert_eq!(DARK_POSITIVE_SUBTLE_CSS, "#193c37");
        assert_eq!(DARK_NEGATIVE_SUBTLE_CSS, "#532b2e");
        // `--buy-subtle` / `--sell-subtle`: `--buy` (primary) and `--sell` mixed 12% over the
        // light surface and 18% over the dark surface.
        assert_eq!(LIGHT_BUY_SUBTLE_CSS, "#e0eefb");
        assert_eq!(LIGHT_SELL_SUBTLE_CSS, "#feeaec");
        assert_eq!(DARK_BUY_SUBTLE_CSS, "#001428");
        assert_eq!(DARK_SELL_SUBTLE_CSS, "#2c0f11"); // `--book-bid-fill` / `--book-ask-fill`: primary and #ff2f5e mixed 40% over the light
        // surface and 35% over the dark surface; `--book-fill-text` is drawn on top of a fill.
        assert_eq!(LIGHT_BOOK_BID_CSS, LIGHT_PRIMARY_CSS);
        assert_eq!(DARK_BOOK_BID_CSS, DARK_PRIMARY_CSS);
        assert_eq!(LIGHT_BOOK_ASK_CSS, "#ff2f5e");
        assert_eq!(DARK_BOOK_ASK_CSS, "#ff2f5e");
        assert_eq!(LIGHT_BOOK_FILL_ALPHA, 102);
        assert_eq!(DARK_BOOK_FILL_ALPHA, 89);
        assert_eq!(LIGHT_BOOK_FILL_TEXT_CSS, "#000000");
        assert_eq!(DARK_BOOK_FILL_TEXT_CSS, "#ffffff");
        // Stored option defaults hold the theme-independent pair and the engine repaints it in the
        // active theme's pair, so it must be the default theme's pair.
        assert_eq!(MARKET_UP_CSS, DEFAULT_MARKET_UP_CSS);
        assert_eq!(MARKET_DOWN_CSS, DEFAULT_MARKET_DOWN_CSS);
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

    /// WCAG 2 relative luminance of an opaque sRGB color.
    fn relative_luminance((r, g, b): (u8, u8, u8)) -> f64 {
        let linear = |channel: u8| {
            let srgb = f64::from(channel) / 255.0;
            if srgb <= 0.04045 {
                srgb / 12.92
            } else {
                ((srgb + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b)
    }

    fn contrast_ratio(a: (u8, u8, u8), b: (u8, u8, u8)) -> f64 {
        let (a, b) = (relative_luminance(a), relative_luminance(b));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    /// Every text and focus role a general or financial chart paints on a theme surface keeps its
    /// WCAG 2.2 AA ratio: 4.5:1 for normal-size text (1.4.3) and 3:1 for the focus/selection ring
    /// (1.4.11). Surfaces are the chart pane (`--surface`) and `--surface-secondary`, which is
    /// also the tooltip fill; tooltip secondary text is its own projection over that fill. Text
    /// alone also sits on the hover/accent tone (drawing note placeholders) and the raised footprint
    /// tile. The brand's light `--text-secondary` (#0a0a0a94) measures about 4.1:1 on those two
    /// light tones, an accepted brand decision, so only the dark theme gates secondary text there.
    /// Content fills are covered where labels pick black or white per fill.
    #[test]
    fn theme_text_and_focus_tokens_meet_wcag_aa_on_every_chart_surface() {
        const TEXT: f64 = 4.5;
        const NON_TEXT: f64 = 3.0;
        let themes = [
            (
                "light",
                [LIGHT_SURFACE_RGB, LIGHT_MUTED_RGB],
                [LIGHT_ACCENT_RGB, LIGHT_RAISED_RGB],
                LIGHT_FOREGROUND_RGB,
                LIGHT_MUTED_FOREGROUND_RGB,
                LIGHT_MUTED_RGB,
                LIGHT_POPOVER_MUTED_FOREGROUND_RGB,
                LIGHT_PRIMARY_RGB,
                LIGHT_PRIMARY_FOREGROUND_RGB,
            ),
            (
                "dark",
                [DARK_SURFACE_RGB, DARK_MUTED_RGB],
                [DARK_ACCENT_RGB, DARK_RAISED_RGB],
                DARK_FOREGROUND_RGB,
                DARK_MUTED_FOREGROUND_RGB,
                DARK_MUTED_RGB,
                DARK_POPOVER_MUTED_FOREGROUND_RGB,
                DARK_PRIMARY_RGB,
                DARK_PRIMARY_FOREGROUND_RGB,
            ),
        ];
        for (
            theme,
            surfaces,
            text_tones,
            foreground,
            muted,
            popover,
            popover_muted,
            primary,
            on_primary,
        ) in themes
        {
            let mut pairs = Vec::new();
            for tone in text_tones {
                pairs.push(("default text on a tone", foreground, tone, TEXT));
                if theme == "dark" {
                    pairs.push(("secondary text on a tone", muted, tone, TEXT));
                }
            }
            for surface in surfaces {
                pairs.push((
                    "default text (axes, legend, labels)",
                    foreground,
                    surface,
                    TEXT,
                ));
                pairs.push(("secondary text", muted, surface, TEXT));
                pairs.push(("focus and selection ring", primary, surface, NON_TEXT));
            }
            pairs.push(("tooltip secondary text", popover_muted, popover, TEXT));
            pairs.push(("primary-foreground text", on_primary, primary, TEXT));
            for (role, fg, bg, threshold) in pairs {
                let ratio = contrast_ratio(fg, bg);
                assert!(
                    ratio >= threshold,
                    "{theme} {role}: {fg:?} on {bg:?} is {ratio:.2}:1, below {threshold}:1"
                );
            }
        }
    }

    /// Tooltip tokens match the platform tooltip: the `--surface-secondary` fill (the `muted`
    /// token), a `--border-secondary` edge, `--text-default` text and `--text-secondary`
    /// secondary text, each translucent token composited over that fill.
    #[test]
    fn popover_tokens_project_the_platform_tooltip() {
        assert_eq!(LIGHT_MUTED_CSS, "#fafafa");
        // `--border-secondary` (#0a0a0a33) and `--text-secondary` (#0a0a0a94) over #fafafa.
        assert_eq!(LIGHT_POPOVER_BORDER_CSS, "#cacaca");
        assert_eq!(LIGHT_POPOVER_MUTED_FOREGROUND_CSS, "#6f6f6f");
        assert_eq!(DARK_MUTED_CSS, "#171717");
        // The dark `--border-secondary` is opaque; `--text-secondary` (#fafafa8c) over #171717.
        assert_eq!(DARK_POPOVER_BORDER_CSS, DARK_MUTED_BORDER_CSS);
        assert_eq!(DARK_POPOVER_MUTED_FOREGROUND_CSS, "#949494");
    }
}
