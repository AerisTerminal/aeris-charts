//! Chart option structs, reference-matching defaults, and `apply_options` deep-merge (roadmap Phase A2).
//!
//! the reference charting library exposes deeply-nested option objects and an `applyOptions(partial)` that
//! **deep-merges** a partial patch into the current options (`helpers/merge.ts`): nested objects
//! are merged key-by-key so an update to `grid.vertLines.color` leaves every sibling untouched,
//! while scalars and arrays replace wholesale. We reproduce that contract exactly.
//!
//! Rather than hand-roll a merge per struct, options are held as a canonical `serde_json::Value`
//! seeded from the typed defaults; a patch (also JSON, straight from the JS boundary) is
//! deep-merged into it, and typed views are produced by deserializing on demand. This mirrors
//! the reference's runtime object merge 1:1 and makes partial updates and round-tripping free.
//!
//! Colors are kept as CSS strings (as in reference); the render layer parses them. `LineStyle` is the
//! numeric wire form (0 Solid, 1 Dotted, 2 Dashed). The reference's 3 LargeDashed/4 SparseDotted
//! do not exist in this engine: `Dotted` IS the sparse pattern and `Dashed` the large one, and
//! the retired values fold into them (3 → Dashed, 4 → Dotted).
//!
//! Scope note: this covers the chart-level visual groups (layout, grid, crosshair), the axis
//! strips' border cosmetics (`leftPriceScale`/`rightPriceScale`/`timeScale`), and the top-level
//! flags. Time-scale and price-scale *behavioral* options currently live in their own core
//! structs (`TimeScaleOptions`, `PriceScaleCoreOptions`) and will be folded into this store in a
//! later Phase B pass; per-series options land with the series work.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// `LineStyle`, numeric wire form (0 Solid, 1 Dotted = sparse, 2 Dashed = large). Values 3/4
/// (the reference's LargeDashed/SparseDotted) are retired and fold into 2/1 at the engine edge.
pub mod line_style {
    pub const SOLID: u8 = 0;
    pub const DOTTED: u8 = 1;
    pub const DASHED: u8 = 2;
}

/// reference `CrosshairMode` (`model/crosshair.ts`), numeric wire form.
pub mod crosshair_mode {
    pub const NORMAL: u8 = 0;
    pub const MAGNET: u8 = 1;
    pub const HIDDEN: u8 = 2;
    pub const MAGNET_OHLC: u8 = 3;
}

fn white() -> String {
    "#FFFFFF".into()
}
fn text_color() -> String {
    "#191919".into()
}
fn grid_color() -> String {
    "#D6DCDE".into()
}
fn crosshair_color() -> String {
    "#9598A1".into()
}
fn crosshair_label_bg() -> String {
    "#131722".into()
}
fn axis_border_color() -> String {
    "#2B2B43".into()
}
fn default_font_family() -> String {
    // `helpers/make-font.ts` defaultFontFamily.
    "-apple-system, BlinkMacSystemFont, 'Trebuchet MS', Roboto, Ubuntu, sans-serif".into()
}

/// `layout.background` — solid only for now (reference also has a vertical gradient variant).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BackgroundOptions {
    /// `"solid"` | `"gradient"` (only solid is honored by the renderer today).
    #[serde(rename = "type")]
    pub kind: String,
    pub color: String,
    #[serde(rename = "topColor")]
    pub top_color: String,
    #[serde(rename = "bottomColor")]
    pub bottom_color: String,
}

impl Default for BackgroundOptions {
    fn default() -> Self {
        Self {
            kind: "solid".into(),
            color: white(),
            top_color: white(),
            bottom_color: white(),
        }
    }
}

/// `layout.panes` — stacked-pane chrome (`api/options/layout-options-defaults.ts` v5:
/// `separatorColor`/`separatorHoverColor`; `enableResize` is not modeled).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PanesOptions {
    /// The pane divider color. Empty (the default) follows the price-axis border color, so the
    /// divider always matches the axis chrome and tracks its theme; an explicit value pins it.
    #[serde(rename = "separatorColor")]
    pub separator_color: String,
    /// reference `panes.separatorHoverColor` (default `rgba(178, 181, 189, 0.2)`): the hover band
    /// painted over a separator by the gesture layer.
    #[serde(rename = "separatorHoverColor")]
    pub separator_hover_color: String,
}

