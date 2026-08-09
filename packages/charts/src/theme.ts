/**
 * Origin's public theme mapping.
 *
 * `style_tokens.json` is the only palette source. The Rust engine compiles the same file into
 * its defaults, so WebGPU, Canvas2D, GPUI, and the TypeScript package cannot drift.
 */

import type { chart_options, deep_partial } from "./types.js";
import style_tokens from "./style_tokens.json";

export interface chart_theme {
  /** Chart main background. */
  background: string;
  /** Price/time axis border color. */
  border: string;
  /** Grid line color. */
  grid: string;
  /** Axis text color (price/time labels). */
  text: string;
  /** Crosshair lines and label background. */
  crosshair: string;
  /** Pane separator hover band. Defaults to the crosshair color for custom palettes. */
  separator_hover?: string;
}

export const light_theme: chart_theme = {
  background: style_tokens.light.surface,
  border: style_tokens.light.border,
  grid: style_tokens.light.border,
  text: style_tokens.light.axis_text,
  crosshair: style_tokens.light.crosshair,
  separator_hover: style_tokens.light.separator_hover,
};

export const dark_theme: chart_theme = {
  background: style_tokens.dark.surface,
  border: style_tokens.dark.border,
  grid: style_tokens.dark.border,
  text: style_tokens.dark.axis_text,
  crosshair: style_tokens.dark.crosshair,
  separator_hover: style_tokens.dark.separator_hover,
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
      textColor: palette.text,
      panes: {
        separatorColor: palette.border,
        separatorHoverColor: palette.separator_hover ?? palette.crosshair,
      },
    },
    leftPriceScale: { borderColor: palette.border },
    rightPriceScale: { borderColor: palette.border },
    timeScale: { borderColor: palette.border },
    grid: {
      vertLines: { color: palette.grid },
      horzLines: { color: palette.grid },
    },
    crosshair: {
      vertLine: { color: palette.crosshair, labelBackgroundColor: palette.crosshair },
      horzLine: { color: palette.crosshair, labelBackgroundColor: palette.crosshair },
    },
  };
}
