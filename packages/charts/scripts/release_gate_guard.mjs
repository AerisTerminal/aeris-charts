import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { WASM_RUST_OPT_LEVEL, wasm_build_environment } from "./build_wasm.mjs";

const root = fileURLToPath(new URL("../../..", import.meta.url));
const ci = readFileSync(`${root}/.github/workflows/ci.yml`, "utf8");
const publish = readFileSync(`${root}/.github/workflows/publish.yml`, "utf8");
const packageJson = JSON.parse(readFileSync(`${root}/packages/charts/package.json`, "utf8"));

function verify(ciSource, publishSource) {
  assert.match(ciSource, /Run required portable browser suite[\s\S]*npx playwright test/,
    "portable Playwright must remain a required browser step");
  assert.doesNotMatch(ciSource, /Run required portable browser suite[\s\S]{0,180}continue-on-error: true/,
    "portable Playwright cannot continue on error");
  assert.match(ciSource, /AERIS_CHARTS_PERF_STRICT: "1"/,
    "the configured release perf budget must block CI");
  assert.match(ciSource, /Enforce production artifact size budgets[\s\S]{0,180}node benchmarks\/benchmark\.mjs size/,
    "deterministic package and WASM size budgets must block CI");
  assert.doesNotMatch(ciSource, /Enforce production artifact size budgets[\s\S]{0,180}continue-on-error: true/,
    "artifact size budgets cannot continue on error");
  assert.match(ciSource, /machine-sensitive[\s\S]{0,220}continue-on-error: true/,
    "machine-calibrated evidence must remain non-authoritative");
  assert.match(publishSource, /tags: \["v\*"\]/,
    "version tags must trigger publication");
  assert.match(publishSource, /actions: read[\s\S]*contents: read[\s\S]*packages: write/,
    "publication must be able to verify CI, read the release source, and publish packages");
  assert.match(publishSource, /registry-url: https:\/\/npm\.pkg\.github\.com/,
    "the scoped browser package must publish through GitHub Packages");
  assert.match(publishSource, /scope: "@aeristerminal"/,
    "GitHub Packages publication must use the Aeris Terminal scope");
  assert.match(publishSource, /NODE_AUTH_TOKEN: \$\{\{ secrets\.GITHUB_TOKEN \}\}/,
    "GitHub Packages publication must use the workflow token");
  assert.match(publishSource, /Verify source passed CI[\s\S]*actions\/workflows\/ci\.yml\/runs/,
    "publication must require successful CI for the release source");
  assert.match(publishSource, /npm publish --tag latest/,
    "publication must update the latest package tag");
}

verify(ci, publish);
assert.equal(packageJson.scripts["build:wasm"], "node scripts/build_wasm.mjs",
  "production package builds must use the checked-in WASM build profile");
assert.equal(WASM_RUST_OPT_LEVEL, "z",
  "published WASM must retain the measured size-oriented Rust optimization level");
assert.equal(wasm_build_environment({ SENTINEL: "kept" }).SENTINEL, "kept",
  "the WASM build profile must preserve its caller environment");
assert.equal(wasm_build_environment({}).CARGO_PROFILE_RELEASE_OPT_LEVEL, "z",
  "the WASM build profile must override Cargo release optimization without changing native release builds");
for (const [brokenCi, brokenPublish] of [
  [ci.replace("AERIS_CHARTS_PERF_STRICT: \"1\"", "AERIS_CHARTS_PERF_STRICT: \"0\""), publish],
  [ci.replace("node benchmarks/benchmark.mjs size", "node benchmarks/benchmark.mjs test"), publish],
  [ci.replace("id: portable-browser-suite", "id: portable-browser-suite\n        continue-on-error: true"), publish],
  [ci, publish.replace("actions/workflows/ci.yml/runs", "actions/workflows/missing.yml/runs")],
  [ci, publish.replace("https://npm.pkg.github.com", "https://registry.npmjs.org")],
  [ci, publish.replace("npm publish --tag latest", "npm publish")],
]) {
  assert.throws(() => verify(brokenCi, brokenPublish), "a simulated release-gate regression was not detected");
}
console.log("release gate policy and failure simulations OK");
