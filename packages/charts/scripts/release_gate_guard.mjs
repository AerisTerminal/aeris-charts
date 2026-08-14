import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../../..", import.meta.url));
const ci = readFileSync(`${root}/.github/workflows/ci.yml`, "utf8");

function verify(source) {
  assert.match(source, /publish:[\s\S]*needs: \[rust, package, browser\]/,
    "tag publication must depend on Rust, package, and portable browser jobs");
  assert.match(source, /Run required portable browser suite[\s\S]*npx playwright test/,
    "portable Playwright must remain a required browser step");
  assert.doesNotMatch(source, /Run required portable browser suite[\s\S]{0,180}continue-on-error: true/,
    "portable Playwright cannot continue on error");
  assert.match(source, /NUCLEUSCHARTS_PERF_STRICT: "1"/,
    "the configured release perf budget must block CI");
  assert.match(source, /machine-sensitive[\s\S]{0,220}continue-on-error: true/,
    "machine-calibrated evidence must remain non-authoritative");
}

verify(ci);
for (const broken of [
  ci.replace("needs: [rust, package, browser]", "needs: [rust, package]"),
  ci.replace("NUCLEUSCHARTS_PERF_STRICT: \"1\"", "NUCLEUSCHARTS_PERF_STRICT: \"0\""),
  ci.replace("id: portable-browser-suite", "id: portable-browser-suite\n        continue-on-error: true"),
]) {
  assert.throws(() => verify(broken), "a simulated release-gate regression was not detected");
}
console.log("release gate policy and failure simulations OK");
