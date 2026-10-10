import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

export const WASM_RUST_OPT_LEVEL = "z";

export function wasm_build_environment(environment = process.env) {
  return {
    ...environment,
    CARGO_PROFILE_RELEASE_OPT_LEVEL: WASM_RUST_OPT_LEVEL,
  };
}

export function build_wasm() {
  const result = spawnSync(
    process.platform === "win32" ? "wasm-pack.exe" : "wasm-pack",
    [
      "build",
      "../../crates/aeris_charts_wasm",
      "--target",
      "web",
      "--out-dir",
      "../../packages/charts/pkg",
    ],
    {
      cwd: fileURLToPath(new URL("..", import.meta.url)),
      env: wasm_build_environment(),
      stdio: "inherit",
    },
  );
  if (result.error) throw result.error;
  process.exitCode = result.status ?? 1;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) build_wasm();
