// @ts-ignore -- pkg is a build artifact, present after build:wasm
import { build_includes_complete_families } from "../pkg/aeris_charts_wasm.js";
import { AerisChartsError } from "./errors.js";

/** Which published build the loaded WebAssembly module is. */
export type chart_build_variant = "standard" | "complete";

/** Package entry that contains every complete-only chart family. */
export const COMPLETE_BUILD_ENTRY = "@aeristerminal/aeris-charts/complete";

/**
 * Chart-family kinds whose engine modules exist only behind the `complete` Cargo feature. A family
 * whose module is gated adds its creation kind here; the creation APIs reject it atomically in the
 * standard build, before any engine mutation.
 */
export const complete_only_families: ReadonlySet<string> = new Set<string>([
  "violin",
  "density_curve",
  "ridgeline",
  "strip_plot",
  "beeswarm",
  "dot_plot",
  "ecdf",
  "qq_plot",
  "hexbin",
  "contour",
  "parallel_coordinates",
  "scatter_matrix",
  "gauge",
  "chord",
  "tree",
  "dendrogram",
  "circle_packing",
  "icicle",
  "network",
  "venn",
  "word_cloud",
]);

let loaded_variant: chart_build_variant | null = null;

/** Record which build the module just initialized; called once WebAssembly instantiation succeeds. */
export function record_loaded_build(): void {
  loaded_variant = build_includes_complete_families() ? "complete" : "standard";
}

/**
 * The build of the initialized WebAssembly engine: `"standard"` for
 * `@aeristerminal/aeris-charts`, `"complete"` for `@aeristerminal/aeris-charts/complete`. The
 * answer comes from the engine itself. Throws `unsupported_operation` before the engine has been
 * initialized by `init_wasm`, `create_chart`, or another chart constructor.
 */
export function build_variant(): chart_build_variant {
  if (loaded_variant === null) {
    throw new AerisChartsError(
      "unsupported_operation",
      "build_variant() requires an initialized engine; await init_wasm() or create a chart first",
    );
  }
  return loaded_variant;
}

/** Camel-case alias of {@link build_variant}. */
export const buildVariant = build_variant;

/** Throw `complete_build_required` when `kind` is a complete-only family and the standard build is loaded. */
export function require_family_in_build(kind: string): void {
  if (!complete_only_families.has(kind) || build_variant() === "complete") return;
  throw new AerisChartsError(
    "complete_build_required",
    `${kind} charts are only in the complete build; import from "${COMPLETE_BUILD_ENTRY}" `
      + `(React: "${COMPLETE_BUILD_ENTRY}/react") instead of "@aeristerminal/aeris-charts"`,
  );
}