impl Default for PanesOptions {
    fn default() -> Self {
        Self {
            separator_color: String::new(),
            separator_hover_color: "rgba(178, 181, 189, 0.2)".into(),
        }
    }
}

/// `layout` — background, text, font (`api/options/layout-options-defaults.ts`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LayoutOptions {
    pub background: BackgroundOptions,
    #[serde(rename = "textColor")]
    pub text_color: String,
    #[serde(rename = "fontSize")]
    pub font_size: f64,
    #[serde(rename = "fontFamily")]
    pub font_family: String,
    #[serde(rename = "attributionLogo")]
    pub attribution_logo: bool,
    pub panes: PanesOptions,
}

impl Default for LayoutOptions {
    fn default() -> Self {
        Self {
            background: BackgroundOptions::default(),
            text_color: text_color(),
            font_size: 12.0,
            font_family: default_font_family(),
            attribution_logo: true,
            panes: PanesOptions::default(),
        }
    }
}

/// A single family of grid lines (`api/options/grid-options-defaults.ts`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GridLineOptions {
    pub color: String,
    /// [`line_style`] value.
    pub style: u8,
    pub visible: bool,
}

impl Default for GridLineOptions {
    fn default() -> Self {
        Self {
            color: grid_color(),
            style: line_style::SOLID,
            // Deliberate divergence from the reference (visible): Aion charts ship grid-free.
            visible: false,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GridOptions {
    #[serde(rename = "vertLines")]
    pub vert_lines: GridLineOptions,
    #[serde(rename = "horzLines")]
    pub horz_lines: GridLineOptions,
}

/// One crosshair line (`api/options/crosshair-options-defaults.ts`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CrosshairLineOptions {
    pub color: String,
    pub width: f64,
    /// [`line_style`] value (default Dotted).
    pub style: u8,
    pub visible: bool,
    #[serde(rename = "labelVisible")]
    pub label_visible: bool,
    #[serde(rename = "labelBackgroundColor")]
    pub label_background_color: String,
}

impl Default for CrosshairLineOptions {
    fn default() -> Self {
        Self {
            color: crosshair_color(),
            width: 1.0,
            // Deliberate divergence from the reference's LargeDashed default: Aion crosshair
            // lines are dotted out of the box.
            style: line_style::DOTTED,
            visible: true,
            label_visible: true,
            label_background_color: crosshair_label_bg(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CrosshairOptions {
    #[serde(rename = "vertLine")]
    pub vert_line: CrosshairLineOptions,
    #[serde(rename = "horzLine")]
    pub horz_line: CrosshairLineOptions,
    /// [`crosshair_mode`] value (default Magnet, matching reference).
    pub mode: u8,
    /// reference `doNotSnapToHiddenSeriesIndices` (default false): when true, the crosshair's snapped
    /// bar index moves to the nearest index that has a bar in any visible series.
    #[serde(rename = "doNotSnapToHiddenSeriesIndices")]
    pub do_not_snap_to_hidden_series_indices: bool,
}

/// Chart-level options of a pane price-axis strip: visibility plus the reference border cosmetics
/// (`price-scale.options.ts`: `borderVisible`/`borderColor`) and the label cosmetics the
/// engine keeps per scale (`alignLabels`/`ticksVisible`/`entireTextOnly`/`minimumWidth`/
/// `textColor` — reference applies these chart-level groups to every pane's scale, pane.ts
/// `applyScaleOptions`). Scale math and per-series scale options are owned by
/// `PriceScaleCore`; `visible` determines whether layout reserves and paints the strip.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PriceAxisOptions {
    pub visible: bool,
    #[serde(rename = "borderVisible")]
    pub border_visible: bool,
    #[serde(rename = "borderColor")]
    pub border_color: String,
    /// reference `alignLabels` (default true).
    #[serde(rename = "alignLabels")]
    pub align_labels: bool,
    /// reference `ticksVisible` (default false).
    #[serde(rename = "ticksVisible")]
    pub ticks_visible: bool,
    /// reference `entireTextOnly` (default false).
    #[serde(rename = "entireTextOnly")]
    pub entire_text_only: bool,
    /// reference `minimumWidth` (default 0).
    #[serde(rename = "minimumWidth")]
    pub minimum_width: f64,
    /// reference `textColor` (default `None` = follow `layout.textColor`).
    #[serde(rename = "textColor")]
    pub text_color: Option<String>,
}

impl PriceAxisOptions {
    fn visible(visible: bool) -> Self {
        Self {
            visible,
            border_visible: true,
            border_color: axis_border_color(),
            // reference defaults (price-scale-options-defaults.ts).
            align_labels: true,
            ticks_visible: false,
            entire_text_only: false,
            minimum_width: 0.0,
            text_color: None,
        }
    }
}

/// Time-axis border cosmetics (`time-scale.options.ts`: `borderVisible`/`borderColor`). The rest
/// of the time-scale surface (bar spacing, offsets) lives in `TimeScaleCore` behind dedicated
/// setters and folds into this store in a later pass.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TimeAxisOptions {
    #[serde(rename = "borderVisible")]
    pub border_visible: bool,
    #[serde(rename = "borderColor")]
    pub border_color: String,
}

impl Default for TimeAxisOptions {
    fn default() -> Self {
        Self {
            border_visible: true,
            border_color: axis_border_color(),
        }
    }
}

impl Default for PriceAxisOptions {
    fn default() -> Self {
        Self::visible(true)
    }
}

impl Default for CrosshairOptions {
    fn default() -> Self {
        Self {
            vert_line: CrosshairLineOptions::default(),
            horz_line: CrosshairLineOptions::default(),
            mode: crosshair_mode::NORMAL, // deliberate divergence from the reference's Magnet default
            do_not_snap_to_hidden_series_indices: false,
        }
    }
}

/// `watermark` — a large text label painted inside the pane (`api/options/watermark`, reference v4
/// shape). Aion draws it on the shared Canvas2D overlay above the series (a deliberate divergence
/// from the reference's behind-series placement: it is the only text path that stays pixel-identical across
/// the WebGPU and Canvas2D pane backends). Colors are CSS strings so alpha is preserved verbatim.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WatermarkOptions {
    pub visible: bool,
    pub text: String,
    /// CSS color (default fully transparent, matching reference — a watermark shows only once colored).
    pub color: String,
    #[serde(rename = "fontSize")]
    pub font_size: f64,
    #[serde(rename = "fontFamily")]
    pub font_family: String,
    #[serde(rename = "fontStyle")]
    pub font_style: String,
    /// `"left" | "center" | "right"`.
    #[serde(rename = "horzAlign")]
    pub horz_align: String,
    /// `"top" | "center" | "bottom"`.
    #[serde(rename = "vertAlign")]
    pub vert_align: String,
}

impl Default for WatermarkOptions {
    fn default() -> Self {
        Self {
            visible: false,
            text: String::new(),
            color: "rgba(0, 0, 0, 0)".into(),
            font_size: 48.0,
            font_family: default_font_family(),
            font_style: String::new(),
            horz_align: "center".into(),
            vert_align: "center".into(),
        }
    }
}

/// Chart-level options (`api/options/chart-options-defaults.ts`, visual subset).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ChartOptions {
    pub layout: LayoutOptions,
    pub grid: GridOptions,
    pub crosshair: CrosshairOptions,
    pub watermark: WatermarkOptions,
    #[serde(rename = "leftPriceScale")]
    pub left_price_scale: PriceAxisOptions,
    #[serde(rename = "rightPriceScale")]
    pub right_price_scale: PriceAxisOptions,
    #[serde(rename = "timeScale")]
    pub time_scale: TimeAxisOptions,
    #[serde(rename = "autoSize")]
    pub auto_size: bool,
    #[serde(rename = "hoveredSeriesOnTop")]
    pub hovered_series_on_top: bool,
}

impl Default for ChartOptions {
    fn default() -> Self {
        Self {
            layout: LayoutOptions::default(),
            grid: GridOptions::default(),
            crosshair: CrosshairOptions::default(),
            watermark: WatermarkOptions::default(),
            left_price_scale: PriceAxisOptions::visible(false),
            right_price_scale: PriceAxisOptions::visible(true),
            time_scale: TimeAxisOptions::default(),
            auto_size: false,
            hovered_series_on_top: true,
        }
    }
}

/// Recursively deep-merge `patch` into `dst`, matching reference `helpers/merge.ts`: when both sides of
/// a key are JSON objects, merge them key-by-key; otherwise `patch` replaces `dst` wholesale
/// (scalars, arrays, and null all overwrite). A `null` in `patch` explicitly sets the key to null.
pub fn deep_merge(dst: &mut Value, patch: &Value) {
    match (dst, patch) {
        (Value::Object(d), Value::Object(p)) => {
            for (k, pv) in p {
                match d.get_mut(k) {
                    Some(dv) if dv.is_object() && pv.is_object() => deep_merge(dv, pv),
                    _ => {
                        d.insert(k.clone(), pv.clone());
                    }
                }
            }
        }
        (d, p) => *d = p.clone(),
    }
}

/// Accumulates chart options across successive `apply_options` calls. Holds the full options as a
/// canonical JSON value so every patch deep-merges into the live state (not back into defaults).
#[derive(Clone, Debug)]
pub struct ChartOptionsStore {
    value: Value,
}

impl Default for ChartOptionsStore {
    fn default() -> Self {
        Self::new()
    }
}

impl ChartOptionsStore {
    /// Start from the reference-matching defaults.
    pub fn new() -> Self {
        Self {
            value: serde_json::to_value(ChartOptions::default()).expect("options serialize"),
        }
    }

    /// Deep-merge a JSON patch object into the current options. A non-object patch is ignored
    /// (options are always an object at the root).
    pub fn apply(&mut self, patch: &Value) {
        if patch.is_object() {
            deep_merge(&mut self.value, patch);
        }
    }

    /// Deep-merge a JSON patch string (as it arrives from the JS boundary). Returns the parse
    /// error for a malformed patch; on error the current options are left unchanged.
    pub fn apply_str(&mut self, patch: &str) -> Result<(), serde_json::Error> {
        let patch: Value = serde_json::from_str(patch)?;
        self.apply(&patch);
        Ok(())
    }

    /// Typed view of the current options.
    pub fn get(&self) -> ChartOptions {
        serde_json::from_value(self.value.clone()).unwrap_or_default()
    }

    /// The raw merged JSON (for round-tripping back to JS via `options()`).
    pub fn value(&self) -> &Value {
        &self.value
    }
}

/// Convenience: build a one-key patch object `{ key: value }` for tests/host glue.
pub fn patch(key: &str, value: Value) -> Value {
    let mut m = Map::new();
    m.insert(key.into(), value);
    Value::Object(m)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn defaults_match_reference() {
        let o = ChartOptions::default();
        assert_eq!(o.layout.background.color, "#FFFFFF");
        assert_eq!(o.layout.text_color, "#191919");
        assert_eq!(o.layout.font_size, 12.0);
        assert_eq!(o.grid.vert_lines.color, "#D6DCDE");
        assert_eq!(o.grid.horz_lines.style, line_style::SOLID);
        assert_eq!(o.crosshair.mode, crosshair_mode::NORMAL);
        assert!(!o.crosshair.do_not_snap_to_hidden_series_indices);
        assert_eq!(o.crosshair.vert_line.style, line_style::DOTTED);
        assert_eq!(o.crosshair.horz_line.label_background_color, "#131722");
        assert!(o.hovered_series_on_top);
        assert!(!o.auto_size);
        // Axis border cosmetics (`#2B2B43` everywhere, all borders visible).
        assert!(o.right_price_scale.visible);
        assert!(!o.left_price_scale.visible);
        assert!(o.right_price_scale.border_visible);
        assert!(o.left_price_scale.border_visible);
        assert_eq!(o.right_price_scale.border_color, "#2B2B43");
        assert_eq!(o.left_price_scale.border_color, "#2B2B43");
        assert!(o.time_scale.border_visible);
        assert_eq!(o.time_scale.border_color, "#2B2B43");
        // Watermark defaults: hidden, transparent, 48px centered (reference v4).
        assert!(!o.watermark.visible);
        assert_eq!(o.watermark.color, "rgba(0, 0, 0, 0)");
        assert_eq!(o.watermark.font_size, 48.0);
        assert_eq!(o.watermark.horz_align, "center");
        assert_eq!(o.watermark.vert_align, "center");
    }

    #[test]
    fn watermark_patch_merges_camelcase_keys() {
        let mut store = ChartOptionsStore::new();
        store.apply(&json!({
            "watermark": { "visible": true, "text": "AION", "fontSize": 64, "horzAlign": "left" },
        }));
        let o = store.get();
        assert!(o.watermark.visible);
        assert_eq!(o.watermark.text, "AION");
        assert_eq!(o.watermark.font_size, 64.0);
        assert_eq!(o.watermark.horz_align, "left");
        // untouched siblings keep their defaults
        assert_eq!(o.watermark.vert_align, "center");
        assert_eq!(o.watermark.color, "rgba(0, 0, 0, 0)");
    }

    #[test]
    fn axis_border_patch_merges_without_touching_siblings() {
        let mut store = ChartOptionsStore::new();
        store.apply(&json!({
            "rightPriceScale": { "borderColor": "#ff0000" },
            "timeScale": { "borderVisible": false },
        }));
        let o = store.get();
        assert_eq!(o.right_price_scale.border_color, "#ff0000");
        // untouched siblings survive: strip visibility and the other border options
        assert!(o.right_price_scale.visible);
        assert!(o.right_price_scale.border_visible);
        assert_eq!(o.left_price_scale.border_color, "#2B2B43");
        assert!(!o.time_scale.border_visible);
        assert_eq!(o.time_scale.border_color, "#2B2B43");
    }

    #[test]
    fn deep_merge_overrides_nested_leaf_only() {
        let mut store = ChartOptionsStore::new();
        store.apply(&json!({ "grid": { "vertLines": { "color": "#000000" } } }));
        let o = store.get();
        // the targeted leaf changed...
        assert_eq!(o.grid.vert_lines.color, "#000000");
        // ...siblings within the same object survived...
        assert_eq!(o.grid.vert_lines.style, line_style::SOLID);
        assert!(!o.grid.vert_lines.visible);
        // ...and the neighbouring family is untouched.
        assert_eq!(o.grid.horz_lines.color, "#D6DCDE");
    }

    #[test]
    fn successive_applies_accumulate() {
        let mut store = ChartOptionsStore::new();
        store.apply(&json!({ "grid": { "vertLines": { "color": "#111111" } } }));
        store.apply(&json!({ "crosshair": { "mode": crosshair_mode::NORMAL } }));
        let o = store.get();
        // first patch persists through the second
        assert_eq!(o.grid.vert_lines.color, "#111111");
        assert_eq!(o.crosshair.mode, crosshair_mode::NORMAL);
        // and unrelated defaults remain
        assert_eq!(o.layout.background.color, "#FFFFFF");
    }

    #[test]
    fn crosshair_do_not_snap_patch_merges_camelcase_key() {
        let mut store = ChartOptionsStore::new();
        store.apply(&json!({ "crosshair": { "doNotSnapToHiddenSeriesIndices": true } }));
        let o = store.get();
        assert!(o.crosshair.do_not_snap_to_hidden_series_indices);
        // untouched siblings keep their defaults
        assert_eq!(o.crosshair.mode, crosshair_mode::NORMAL);
    }

    #[test]
    fn scalar_and_bool_replace() {
        let mut store = ChartOptionsStore::new();
        store.apply(&json!({ "layout": { "fontSize": 16, "attributionLogo": false } }));
        let o = store.get();
        assert_eq!(o.layout.font_size, 16.0);
        assert!(!o.layout.attribution_logo);
    }

    #[test]
    fn apply_str_parses_and_merges() {
        let mut store = ChartOptionsStore::new();
        store
            .apply_str(r##"{ "layout": { "background": { "color": "#0d0d0d" } } }"##)
            .unwrap();
        assert_eq!(store.get().layout.background.color, "#0d0d0d");
    }

    #[test]
    fn apply_str_rejects_malformed_without_mutating() {
        let mut store = ChartOptionsStore::new();
        let before = store.get();
        assert!(store.apply_str("{ not valid json ").is_err());
        assert_eq!(store.get(), before);
    }

    #[test]
    fn unknown_keys_are_ignored_on_read() {
        // A patch with keys we don't model must not break deserialization of the ones we do.
        let mut store = ChartOptionsStore::new();
        store
            .apply(&json!({ "grid": { "vertLines": { "color": "#abcabc" } }, "somethingNew": 42 }));
        assert_eq!(store.get().grid.vert_lines.color, "#abcabc");
    }

    #[test]
    fn deep_merge_null_overwrites() {
        let mut v = json!({ "a": { "b": 1 } });
        deep_merge(&mut v, &json!({ "a": { "b": null } }));
        assert_eq!(v, json!({ "a": { "b": null } }));
    }
}
