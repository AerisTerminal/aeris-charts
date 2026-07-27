/**
 * Default style settings — the single place to re-theme `@tradeaion/charts`.
 *
 * Each theme maps the platform's design tokens onto the chart-options tree:
 * - `background` → the chart main background (`layout.background.color`)
 * - `border`     → the price/time axis border color (`*.borderColor` on all three strips)
 * - `grid`       → the grid line color (`grid.vertLines/horzLines.color`) — a DEDICATED token
 *                  (defaults to the border color, but settable independently)
 * - `text`       → the axis text color (`layout.textColor`) — the platform's `--foreground`
 *
 * Tokens mirror the platform stylesheet (`:root` / `.dark`); oklch values are converted to
 * sRGB with the exact CSS Color 4 path (OKLab → XYZ → sRGB). `create_chart` applies the
 * selected theme *under* any explicit options (the engine deep-merges), so a caller can
 * override individual leaves while keeping the rest of the palette. The engine's own
 * built-in defaults stay the reference values; theming is a package-layer concern.
 */

import type { chart_options, deep_partial } from "./types.js";

export interface chart_theme {
  /** Chart main background. */
  background: string;
  /** Price/time axis border color. */
  border: string;
  /** Grid line color (dedicated token — defaults to the border color, independently settable). */
  grid: string;
  /** Axis text color (price/time labels). */
  text: string;
}

/** Light defaults — bg `oklch(1 0 0)` → #ffffff, border #f5f5f5, fg `oklch(0.145 0 0)` → #0a0a0a. */
export const light_theme: chart_theme = {
  background: "#ffffff",
  border: "#f5f5f5",
  grid: "#f5f5f5",
  text: "#0a0a0a",
};

/** Dark defaults — bg `oklch(0.145 0 0)` → #0a0a0a, border #16191f, fg `oklch(0.985 0 0)` → #fafafa. */
export const dark_theme: chart_theme = {
  background: "#0a0a0a",
  border: "#16191f",
  grid: "#16191f",
  text: "#fafafa",
};

export type theme_name = "light" | "dark";

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
    },
    leftPriceScale: { borderColor: palette.border },
    rightPriceScale: { borderColor: palette.border },
    timeScale: { borderColor: palette.border },
    // Dedicated grid token (defaults to the border color but is settable independently). A
    // caller's explicit grid colors still win — the theme merges under them (engine deep-merge).
    grid: {
      vertLines: { color: palette.grid },
      horzLines: { color: palette.grid },
    },
  };
}

