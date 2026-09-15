/**
 * `npm run test:pack` — publish-readiness smoke test.
 *
 * Packs the package exactly as npm would for publish, installs the tarball into a scratch dir,
 * and asserts the installed artifact is complete and importable:
 *   1. `npm pack` produces a tarball containing JavaScript, types, WebAssembly, the portable
 *      design system, and LICENSE.
 *   2. `npm install <tarball>` into an empty consumer dir.
 *   3. The installed module imports in Node (side-effect-free) and exposes `create_chart`.
 *   4. `dist/nucleuscharts_wasm_bg.wasm` is present inside the installed package (non-trivial size).
 *
 * Node cannot *run* create_chart (browser-only wasm fetch + DOM) — this test deliberately checks
 * only that importing does not throw and the artifact set is complete.
 */
import { execFileSync } from "node:child_process";
import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const pkg_dir = fileURLToPath(new URL("..", import.meta.url));
const scratch = mkdtempSync(join(tmpdir(), "nucleuscharts-pack-smoke-"));

// When invoked from `npm publish --dry-run` (prepublishOnly), the parent leaks its dry-run
// config into our nested npm calls — strip it so the inner `npm pack` really writes a tarball.
const env = { ...process.env };
delete env.npm_config_dry_run;

const run = (cmd, args, cwd) => {
  if (cmd === "npm" && process.env.npm_execpath) {
    return execFileSync(process.execPath, [process.env.npm_execpath, ...args], { cwd, env, encoding: "utf8" }).trim();
  }
  return execFileSync(cmd, args, { cwd, env, encoding: "utf8" }).trim();
};

try {
  // 1. Pack and inspect the tarball file list.
  const tgz = run("npm", ["pack", "--silent"], pkg_dir);
  const files = run("tar", ["-tf", join(pkg_dir, tgz)], pkg_dir).split(/\r?\n/);
  for (const required of [
    "package/dist/index.js",
    "package/dist/index.d.ts",
    "package/dist/nucleuscharts_wasm_bg.wasm",
    "package/dist/nucleuscharts.css",
    "package/LICENSE",
  ]) {
    assert.ok(files.includes(required), `tarball is missing ${required}`);
  }
  assert.ok(
    !files.some((f) => f.startsWith("package/dist/src/")),
    "stale dist/src/*.d.ts duplicates leaked into the tarball (run npm run clean first)",
  );

  // 2. Install the tarball into a scratch consumer.
  writeFileSync(join(scratch, "package.json"), JSON.stringify({ name: "pack-smoke", type: "module" }));
  run("npm", ["install", "--silent", "--no-audit", "--no-fund", join(pkg_dir, tgz)], scratch);

  // 3. Import the installed module (must be side-effect-free) and check the API surface.
  const entry = join(scratch, "node_modules", "@nucleuscharts", "financial", "dist", "index.js");
  const mod = await import(pathToFileURL(entry).href);
  assert.equal(typeof mod.create_chart, "function", "create_chart not exported");
  assert.equal(typeof mod.init_wasm, "function", "init_wasm not exported");

  // 4. The wasm binary shipped with real content.
  const wasm = statSync(
    join(scratch, "node_modules", "@nucleuscharts", "financial", "dist", "nucleuscharts_wasm_bg.wasm"),
  );
  assert.ok(wasm.size > 100_000, `wasm binary suspiciously small (${wasm.size} bytes)`);

  const pkg = JSON.parse(
    readFileSync(join(scratch, "node_modules", "@nucleuscharts", "financial", "package.json"), "utf8"),
  );
  assert.equal(pkg.license, "PolyForm-Noncommercial-1.0.0");

  console.log(`pack smoke OK: ${tgz} (${files.length} files, wasm ${(wasm.size / 1024).toFixed(0)} kB)`);
  rmSync(join(pkg_dir, tgz), { force: true });
} finally {
  rmSync(scratch, { recursive: true, force: true });
}
