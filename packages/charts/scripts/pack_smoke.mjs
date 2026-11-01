/**
 * `npm run test:pack` — publish-readiness smoke test.
 *
 * Packs the package exactly as npm would for publish, installs the tarball into a scratch dir,
 * and asserts the installed artifact is complete and importable for both published builds:
 *   1. `npm pack` produces a tarball containing JavaScript, types, both WebAssembly builds, the
 *      portable design system, LICENSE, and NOTICE.
 *   2. `npm install <tarball>` into an empty consumer dir.
 *   3. The installed standard and complete core modules import in Node (side-effect-free) and expose
 *      both naming styles.
 *   4. Each entry initializes its own sibling `.wasm` through the default `import.meta.url`
 *      resolution, and the engine reports the matching build.
 *   5. The checked-in consumer recipes bundle against each build's entries.
 *
 * Node cannot *run* create_chart (browser-only canvas + DOM) — initialization goes through a
 * file-backed `fetch` so the real default asset resolution and engine marker are exercised.
 */
import { execFileSync } from "node:child_process";
import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { build } from "esbuild";

const pkg_dir = fileURLToPath(new URL("..", import.meta.url));
const examples_dir = fileURLToPath(new URL("../../../examples/all_in_one/", import.meta.url));
const scratch = mkdtempSync(join(tmpdir(), "aeris_charts-pack-smoke-"));

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

const builds = [
  { variant: "standard", entry: "index.js", react: "react.js", wasm: "aeris_charts_wasm_bg.wasm", subpath: "" },
  { variant: "complete", entry: "complete.js", react: "complete_react.js", wasm: "aeris_charts_wasm_complete_bg.wasm", subpath: "/complete" },
];

