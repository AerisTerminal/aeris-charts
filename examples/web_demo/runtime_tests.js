// Browser runtime gates deliberately consume only the published package API. They do not own chart
// state or rendering logic; their job is to prove that two package configurations produce the same
// externally observable frame.

function wait_for_presentation() {
  return new Promise((resolve) => {
    requestAnimationFrame(() => requestAnimationFrame(resolve));
  });
}

function pixels(canvas) {
  const context = canvas.getContext("2d", { willReadFrequently: true });
  if (context === null) throw new Error("parity gate: screenshot context is unavailable");
  return context.getImageData(0, 0, canvas.width, canvas.height).data;
}

function compare_screenshots(webgpu, canvas2d) {
  if (webgpu.width !== canvas2d.width || webgpu.height !== canvas2d.height) {
    return {
      status: "failed",
      reason: "bitmap dimensions differ",
      webgpu_size: [webgpu.width, webgpu.height],
      canvas2d_size: [canvas2d.width, canvas2d.height],
    };
  }

  const gpu = pixels(webgpu);
  const fallback = pixels(canvas2d);
  let different_pixels = 0;
  let maximum_channel_delta = 0;
  let absolute_channel_delta = 0;

  for (let offset = 0; offset < gpu.length; offset += 4) {
    let pixel_delta = 0;
    for (let channel = 0; channel < 4; channel += 1) {
      const delta = Math.abs(gpu[offset + channel] - fallback[offset + channel]);
      absolute_channel_delta += delta;
      pixel_delta = Math.max(pixel_delta, delta);
      maximum_channel_delta = Math.max(maximum_channel_delta, delta);
    }
    if (pixel_delta !== 0) different_pixels += 1;
  }

  const total_pixels = webgpu.width * webgpu.height;
  const center_offset = (Math.floor(webgpu.height / 2) * webgpu.width + Math.floor(webgpu.width / 2)) * 4;
  return {
    status: different_pixels === 0 ? "passed" : "failed",
    width: webgpu.width,
    height: webgpu.height,
    total_pixels,
    different_pixels,
    different_percent: total_pixels === 0 ? 0 : (different_pixels / total_pixels) * 100,
    maximum_channel_delta,
    mean_absolute_channel_delta: gpu.length === 0 ? 0 : absolute_channel_delta / gpu.length,
    sample_webgpu: {
      corner: Array.from(gpu.slice(0, 4)),
      center: Array.from(gpu.slice(center_offset, center_offset + 4)),
    },
    sample_canvas2d: {
      corner: Array.from(fallback.slice(0, 4)),
      center: Array.from(fallback.slice(center_offset, center_offset + 4)),
    },
  };
}

async function capture_presented_webgpu(container) {
  const canvases = Array.from(container.querySelectorAll(":scope > canvas"));
  if (canvases.length !== 4) {
    throw new Error(`parity gate: expected four package canvases, found ${canvases.length}`);
  }
  const [gpu_pane, , , overlay] = canvases;
  const bitmap = await createImageBitmap(gpu_pane);
  const output = document.createElement("canvas");
  output.width = overlay.width;
  output.height = overlay.height;
  const context = output.getContext("2d");
  if (context === null) throw new Error("parity gate: WebGPU capture context is unavailable");
  context.drawImage(bitmap, 0, 0);
  context.drawImage(overlay, 0, 0);
  bitmap.close();
  return output;
}

/**
 * Compare one deterministic chart through automatic WebGPU and forced Canvas2D. The first chart is
 * supplied by the ordinary demo bootstrap, which ensures this gate exercises the same public path
 * as a consumer application.
 */
export async function run_backend_parity({ create_chart, chart, container, data, fixture, chart_options }) {
  await wait_for_presentation();
  if (chart.backend() !== "webgpu") {
    return {
      status: "skipped",
      reason: "WebGPU was unavailable; the automatic backend already selected Canvas2D",
      backend: chart.backend(),
    };
  }

  // `createImageBitmap` is asynchronous and can read the actually presented WebGPU canvas. The
  // public synchronous screenshot is captured separately and must match the canonical Canvas2D
  // result, while this bitmap tests visual parity of the active GPU backend itself.
  const webgpu_screenshot = await capture_presented_webgpu(container);
  const public_screenshot = chart.take_screenshot();
  chart.remove();

  // The forced-Canvas2D twin must receive the same visual options as the demo-bootstrapped chart;
  // only the backend differs. (Historically this relied on the fixture palette matching engine
  // defaults, which package theming would silently break.)
  const fallback_chart = await create_chart(container, { ...chart_options, autoSize: false, backend: "canvas2d" });
  fallback_chart.resize(fixture.css_width, fixture.css_height, fixture.pixel_ratio);
  const fallback_main = fallback_chart.add_series("candlestick", {
    up_color: "#26a69a",
    down_color: "#ef5350",
    wick_up_color: "#26a69a",
    wick_down_color: "#ef5350",
    border_up_color: "#26a69a",
    border_down_color: "#ef5350",
  });
  fallback_main.set_data(data);
  fallback_chart.add_sma(fallback_main, 20, { color: "#ff9800", line_width: 2, visible: false });
  fallback_chart.time_scale().fit_content();
  await wait_for_presentation();
  const canvas2d_screenshot = fallback_chart.take_screenshot();
  const result = compare_screenshots(webgpu_screenshot, canvas2d_screenshot);
  result.screenshot_api = compare_screenshots(public_screenshot, canvas2d_screenshot);
  const gpu_is_unreadable = result.sample_webgpu?.corner?.every((value) => value === 0)
    && result.sample_webgpu?.center?.every((value) => value === 0);
  result.presented_backend_capture = gpu_is_unreadable ? "unsupported" : result.status;
  if (gpu_is_unreadable) {
    result.reason = "Chromium returned transparent pixels when the presented WebGPU canvas was read in-page";
    result.status = result.screenshot_api.status;
  } else if (result.screenshot_api.status !== "passed") {
    result.status = "failed";
  }
  fallback_chart.remove();

  // Leave the tested frame visible for human inspection without retaining either live chart.
  canvas2d_screenshot.id = "backend_parity_screenshot";
  canvas2d_screenshot.style.width = "100%";
  canvas2d_screenshot.style.height = "100%";
  canvas2d_screenshot.style.display = "block";
  container.appendChild(canvas2d_screenshot);
  return result;
}
