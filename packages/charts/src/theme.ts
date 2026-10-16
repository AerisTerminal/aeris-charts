/**
 * Aeris's public theme mapping.
 *
 * The core crate's `style_tokens.json` is the backend-consumed projection of the complete host design system in
 * `aeris_charts.css`. The Rust engine compiles its opaque colors and shared radii into defaults,
 * so WebGPU, Canvas2D, GPUI, and the TypeScript package cannot drift while host-only CSS stays out
 * of Rust.
 */

import type { chart_options, deep_partial } from "./types.js";
import style_tokens from "../../../crates/aeris_charts_core/style_tokens.json";

export interface chart_theme {
  /** Chart main background. */
  background: string;
  /** Primary non-interactive text: axes, prices, values, and boxed labels. */
  foreground: string;
  primary: string;
  primary_foreground: string;
  /** Opaque projection of the CSS primary-hover mix onto this theme's chart surface. */
  primary_hover: string;
  /** Secondary surface; named themes also use it for crosshair labels. */
  muted: string;
  muted_foreground: string;
  accent: string;
  border: string;
  muted_border: string;
  ring: string;
  /** Crosshair line color. Named themes keep the token's alpha so the line stays translucent. */
  crosshair_line: string;
  /**
   * Crosshair label surface. The dark theme shares the tooltip popover surface; the light theme
   * uses the dark-theme `muted`.
   */
  crosshair_label: string;
  /** Candle, volume, and other up-market geometry. */
  bullish: string;
  /** Candle, volume, and other down-market geometry. */
  bearish: string;
}

/** `#rrggbb` plus an alpha byte as CSS; opaque colors stay in their six-digit form. */
function with_alpha(hex: string, alpha: number): string {
  return alpha === 255 ? hex : `${hex}${alpha.toString(16).padStart(2, "0")}`;
}

export const light_theme: chart_theme = {
  background: style_tokens.light.surface,
  foreground: style_tokens.light.foreground,
  primary: style_tokens.light.primary,
  primary_foreground: style_tokens.light.primary_foreground,
  primary_hover: style_tokens.light.primary_hover,
  muted: style_tokens.light.muted,
  muted_foreground: style_tokens.light.muted_foreground,
  accent: style_tokens.light.accent,
  border: style_tokens.light.border,
  muted_border: style_tokens.light.muted_border,
  ring: style_tokens.light.ring,
  crosshair_line: with_alpha(style_tokens.light.crosshair_line, style_tokens.light.crosshair_line_alpha),
  crosshair_label: style_tokens.dark.muted,
  bullish: style_tokens.light.bullish,
  bearish: style_tokens.light.bearish,
};

export const dark_theme: chart_theme = {
  background: style_tokens.dark.surface,
  foreground: style_tokens.dark.foreground,
  primary: style_tokens.dark.primary,
  primary_foreground: style_tokens.dark.primary_foreground,
  primary_hover: style_tokens.dark.primary_hover,
  muted: style_tokens.dark.muted,
  muted_foreground: style_tokens.dark.muted_foreground,
  accent: style_tokens.dark.accent,
  border: style_tokens.dark.border,
  muted_border: style_tokens.dark.muted_border,
  ring: style_tokens.dark.ring,
  crosshair_line: with_alpha(style_tokens.dark.crosshair_line, style_tokens.dark.crosshair_line_alpha),
  crosshair_label: style_tokens.dark.popover_surface,
  bullish: style_tokens.dark.bullish,
  bearish: style_tokens.dark.bearish,
};

export type theme_name = "light" | "dark";

export const default_theme_name = style_tokens.default_theme as theme_name;

export function theme_palette(name: theme_name): chart_theme {
  return name === "dark" ? dark_theme : light_theme;
}

/**
 * Floating-layer (tooltip) colors: `--surface-popover`, `--border-popover` composited over it,
 * `--border-popover-highlight` with its CSS alpha, and primary/secondary text over the popover.
 */
export interface popover_palette {
  surface: string;
  border: string;
  highlight: string;
  foreground: string;
  muted_foreground: string;
}

function popover_tokens(tokens: typeof style_tokens.light): popover_palette {
  return {
    surface: tokens.popover_surface,
    border: tokens.popover_border,
    highlight: with_alpha(tokens.popover_highlight, tokens.popover_highlight_alpha),
    foreground: tokens.foreground,
    muted_foreground: tokens.popover_muted_foreground,
  };
}

const light_popover = popover_tokens(style_tokens.light);
const dark_popover = popover_tokens(style_tokens.dark);

export function popover_palette(name: theme_name): popover_palette {
  return name === "dark" ? dark_popover : light_popover;
}

/**
 * Map a theme (name or explicit palette) onto the chart-options tree. Named themes retain their
 * package-level identity so `chart.apply_options(theme_options(name))` remains compatible with
 * `reset_style_to_defaults()`. Passing an explicit palette is style-only because it has no
 * canonical light/dark identity to retain.
 */
export function theme_options(theme: theme_name | chart_theme): deep_partial<chart_options> {
  const palette = typeof theme === "string" ? theme_palette(theme) : theme;
  return {
    ...(typeof theme === "string" ? { theme } : {}),
    layout: {
      background: { type: "solid", color: palette.background },
      textColor: palette.foreground,
      mutedTextColor: palette.muted_foreground,
      bullishColor: palette.bullish,
      bearishColor: palette.bearish,
      panes: {
        separatorColor: palette.border,
        separatorHoverColor: palette.accent,
      },
    },
    leftPriceScale: { borderColor: palette.border, textColor: palette.foreground },
    rightPriceScale: { borderColor: palette.border, textColor: palette.foreground },
    timeScale: { borderColor: palette.border },
    grid: {
      vertLines: { color: palette.border },
      horzLines: { color: palette.border },
    },
    crosshair: {
      vertLine: { color: palette.crosshair_line, labelBackgroundColor: palette.crosshair_label },
      horzLine: { color: palette.crosshair_line, labelBackgroundColor: palette.crosshair_label },
    },
  };
}