try {
  // 1. Pack and inspect the tarball file list.
  const tgz = run("npm", ["pack", "--silent"], pkg_dir);
  const files = run("tar", ["-tf", join(pkg_dir, tgz)], pkg_dir).split(/\r?\n/);
  for (const required of [
    ...builds.flatMap(({ entry, react, wasm }) => [
      `package/dist/${entry}`,
      `package/dist/${entry.replace(/\.js$/, ".d.ts")}`,
      `package/dist/${react}`,
      `package/dist/${react.replace(/\.js$/, ".d.ts")}`,
      `package/dist/${wasm}`,
    ]),
    "package/dist/build_variant.d.ts",
    "package/dist/aeris_charts.css",
    "package/LICENSE",
    "package/NOTICE",
  ]) {
    assert.ok(files.includes(required), `tarball is missing ${required}`);
  }
  for (const excluded of ["package/dist/index.js.map", "package/dist/complete.js.map"]) {
    assert.ok(!files.includes(excluded), `tarball ships the large core source map ${excluded}`);
  }
  assert.ok(
    !files.some((f) => f.startsWith("package/dist/src/")),
    "stale dist/src/*.d.ts duplicates leaked into the tarball (run npm run clean first)",
  );

  // 2. Install the tarball into a scratch consumer.
  writeFileSync(join(scratch, "package.json"), JSON.stringify({ name: "pack-smoke", type: "module" }));
  run("npm", ["install", "--silent", "--no-audit", "--no-fund", join(pkg_dir, tgz)], scratch);
  const installed_package = join(scratch, "node_modules", "@aeristerminal", "aeris-charts");

  // Each entry's generated glue resolves its sibling `.wasm` by `import.meta.url`; serve those file
  // URLs so initialization runs the published default resolution path.
  const fetched = [];
  globalThis.fetch = async (input) => {
    const url = new URL(typeof input === "string" ? input : input.url ?? String(input));
    assert.equal(url.protocol, "file:", `unexpected wasm fetch ${url.href}`);
    fetched.push(fileURLToPath(url));
    return new Response(readFileSync(url), { headers: { "content-type": "application/wasm" } });
  };

  const pkg = JSON.parse(readFileSync(join(installed_package, "package.json"), "utf8"));
  assert.equal(pkg.license, "AGPL-3.0-only");
  assert.equal(pkg.exports["./wasm"], "./dist/aeris_charts_wasm_bg.wasm");
  assert.equal(pkg.exports["./complete/wasm"], "./dist/aeris_charts_wasm_complete_bg.wasm");

  for (const { variant, entry, react, wasm, subpath } of builds) {
    assert.equal(pkg.exports[subpath === "" ? "." : `.${subpath}`].import, `./dist/${entry}`);
    assert.equal(pkg.exports[`.${subpath}/react`].import, `./dist/${react}`);
    assert.equal(pkg.exports[`.${subpath}/react`].types, `./dist/${react.replace(/\.js$/, ".d.ts")}`);

    // 3. Import the installed module (must be side-effect-free) and check the API surface.
    const entry_path = join(installed_package, "dist", entry);
    const mod = await import(pathToFileURL(entry_path).href);
    for (const name of ["create_chart", "createChart", "init_wasm", "initWasm", "build_variant", "buildVariant"]) {
      assert.equal(typeof mod[name], "function", `${variant}: ${name} not exported`);
    }
    assert.throws(() => mod.build_variant(), (error) => error.code === "unsupported_operation",
      `${variant}: build_variant() must refuse to answer before initialization`);

    const installed_js = readFileSync(entry_path, "utf8");
    for (const forbidden of ["../pkg/", "../pkg_complete/", "../../crates/", "examples/web_demo", "benchmarks/"]) {
      assert.ok(!installed_js.includes(forbidden), `${variant}: published runtime leaked repository-only path: ${forbidden}`);
    }
    const other_wasm = builds.find((other) => other.variant !== variant).wasm;
    assert.ok(installed_js.includes(wasm) && !installed_js.includes(other_wasm),
      `${variant}: core module must resolve only ${wasm}`);
    const react_js = readFileSync(join(installed_package, "dist", react), "utf8");
    const other_entry = builds.find((other) => other.variant !== variant).entry;
    assert.ok(react_js.includes(`./${entry}`) && !react_js.includes(`./${other_entry}`),
      `${variant}: React adapter must drive ./${entry}`);

    // 4. Initialize from the sibling asset and ask the engine which build it is.
    await mod.init_wasm();
    assert.deepEqual(fetched.splice(0), [join(installed_package, "dist", wasm)], `${variant}: wrong wasm fetched`);
    assert.equal(mod.build_variant(), variant, `${variant}: engine reports the wrong build`);

    // The wasm binary shipped with real content and the NOTICE section 7(b) legal notices survive
    // release optimization: the runtime console notice and the shader provenance notice.
    const wasm_path = join(installed_package, "dist", wasm);
    assert.ok(statSync(wasm_path).size > 100_000, `${variant}: wasm binary suspiciously small`);
    const wasm_bytes = readFileSync(wasm_path);
    for (const marker of ["Powered by Aeris Charts (AGPL-3.0-only)", "// aeris_charts provenance:"]) {
      assert.ok(wasm_bytes.includes(marker), `${variant}: release wasm is missing license marker: ${marker}`);
    }
  }

  assert.equal(
    readFileSync(join(installed_package, "NOTICE"), "utf8").replaceAll("\r\n", "\n"),
    readFileSync(join(pkg_dir, "..", "..", "NOTICE"), "utf8").replaceAll("\r\n", "\n"),
    "packaged NOTICE differs from the repository NOTICE",
  );

  // 5. Compile the checked-in Cartesian, polar, flow, mixed, linked, and React recipes from the
  // installed tarball, once per build, so package exports and consumer resolution are exercised
  // without repository paths.
  const examples = ["standalone.mjs", "polar.mjs", "flow.mjs", "vanilla.mjs", "linked.mjs", "react.tsx"];
  const bundles = [];
  for (const { variant, wasm, subpath } of builds) {
    for (const name of examples) {
      const source = readFileSync(join(examples_dir, name), "utf8")
        .replace(/"@aeristerminal\/aeris-charts(\/react)?"/g, `"@aeristerminal/aeris-charts${subpath}$1"`);
      const copy = join(scratch, `${variant}_${name}`);
      writeFileSync(copy, source);
      bundles.push({ variant, wasm, name, copy });
    }
  }
  await build({
    entryPoints: bundles.map(({ copy }) => copy),
    outdir: join(scratch, "bundled"),
    bundle: true,
    format: "esm",
    platform: "browser",
    jsx: "automatic",
    external: ["react", "react/*"],
    logLevel: "silent",
  });
  for (const { variant, wasm, name } of bundles) {
    const output = join(scratch, "bundled", `${variant}_${name}`.replace(/\.(mjs|tsx)$/, ".js"));
    assert.ok(statSync(output).size > 10_000, `packed consumer bundle is incomplete: ${variant} ${name}`);
    const other_wasm = builds.find((other) => other.variant !== variant).wasm;
    const bundled = readFileSync(output, "utf8");
    assert.ok(bundled.includes(wasm) && !bundled.includes(other_wasm),
      `packed consumer bundle ${variant} ${name} must load only ${wasm}`);
  }

  const sizes = builds.map(({ variant, wasm }) =>
    `${variant} wasm ${(statSync(join(installed_package, "dist", wasm)).size / 1024).toFixed(0)} kB`).join(", ");
  console.log(`pack smoke OK: ${tgz} (${files.length} files; ${sizes}; ${bundles.length} consumer bundles)`);
  rmSync(join(pkg_dir, tgz), { force: true });
} finally {
  rmSync(scratch, { recursive: true, force: true });
}
