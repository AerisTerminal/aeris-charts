/**
 * Nucleus's public theme mapping.
 *
 * `style_tokens.json` is the only palette source. The Rust engine compiles the same file into
 * its defaults, so WebGPU, Canvas2D, GPUI, and the TypeScript package cannot drift.
 */

import type { chart_options, deep_partial } from "./types.js";
import style_tokens from "./style_tokens.json";

export interface chart_theme {
  /** Chart main background. */
  background: string;
  /** Primary non-interactive text: axes, prices, values, and boxed labels. */
  foreground: string;
  primary: string;
  primary_foreground: string;
  primary_hover: string;
  /** Passive control surface. Crosshair colors have dedicated tokens below. */
  muted: string;
  muted_foreground: string;
  accent: string;
  border: string;
  muted_border: string;
  ring: string;
  crosshair_line: string;
  crosshair_label: string;
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
  crosshair_line: style_tokens.light.crosshair_line,
  crosshair_label: style_tokens.light.crosshair_label,
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
  crosshair_line: style_tokens.dark.crosshair_line,
  crosshair_label: style_tokens.dark.crosshair_label,
};

export type theme_name = "light" | "dark";

export const default_theme_name = style_tokens.default_theme as theme_name;

export function theme_palette(name: theme_name): chart_theme {
  return name === "dark" ? dark_theme : light_theme;
}

/** Map a theme (name or explicit palette) onto the chart-options tree. */
export function theme_options(theme: theme_name | chart_theme): deep_partial<chart_options> {
  const palette = typeof theme === "string" ? theme_palette(theme) : theme;
  return {
    layout: {
      background: { type: "solid", color: palette.background },
      textColor: palette.foreground,
      mutedTextColor: palette.muted_foreground,
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
