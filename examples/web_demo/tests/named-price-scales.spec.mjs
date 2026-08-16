import { test, expect } from "@playwright/test";

async function wait_for_chart(page) {
  await page.waitForFunction(() => window.__chart?.backend?.() !== undefined);
  await page.evaluate(() => new Promise((resolve) => {
    requestAnimationFrame(() => requestAnimationFrame(resolve));
  }));
}

for (const backend of ["canvas2d", "webgpu"]) {
  test(`${backend}: named scales own placement, formatting, and comparison normalization`, async ({ page }) => {
    await page.goto(`/?backend=${backend}&forceFallbackAdapter=1`);
    await wait_for_chart(page);

    const result = await page.evaluate(async () => {
      const chart = window.__chart;
      const rows = window.__data.slice(0, 3);
      const points = (values) => rows.map((row, index) => ({ time: row.time, value: values[index] }));

      const inner = chart.add_price_scale({ id: "comparison-inner", side: "right", order: 0, minimum_width: 70 });
      const outer = chart.add_price_scale({ id: "comparison-outer", side: "right", minimum_width: 76 });
      const left = chart.add_price_scale({ id: "comparison-left", side: "left", order: 0, minimum_width: 72 });
      const percentage = chart.add_price_scale({ id: "shared-percentage", side: "left", order: 0, mode: 2 });

      const inner_series = chart.add_series("line", { price_scale_id: "comparison-inner" });
      const outer_series = chart.add_series("area", { price_scale_id: "comparison-outer" });
      const left_series = chart.add_series("histogram", { price_scale_id: "comparison-left" });
      const pct_a = chart.add_series("line", { price_scale_id: "shared-percentage" });
      const pct_b = chart.add_series("line", { price_scale_id: "shared-percentage" });
      inner_series.set_data(points([1_000, 1_010, 1_020]));
      outer_series.set_data(points([10_000, 10_100, 10_200]));
      left_series.set_data(points([10, 11, 12]));
      pct_a.set_data(points([100, 105, 110]));
      pct_b.set_data(points([200, 210, 220]));
      chart.time_scale().fit_content();
      await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));

      const scales = chart.price_scales();
      const info = Object.fromEntries(scales.map((scale) => [scale.id, scale]));
      const pane_width = chart.wasm.time_scale_width();
      const inner_target = chart.wasm.price_scale_target_by_id(0, "comparison-inner");
      const outer_target = chart.wasm.price_scale_target_by_id(0, "comparison-outer");
      const right_target = chart.wasm.price_scale_target_by_id(0, "right");
      const inner_width = inner.width();
      const right_width = chart.price_scale("right").width();
      const outer_width = outer.width();
      const hit_inner = chart.wasm.price_axis_target_at(0, pane_width + inner_width / 2);
      const hit_outer = chart.wasm.price_axis_target_at(
        0,
        pane_width + inner_width + right_width + outer_width / 2,
      );
      const pct_y_a = pct_a.price_to_coordinate(110);
      const pct_y_b = pct_b.price_to_coordinate(220);

      inner.apply_options({ visible: false });
      await new Promise((resolve) => requestAnimationFrame(resolve));

      return {
        backend: chart.backend(),
        ids: [inner_series.price_scale_id(), outer_series.price_scale_id(), left_series.price_scale_id()],
        right_order: [
          [info["comparison-inner"].id, info["comparison-inner"].order],
          [info.right.id, info.right.order],
          [info["comparison-outer"].id, info["comparison-outer"].order],
        ],
        left_side: info["comparison-left"].side,
        widths: [inner_width, outer_width, left.width()],
        targets: { inner_target, outer_target, right_target, hit_inner, hit_outer },
        percentage_delta: Math.abs(pct_y_a - pct_y_b),
        hidden_width: inner.width(),
        hidden_state: inner.options().visible,
      };
    });

    expect(result.backend).toBe(backend);
    expect(result.ids).toEqual(["comparison-inner", "comparison-outer", "comparison-left"]);
    expect(result.right_order).toEqual([
      ["comparison-inner", 0],
      ["right", 1],
      ["comparison-outer", 2],
    ]);
    expect(result.left_side).toBe("left");
    expect(result.widths.every((width) => width >= 70)).toBe(true);
    expect(result.targets.hit_inner).toBe(result.targets.inner_target);
    expect(result.targets.hit_outer).toBe(result.targets.outer_target);
    expect(result.targets.inner_target).not.toBe(result.targets.right_target);
    expect(result.percentage_delta).toBeLessThan(1e-7);
    expect(result.hidden_width).toBe(0);
    expect(result.hidden_state).toBe(false);
  });
}

test("named scale descriptors support atomic pane moves and host-owned reconstruction", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  await wait_for_chart(page);

  const result = await page.evaluate(async () => {
    const chart = window.__chart;
    const rows = window.__data.slice(0, 3);
    const series = chart.add_series("line", { price_scale_id: "right" });
    series.set_data(rows.map((row, index) => ({ time: row.time, value: 500 + index * 10 })));
    const handle = chart.add_price_scale({
      id: "host-owned",
      side: "right",
      order: 0,
      minimum_width: 88,
      invert_scale: true,
    });
    series.move_to_price_scale("host-owned");
    const descriptor = chart.price_scales().find((scale) => scale.id === "host-owned");
    const options = handle.options();

    const pane = chart.add_pane(true);
    let failed_move_code = null;
    try {
      series.move_to_pane(pane.pane_index());
    } catch (error) {
      failed_move_code = error.code;
    }
    const after_failed_move = { pane: series.pane_index(), scale: series.price_scale_id() };

    chart.add_price_scale({ id: descriptor.id, side: descriptor.side, order: descriptor.order, ...options }, pane.pane_index());
    series.move_to_pane(pane.pane_index());
    const after_successful_move = { pane: series.pane_index(), scale: series.price_scale_id() };

    series.move_to_pane(0);
    series.move_to_price_scale("right");
    chart.remove_price_scale("host-owned", 0);
    let stale_code = null;
    try {
      handle.options();
    } catch (error) {
      stale_code = error.code;
    }
    const restored = chart.add_price_scale({
      id: descriptor.id,
      side: descriptor.side,
      order: descriptor.order,
      ...options,
    });
    series.move_to_price_scale("host-owned");
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));

    return {
      descriptor,
      failed_move_code,
      after_failed_move,
      after_successful_move,
      stale_code,
      restored_options: restored.options(),
      final_binding: { pane: series.pane_index(), scale: series.price_scale_id() },
    };
  });

  expect(result.descriptor).toMatchObject({
    id: "host-owned",
    side: "right",
    order: 0,
    visible: true,
    built_in: false,
    pane_index: 0,
  });
  expect(result.failed_move_code).toBe("invalid_options");
  expect(result.after_failed_move).toEqual({ pane: 0, scale: "host-owned" });
  expect(result.after_successful_move).toEqual({ pane: 1, scale: "host-owned" });
  expect(result.stale_code).toBe("stale_handle");
  expect(result.restored_options).toMatchObject({ minimum_width: 88, invert_scale: true, visible: true });
  expect(result.final_binding).toEqual({ pane: 0, scale: "host-owned" });
});
