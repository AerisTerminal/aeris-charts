/**
 * `npm run test:build-variant` — the standard/complete build contract of the TypeScript package.
 *
 * Bundles the real `src/build_variant.ts` against a stub of the generated WebAssembly glue whose
 * engine marker is switchable, registers a fake complete-only family, and checks that:
 *   1. `build_variant()` refuses to answer before initialization;
 *   2. the standard build rejects the complete-only family with `complete_build_required` naming the
 *      `/complete` entry, while shared families pass;
 *   3. the complete build accepts it;
 *   4. every family creation API runs the check before it touches the engine.
 */
import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { build } from "esbuild";

const root = fileURLToPath(new URL("..", import.meta.url));
const scratch = mkdtempSync(join(tmpdir(), "aeris_charts-build-variant-"));
const fake_family = "__complete_only_fixture";

const stub_glue = {
  name: "stub-glue",
  setup(context) {
    context.onResolve({ filter: /\/pkg\/aeris_charts_wasm\.js$/ }, () => ({ path: "glue", namespace: "stub" }));
    context.onLoad({ filter: /.*/, namespace: "stub" }, () => ({
      contents: "export const build_includes_complete_families = () => globalThis.__aeris_complete_build === true;",
      loader: "js",
    }));
  },
};

try {
  const outfile = join(scratch, "build_variant.mjs");
  await build({
    entryPoints: [join(root, "src", "build_variant.ts")],
    outfile,
    bundle: true,
    format: "esm",
    platform: "neutral",
    plugins: [stub_glue],
    logLevel: "silent",
  });

  for (const variant of ["standard", "complete"]) {
    // A distinct URL gives each variant a fresh module instance with its own recorded build.
    const mod = await import(`${pathToFileURL(outfile).href}?variant=${variant}`);
    assert.throws(() => mod.build_variant(), (error) => error.code === "unsupported_operation",
      "build_variant() must refuse to answer before the engine is initialized");

    globalThis.__aeris_complete_build = variant === "complete";
    mod.record_loaded_build();
    assert.equal(mod.build_variant(), variant);
    assert.equal(mod.buildVariant, mod.build_variant);

    mod.complete_only_families.add(fake_family);
    mod.require_family_in_build("column");
    if (variant === "complete") {
      mod.require_family_in_build(fake_family);
    } else {
      assert.throws(() => mod.require_family_in_build(fake_family), (error) => {
        assert.equal(error.name, "AerisChartsError");
        assert.equal(error.code, "complete_build_required");
        assert.match(error.message, new RegExp(fake_family));
        assert.match(error.message, /"@aeristerminal\/aeris-charts\/complete"/);
        return true;
      });
    }
  }

  const impl = readFileSync(join(root, "src", "impl.ts"), "utf8");
  for (const [name, signature] of [
    ["add_series", /\): series_api \| general_series_api \{\s*require_family_in_build\(kind\);/],
    ["add_layout_series", /add_layout_series\([^)]*\): layout_series_api \{\s*require_family_in_build\(kind\);/],
  ]) {
    assert.match(impl, signature, `${name} must reject complete-only families before any engine mutation`);
  }
  assert.match(impl, /init\(\)\)\.then\(\(module: unknown\) => \{\s*record_loaded_build\(\);/,
    "initialization must record the loaded build from the engine marker");

  console.log("build variant guard OK: standard rejects complete-only families, complete accepts them");
} finally {
  rmSync(scratch, { recursive: true, force: true });
}
