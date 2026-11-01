/**
 * @aeristerminal/aeris-charts/complete — the complete build.
 *
 * The same public API as the root entry, bundled against the WebAssembly module compiled with the
 * `complete` Cargo feature. It adds chart families that the standard build omits to stay within its
 * size budget; every family both builds contain behaves identically.
 */
export * from "./index.js";
