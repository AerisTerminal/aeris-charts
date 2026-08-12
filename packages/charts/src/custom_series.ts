/**
 * Custom series (plugin platform Phase C-c) — Nucleus's take on the reference charting library v5's
 * `addCustomSeries`/`ICustomSeriesPaneView` (reference api/chart-api.ts, model/icustom-series.ts).
 *
 * A custom series is a user-defined series TYPE: the engine owns its time mapping (its data
 * rows carry times only, so the merged time scale, logical ranges, and coordinate math work
 * exactly like built-ins) while the plugin owns the item shape and the per-bar drawing. Like
 * the primitives (see `primitives.ts`), drawing never touches a canvas: the plugin's
 * {@link custom_series_pane_view.render} records backend-neutral draw commands through the
 * context below, and the host folds them into the pane's `main` layer **at the series' own
 * paint-order position** — a custom series z-orders between built-in series exactly like a
 * built-in kind, and its output is pixel-identical on the WebGPU and Canvas2D backends.
 *
 * Data flow mirrors reference: `series.set_data(items)` / `series.update(item)` take the raw plugin
 * items (ascending unique times required — out-of-order input is stably sorted and duplicates
 * collapse last-wins, the same repair the built-ins get); `series.data()` returns the aligned
 * raw items. Item `time` values go through the same UTC-seconds conversion as built-in data.
 */

import type { primitive_draw_context } from "./primitives.js";
import type { time } from "./types.js";

/**
 * One custom-series data item (reference `CustomData`): a `time` plus any plugin-defined fields.
 * An optional `color` field colors the series' built-in last-price line and last-value label
 * for that item (the reference's custom barColorer). snake_case per the package API convention.
 */
export type custom_series_item = { time: time } & Record<string, unknown>;

/**
 * The context handed to {@link custom_series_pane_view.render} once per frame. It carries the
 * pane-primitive draw context's fields and command recorders (absolute bitmap px of the whole
 * chart, pane offsets included), minus the horizontal converters — visible items arrive with
 * their bar-center x already resolved, matching how reference hands its renderer `bars` with x
 * coordinates. The context is valid only for the duration of the synchronous `render` call.
 */
export interface custom_series_render_context
  extends Omit<primitive_draw_context, "price_to_y" | "time_to_x" | "logical_to_x"> {
  /**
   * The VISIBLE, non-whitespace items of this series, in ascending time order. `x` is the
   * item's bar-center in absolute bitmap px (the reference's `CustomBarItemData.x`, except absolute and
   * in bitmap rather than pane-media px, like every converter on this context); `item` is the
   * raw data item as given to `set_data`/`update` (the reference's `source data`).
   */
  // oxlint-disable-next-line typescript/no-explicit-any -- plugin-defined item shape (reference `CustomData`)
  readonly items: readonly { x: number; item: any }[];
  /**
   * The time scale's current bar spacing in media px (the reference's `PaneRendererCustomData.barSpacing`
   * verbatim — plugins multiply by `dpr` for bitmap widths, as the reference examples do with
   * `horizontalPixelRatio`).
   */
  readonly bar_spacing: number;
  /**
   * Bitmap y for a price on THIS series' price scale (its pane's left/right scale as bound,
   * or the overlay scale for an overlay series); `null` when the scale has no range (the reference's
   * `PriceToCoordinateConverter`). Percentage/indexed modes anchor on the series' first
   * visible value, like its built-in geometry.
   */
  price_to_y(price: number): number | null;
}

/**
 * A user-defined series type (reference `ICustomSeriesPaneView`, reduced to the command-recording
 * model — the reference's `renderer()`/`update()` pair collapses into one per-frame `render(ctx)`).
 * Plain objects and class instances both work — the package binds the methods it finds.
 *
 * Rendering options the engine does not model (reference style options like the rounded-candles
 * example's `radius`) stay plugin-side (closure state); {@link default_options} covers the
 * engine series options (`visible`, `price_scale_id`, `last_value_visible`, ...). Unsupported
 * style keys in those options are ignored, like every series kind.
 */
export interface custom_series_pane_view {
  /**
   * The item's price values (reference `priceValueBuilder`): used for autoscaling (min/max over the
   * visible items) and for the built-in last-price line / last-value label / `last_value_data`,
   * which track the LAST element of the last non-whitespace item (the Close slot of the reference's
   * `[last, max, min, last]` custom plot-row mapping). Called per visible item per frame —
   * keep it cheap. Non-finite values are dropped; an empty/all-non-finite result skips the item.
   */
  // oxlint-disable-next-line typescript/no-explicit-any -- plugin-defined item shape (reference `CustomData`)
  price_value_builder(item: any): number[];
  /**
   * Whether an item is whitespace (reference `isWhitespace`): whitespace items render nothing at
   * their slot and join no autoscale union, exactly like the built-ins' whitespace rows.
   * Default: the item has only a `time` field (the reference's `{time}`-only whitespace item).
   */
  // oxlint-disable-next-line typescript/no-explicit-any -- plugin-defined item shape (reference `CustomData`)
  is_whitespace?(item: any): boolean;
  /**
   * Engine series options merged UNDER the caller's options at `add_custom_series` time (reference
   * `defaultOptions`, reduced to the engine's option set).
   */
  default_options?: Record<string, unknown>;
  /**
   * Record this series' drawing for the current frame. Synchronous; do not stash `ctx`. Runs
   * while the chart is mid-render: do not call chart/series/scale APIs from inside it (the
   * context is the read path); capture any other inputs from outside.
   */
  render(ctx: custom_series_render_context): void;
  /**
   * Called once when the series is removed from the chart (reference `destroy`): release references,
   * listeners, and timers the view holds.
   */
  destroy?(): void;
}
