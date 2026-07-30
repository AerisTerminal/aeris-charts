import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

if (process.platform !== "win32") {
  console.error("The GPUI/WebGPU pixel matrix requires Windows DWM capture and cannot run on this platform.");
  process.exit(1);
}

const cli = fileURLToPath(new URL("./node_modules/@playwright/test/cli.js", import.meta.url));
const result = spawnSync(
  process.execPath,
  [cli, "test", "tests/gpui-webgpu-matrix.spec.mjs", "--project=chromium"],
  {
    cwd: fileURLToPath(new URL(".", import.meta.url)),
    env: { ...process.env, ORIGIN_RUN_GPUI_WEBGPU_MATRIX: "1" },
    stdio: "inherit",
  },
);
if (result.error) throw result.error;
process.exit(result.status ?? 1);
