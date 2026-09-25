import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../../..", import.meta.url));
const ci = readFileSync(`${root}/.github/workflows/ci.yml`, "utf8");
const publish = readFileSync(`${root}/.github/workflows/publish.yml`, "utf8");

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
  assert.match(publishSource, /actions: read[\s\S]*contents: read/,
    "publication must be able to verify CI and read the release source");
  assert.match(publishSource, /registry-url: https:\/\/registry\.npmjs\.org/,
    "the unscoped browser package must publish through the public npm registry");
  assert.match(publishSource, /NODE_AUTH_TOKEN: \$\{\{ secrets\.NPM_TOKEN \}\}/,
    "npm publication must use the configured npm release token");
  assert.match(publishSource, /Verify source passed CI[\s\S]*actions\/workflows\/ci\.yml\/runs/,
    "publication must require successful CI for the release source");
  assert.match(publishSource, /npm publish --tag latest/,
    "publication must update the latest package tag");
}

verify(ci, publish);
for (const [brokenCi, brokenPublish] of [
  [ci.replace("AERIS_CHARTS_PERF_STRICT: \"1\"", "AERIS_CHARTS_PERF_STRICT: \"0\""), publish],
  [ci.replace("node benchmarks/benchmark.mjs size", "node benchmarks/benchmark.mjs test"), publish],
  [ci.replace("id: portable-browser-suite", "id: portable-browser-suite\n        continue-on-error: true"), publish],
  [ci, publish.replace("actions/workflows/ci.yml/runs", "actions/workflows/missing.yml/runs")],
  [ci, publish.replace("https://registry.npmjs.org", "https://npm.pkg.github.com")],
  [ci, publish.replace("npm publish --tag latest", "npm publish")],
]) {
  assert.throws(() => verify(brokenCi, brokenPublish), "a simulated release-gate regression was not detected");
}
console.log("release gate policy and failure simulations OK");
