import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

export const WASM_RUST_OPT_LEVEL = "z";

/**
 * The two published WebAssembly builds. They compile the same crates with the same release profile
 * and wasm-opt flags; the complete build only adds the `complete` Cargo feature, which gates whole
 * chart-family modules. The output name sets the sibling `.wasm` file the generated glue resolves
 * through `import.meta.url`, so each JavaScript entry loads its own module.
 */
export const WASM_BUILDS = Object.freeze([
  Object.freeze({ variant: "standard", out_dir: "pkg", out_name: "aeris_charts_wasm", features: Object.freeze([]) }),
  Object.freeze({ variant: "complete", out_dir: "pkg_complete", out_name: "aeris_charts_wasm_complete", features: Object.freeze(["complete"]) }),
]);

export function wasm_build_environment(environment = process.env) {
  return {
    ...environment,
    CARGO_PROFILE_RELEASE_OPT_LEVEL: WASM_RUST_OPT_LEVEL,
  };
}

export function wasm_pack_arguments({ out_dir, out_name, features }) {
  const args = [
    "build",
    "../../crates/aeris_charts_wasm",
    "--target",
    "web",
    "--out-dir",
    `../../packages/charts/${out_dir}`,
    "--out-name",
    out_name,
  ];
  if (features.length > 0) args.push("--", "--features", features.join(","));
  return args;
}

export function build_wasm() {
  for (const build of WASM_BUILDS) {
    const result = spawnSync(
      process.platform === "win32" ? "wasm-pack.exe" : "wasm-pack",
      wasm_pack_arguments(build),
      {
        cwd: fileURLToPath(new URL("..", import.meta.url)),
        env: wasm_build_environment(),
        stdio: "inherit",
      },
    );
    if (result.error) throw result.error;
    if (result.status !== 0) {
      process.exitCode = result.status ?? 1;
      return;
    }
  }
  process.exitCode = 0;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) build_wasm();
