import { test, expect } from "@playwright/test";
import { PNG } from "pngjs";

async function open_trading_demo(page, backend = "canvas2d") {
  await page.goto(`/?feature=trading&backend=${backend}`);
  await page.waitForFunction(() => window.__feature_lab?.active_ids().includes("trading-bracket"));
  await page.evaluate(() => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  await page.evaluate(() => {
    // The close control is the trailing cell of the marker's one container. Sweep the marker span
    // for it instead of hardcoding the cell widths, and answer with the cell's center.
    window.__close_x = (id, y) => {
      const trading = window.__chart.trading();
      const width = Math.round(window.__chart.time_scale().width());
      let first = null;
      let last = null;
      for (let x = width; x > width - 320; x -= 1) {
        const hit = trading.hit_at(x, y);
        if (hit?.id === id && hit.kind === "cancel_button") {
          if (last === null) last = x;
          first = x;
        } else if (last !== null) {
          break;
        }
      }
      return first === null ? null : (first + last) / 2;
    };
  });
}

function count_near(image, expected, tolerance = 10) {
  let count = 0;
  for (let offset = 0; offset < image.data.length; offset += 4) {
    if (
      Math.abs(image.data[offset] - expected[0]) <= tolerance
      && Math.abs(image.data[offset + 1] - expected[1]) <= tolerance
      && Math.abs(image.data[offset + 2] - expected[2]) <= tolerance
      && image.data[offset + 3] > 200
    ) count += 1;
  }
  return count;
}

test("first-party trading snapshot preserves identity and broker relationships", async ({ page }) => {
  await open_trading_demo(page);
  const state = await page.evaluate(() => window.__chart.trading().state());
  expect(state.positions).toEqual([expect.objectContaining({
    id: "demo-position",
    side: "long",
    quantity: 12,
  })]);
  expect(state.orders).toHaveLength(3);
  expect(state.orders.find((order) => order.id === "demo-target")).toMatchObject({
    role: "take_profit",
    position_id: "demo-position",
    bracket_id: "demo-bracket",
    oco_group_id: "demo-oco",
  });
  expect(state.orders.find((order) => order.id === "demo-stop")).toMatchObject({
    role: "stop_loss",
    oco_group_id: "demo-oco",
  });
  expect(state.orders.find((order) => order.id === "demo-partial")).toMatchObject({
    status: "partially_filled",
    quantity: 12,
    filled_quantity: 5,
  });
  expect(state.executions).toEqual([expect.objectContaining({
    id: "demo-fill",
    kind: "partial_fill",
    order_id: "demo-partial",
  })]);

  const invalid = await page.evaluate(() => {
    const trading = window.__chart.trading();
    const before = trading.state();
    try {
      trading.apply_snapshot({
        positions: [
          { id: "duplicate", side: "long", average_price: 100, quantity: 1 },
          { id: "duplicate", side: "short", average_price: 101, quantity: 1 },
        ],
      });
      return { threw: false };
    } catch (error) {
      return { threw: true, code: error.code, unchanged: JSON.stringify(trading.state()) === JSON.stringify(before) };
    }
  });
  expect(invalid).toEqual({ threw: true, code: "invalid_data", unchanged: true });
});

test("trading lines use dedicated hits and render semantic colors through the shared frame", async ({ page }) => {
  await open_trading_demo(page);
  const probe = await page.evaluate(() => {
    const trading = window.__chart.trading();
    const target = trading.state().orders.find((order) => order.id === "demo-target");
    const position = trading.state().positions[0];
    const position_y = window.__main.price_to_coordinate(position.average_price);
    const width = window.__chart.time_scale().width();
    const exact_axis_controls = [
      { id: position.id, price: position.average_price },
      ...trading.state().orders.map((order) => ({ id: order.id, price: order.price })),
    ].map(({ id, price }) => {
      const y = window.__main.price_to_coordinate(price);
      return { id, y, hit: trading.hit_at(window.__close_x(id, y), y) };
    });
    return {
      empty_left: trading.hit_at(40, window.__main.price_to_coordinate(target.price)),
      order: trading.hit_at(width - 200, window.__main.price_to_coordinate(target.price)),
      position: trading.hit_at(width - 200, position_y),
      close: trading.hit_at(window.__close_x(position.id, position_y), position_y),
      exact_axis_controls,
    };
  });
  expect(probe.empty_left).toBeNull();
  expect(probe.order).toMatchObject({
    object_type: "order",
    id: "demo-target",
    kind: "order_line",
  });
  expect(probe.position).toMatchObject({ object_type: "position", kind: "position_line" });
  expect(probe.close).toMatchObject({ object_type: "position", kind: "cancel_button" });
  for (const control of probe.exact_axis_controls) {
    expect(control.hit, `${control.id} close control must remain on its exact price coordinate`).toMatchObject({
      id: control.id,
      kind: "cancel_button",
    });
  }

  const url = await page.evaluate(() => window.__chart.take_screenshot().toDataURL("image/png"));
  const image = PNG.sync.read(Buffer.from(url.split(",")[1], "base64"));
  expect(count_near(image, [247, 82, 95]), "sell order line/label pixels").toBeGreaterThan(100);
  expect(count_near(image, [8, 153, 129]), "long position and buy order pixels").toBeGreaterThan(100);
});

test("trading state is chart-local and clear removes all live objects", async ({ page }) => {
  await open_trading_demo(page);
  await page.evaluate(() => window.__feature_lab.clear());
  expect(await page.evaluate(() => window.__chart.trading().state())).toEqual({
    instrument: {},
    positions: [],
    orders: [],
    executions: [],
  });
});

test("pointer drag has trading priority and emits one broker-neutral modify intent", async ({ page }) => {
  await open_trading_demo(page);
  const probe = await page.evaluate(() => {
    window.__trading_intents = [];
    window.__chart.trading().subscribe_intents((intent) => window.__trading_intents.push(intent));
    const order = window.__chart.trading().state().orders.find((item) => item.id === "demo-target");
    window.__trading_blocker = window.__chart.add_drawing(
      "horizontal_line",
      [{ logical: 620, price: order.price }],
      { color: "#a459d1" },
    );
    const overlay = document.querySelector("#chart_container canvas:last-of-type").getBoundingClientRect();
    const before_range = window.__chart.time_scale().get_visible_logical_range();
    return {
      from: { x: overlay.left + window.__chart.time_scale().width() - 200, y: overlay.top + window.__main.price_to_coordinate(order.price) },
      to: { x: overlay.left + window.__chart.time_scale().width() - 200, y: overlay.top + window.__main.price_to_coordinate(order.price + 1.25) },
      before_range,
      confirmed_price: order.price,
      drawing_points: window.__trading_blocker.points(),
    };
  });
  await page.mouse.move(probe.from.x, probe.from.y);
  await page.mouse.down();
  await page.mouse.move(probe.to.x, probe.to.y, { steps: 6 });
  await page.mouse.up();

  const result = await page.evaluate(() => ({
    intents: window.__trading_intents,
    preview: window.__chart.trading().preview(),
    confirmed: window.__chart.trading().state().orders.find((item) => item.id === "demo-target"),
    range: window.__chart.time_scale().get_visible_logical_range(),
    drawing_points: window.__trading_blocker.points(),
  }));
  expect(result.intents).toHaveLength(1);
  expect(result.intents[0]).toMatchObject({
    action: "modify_order",
    order_id: "demo-target",
    role: "take_profit",
    base_revision: 0,
  });
  expect(result.preview).toMatchObject({ source: "order", order_id: "demo-target", phase: "pending" });
  expect(result.preview.price).not.toBe(probe.confirmed_price);
  expect(result.confirmed.price).toBe(probe.confirmed_price);
  expect(result.range).toEqual(probe.before_range);
  expect(result.drawing_points).toEqual(probe.drawing_points);

  expect(await page.evaluate(() => {
    const sequence = window.__trading_intents[0].sequence;
    return window.__chart.trading().resolve_intent(sequence, false);
  })).toBe(true);
  expect(await page.evaluate(() => window.__chart.trading().preview())).toBeNull();
});

test("unlinked protection orders remain draggable and preserve stop-limit modify fields", async ({ page }) => {
  await open_trading_demo(page);
  const probe = await page.evaluate(() => {
    const trading = window.__chart.trading();
    trading.apply_snapshot({
      instrument: { tick_size: 0.25, price_precision: 2 },
      orders: [{
        id: "orphan-stop",
        side: "sell",
        kind: "stop_limit",
        role: "stop_loss",
        status: "working",
        price: 100,
        stop_price: 99.5,
        quantity: 2,
        revision: 7,
      }],
    });
    window.__orphan_intents = [];
    trading.subscribe_intents((intent) => window.__orphan_intents.push(intent));
    const overlay = document.querySelector("#chart_container canvas:last-of-type").getBoundingClientRect();
    const width = window.__chart.time_scale().width();
    const chip_x = width - 200;
    const y = window.__main.price_to_coordinate(100);
    return {
      from: { x: overlay.left + chip_x, y: overlay.top + y },
      to: { x: overlay.left + chip_x, y: overlay.top + window.__main.price_to_coordinate(98) },
      hit: trading.hit_at(chip_x, y),
    };
  });
  expect(probe.hit).toMatchObject({ id: "orphan-stop", kind: "order_line" });
  await page.mouse.move(probe.from.x, probe.from.y);
  expect(await page.locator("#chart_container canvas:last-of-type").evaluate((canvas) => canvas.style.cursor)).toBe("grab");
  await page.mouse.down();
  await page.mouse.move(probe.to.x, probe.to.y, { steps: 6 });
  await page.mouse.up();
  expect(await page.evaluate(() => window.__orphan_intents)).toEqual([
    expect.objectContaining({
      action: "modify_order",
      order_id: "orphan-stop",
      kind: "stop_limit",
      stop_price: 99.5,
      price: 98,
      base_revision: 7,
    }),
  ]);
});

test("cancel control emits intent without removing authoritative order", async ({ page }) => {
  await open_trading_demo(page);
  const probe = await page.evaluate(() => {
    window.__cancel_intents = [];
    window.__chart.trading().subscribe_intents((intent) => window.__cancel_intents.push(intent));
    const order = window.__chart.trading().state().orders.find((item) => item.id === "demo-stop");
    const overlay = document.querySelector("#chart_container canvas:last-of-type").getBoundingClientRect();
    const y = window.__main.price_to_coordinate(order.price);
    return {
      x: overlay.left + window.__close_x(order.id, y),
      y: overlay.top + y,
    };
  });
  await page.mouse.click(probe.x, probe.y);
  expect(await page.evaluate(() => window.__cancel_intents)).toEqual([
    expect.objectContaining({ action: "cancel_order", order_id: "demo-stop" }),
  ]);
  expect(await page.evaluate(() => window.__chart.trading().state().orders.some((order) => order.id === "demo-stop"))).toBe(true);
});

test("confirmed bracket connector deactivates on an empty-canvas click without removing orders", async ({ page }) => {
  await open_trading_demo(page, "canvas2d");
  const probe = await page.evaluate(() => {
    window.__connector_intents = [];
    const trading = window.__chart.trading();
    trading.subscribe_intents((intent) => window.__connector_intents.push(intent));
    const target = trading.state().orders.find((order) => order.id === "demo-target");
    const overlay = document.querySelector("#chart_container canvas:last-of-type").getBoundingClientRect();
    return {
      overlay: { left: overlay.left, top: overlay.top, width: overlay.width, height: overlay.height },
      pane_width: window.__chart.time_scale().width(),
      from_y: window.__main.price_to_coordinate(target.price),
      to_y: window.__main.price_to_coordinate(target.price + 0.5),
    };
  });
  await page.mouse.move(probe.overlay.left + probe.pane_width - 200, probe.overlay.top + probe.from_y);
  await page.mouse.down();
  await page.mouse.move(probe.overlay.left + 30, probe.overlay.top + probe.to_y, { steps: 5 });
  await page.mouse.up();
  const active = await page.evaluate(() => {
    const trading = window.__chart.trading();
    const intent = window.__connector_intents[0];
    trading.resolve_intent(intent.sequence, true);
    const target = trading.state().orders.find((order) => order.id === "demo-target");
    trading.update_order({ ...target, price: intent.price, revision: target.revision + 1 });
    const stop = trading.state().orders.find((order) => order.id === "demo-stop");
    const canvas = window.__chart.take_screenshot();
    return {
      url: canvas.toDataURL("image/png"),
      width: canvas.width,
      height: canvas.height,
      top: window.__main.price_to_coordinate(intent.price),
      bottom: window.__main.price_to_coordinate(stop.price),
    };
  });

  await page.mouse.click(probe.overlay.left + probe.pane_width / 2, probe.overlay.top + 20);
  const inactive = await page.evaluate(() => {
    const canvas = window.__chart.take_screenshot();
    return {
      url: canvas.toDataURL("image/png"),
      order_ids: window.__chart.trading().state().orders.map((order) => order.id),
    };
  });
  expect(inactive.order_ids).toEqual(["demo-target", "demo-stop", "demo-partial"]);

  const connector_pixels = (url) => {
    const image = PNG.sync.read(Buffer.from(url.split(",")[1], "base64"));
    const scale_x = active.width / probe.overlay.width;
    const scale_y = active.height / probe.overlay.height;
    const x = Math.round((probe.pane_width - 8) * scale_x);
    const y0 = Math.round(Math.min(active.top, active.bottom) * scale_y);
    const y1 = Math.round(Math.max(active.top, active.bottom) * scale_y);
    let count = 0;
    for (let y = y0; y <= y1; y += 1) {
      for (let dx = -2; dx <= 2; dx += 1) {
        const offset = (y * image.width + x + dx) * 4;
        if (
          Math.abs(image.data[offset] - 62) <= 12
          && Math.abs(image.data[offset + 1] - 99) <= 12
          && Math.abs(image.data[offset + 2] - 221) <= 12
          && image.data[offset + 3] > 200
        ) count += 1;
      }
    }
    return count;
  };
  expect(connector_pixels(active.url)).toBeGreaterThan(connector_pixels(inactive.url) + 20);
});

test("working-order markers omit TP/SL controls and retain manual Confirm/Discard", async ({ page }) => {
  await open_trading_demo(page);
  const probe = await page.evaluate(() => {
    const trading = window.__chart.trading();
    trading.apply_snapshot({
      instrument: { tick_size: 0.25, price_precision: 2 },
      orders: [
        { id: "buy-limit", side: "buy", kind: "limit", status: "working", price: 100, quantity: 1 },
        { id: "sell-stop", side: "sell", kind: "stop", status: "working", price: 98, quantity: 1 },
      ],
    });
    trading.set_confirmation_mode("manual");
    window.__manual_intents = [];
    trading.subscribe_intents((intent) => window.__manual_intents.push(intent));
    const overlay = document.querySelector("#chart_container canvas:last-of-type").getBoundingClientRect();
    const width = window.__chart.time_scale().width();
    const marker_start = width - 280;
    return {
      overlay: { left: overlay.left, top: overlay.top },
      width,
      marker_start,
      buy_y: window.__main.price_to_coordinate(100),
      target_y: window.__main.price_to_coordinate(101.25),
      sell_y: window.__main.price_to_coordinate(98),
      sell_target_y: window.__main.price_to_coordinate(97.25),
      hits: {
        empty_left: trading.hit_at(40, window.__main.price_to_coordinate(100)),
        marker: trading.hit_at(width - 200, window.__main.price_to_coordinate(100)),
        cancel: trading.hit_at(
          window.__close_x("buy-limit", window.__main.price_to_coordinate(100)),
          window.__main.price_to_coordinate(100),
        ),
      },
    };
  });
  expect(probe.hits.empty_left).toBeNull();
  expect(probe.hits.marker).toMatchObject({ id: "buy-limit", kind: "order_line" });
  expect(probe.hits.cancel).toMatchObject({ id: "buy-limit", kind: "cancel_button" });

  await page.mouse.move(probe.overlay.left + probe.width - 200, probe.overlay.top + probe.buy_y);
  await page.mouse.down();
  await page.mouse.move(probe.overlay.left + 30, probe.overlay.top + probe.target_y, { steps: 5 });
  await page.mouse.up();
  expect(await page.evaluate(() => ({
    preview: window.__chart.trading().preview(),
    intents: window.__manual_intents,
  }))).toMatchObject({
    preview: { source: "order", order_id: "buy-limit", phase: "awaiting_confirmation" },
    intents: [],
  });

  const manual_main_x = probe.marker_start;
  await page.mouse.click(probe.overlay.left + manual_main_x - 36, probe.overlay.top + probe.target_y);
  const confirmed = await page.evaluate(() => ({
    preview: window.__chart.trading().preview(),
    intents: window.__manual_intents,
    order: window.__chart.trading().state().orders.find((order) => order.id === "buy-limit"),
  }));
  expect(confirmed.preview).toMatchObject({ phase: "pending", price: 101.25 });
  expect(confirmed.intents).toEqual([expect.objectContaining({ action: "modify_order", order_id: "buy-limit", price: 101.25 })]);
  expect(confirmed.order.price).toBe(100);
  await page.evaluate(() => window.__chart.trading().resolve_intent(window.__manual_intents[0].sequence, false));

  await page.mouse.move(probe.overlay.left + probe.width - 200, probe.overlay.top + probe.buy_y);
  await page.mouse.down();
  await page.mouse.move(probe.overlay.left + 30, probe.overlay.top + probe.target_y, { steps: 5 });
  await page.mouse.up();
  await page.mouse.click(probe.overlay.left + manual_main_x - 97, probe.overlay.top + probe.target_y);
  expect(await page.evaluate(() => ({
    preview: window.__chart.trading().preview(),
    intent_count: window.__manual_intents.length,
  }))).toEqual({ preview: null, intent_count: 1 });

  await page.mouse.move(probe.overlay.left + probe.width - 200, probe.overlay.top + probe.sell_y);
  await page.mouse.down();
  await page.mouse.move(probe.overlay.left + 30, probe.overlay.top + probe.sell_target_y, { steps: 5 });
  await page.mouse.up();
  expect(await page.evaluate(() => ({
    preview: window.__chart.trading().preview(),
    intent_count: window.__manual_intents.length,
  }))).toMatchObject({
    preview: {
      source: "order",
      order_id: "sell-stop",
      side: "sell",
      phase: "awaiting_confirmation",
    },
    intent_count: 1,
  });
  await page.mouse.click(probe.overlay.left + manual_main_x - 97, probe.overlay.top + probe.sell_target_y);
  expect(await page.evaluate(() => ({
    preview: window.__chart.trading().preview(),
    intent_count: window.__manual_intents.length,
  }))).toEqual({ preview: null, intent_count: 1 });
});

test("existing TP and SL adjustments keep manual confirmation controls", async ({ page }) => {
  await open_trading_demo(page);
  const probe = await page.evaluate(() => {
    const trading = window.__chart.trading();
    trading.set_confirmation_mode("manual");
    window.__protection_intents = [];
    trading.subscribe_intents((intent) => window.__protection_intents.push(intent));
    const overlay = document.querySelector("#chart_container canvas:last-of-type").getBoundingClientRect();
    const target = trading.state().orders.find((order) => order.id === "demo-target");
    const stop = trading.state().orders.find((order) => order.id === "demo-stop");
    return {
      overlay: { left: overlay.left, top: overlay.top },
      width: window.__chart.time_scale().width(),
      target_y: window.__main.price_to_coordinate(target.price),
      target_next_y: window.__main.price_to_coordinate(target.price + 0.75),
      stop_y: window.__main.price_to_coordinate(stop.price),
      stop_next_y: window.__main.price_to_coordinate(stop.price - 0.75),
    };
  });
  const manual_main_x = probe.width - 297;

  await page.mouse.move(probe.overlay.left + probe.width - 200, probe.overlay.top + probe.target_y);
  await page.mouse.down();
  await page.mouse.move(probe.overlay.left + 30, probe.overlay.top + probe.target_next_y, { steps: 5 });
  await page.mouse.up();
  expect(await page.evaluate(({ x, y }) => ({
    preview: window.__chart.trading().preview(),
    confirm: window.__chart.trading().hit_at(x - 36, y),
    discard: window.__chart.trading().hit_at(x - 97, y),
    intents: window.__protection_intents,
  }), { x: manual_main_x, y: probe.target_next_y })).toMatchObject({
    preview: { source: "order", order_id: "demo-target", phase: "awaiting_confirmation", role: "take_profit" },
    confirm: { id: "demo-target", kind: "confirm_button" },
    discard: { id: "demo-target", kind: "discard_button" },
    intents: [],
  });
  await page.mouse.click(probe.overlay.left + manual_main_x - 36, probe.overlay.top + probe.target_next_y);
  expect(await page.evaluate(() => window.__protection_intents)).toEqual([
    expect.objectContaining({ action: "modify_order", order_id: "demo-target" }),
  ]);
  await page.evaluate(() => window.__chart.trading().resolve_intent(window.__protection_intents[0].sequence, false));

  await page.mouse.move(probe.overlay.left + probe.width - 200, probe.overlay.top + probe.stop_y);
  await page.mouse.down();
  await page.mouse.move(probe.overlay.left + 30, probe.overlay.top + probe.stop_next_y, { steps: 5 });
  await page.mouse.up();
  expect(await page.evaluate(({ x, y }) => ({
    preview: window.__chart.trading().preview(),
    confirm: window.__chart.trading().hit_at(x - 36, y),
    discard: window.__chart.trading().hit_at(x - 97, y),
    intent_count: window.__protection_intents.length,
  }), { x: manual_main_x, y: probe.stop_next_y })).toMatchObject({
    preview: { source: "order", order_id: "demo-stop", phase: "awaiting_confirmation", role: "stop_loss" },
    confirm: { id: "demo-stop", kind: "confirm_button" },
    discard: { id: "demo-stop", kind: "discard_button" },
    intent_count: 1,
  });
  await page.mouse.click(probe.overlay.left + manual_main_x - 97, probe.overlay.top + probe.stop_next_y);
  expect(await page.evaluate(() => ({
    preview: window.__chart.trading().preview(),
    intent_count: window.__protection_intents.length,
  }))).toEqual({ preview: null, intent_count: 1 });
});

for (const backend of ["canvas2d", "webgpu"]) {
  test(`${backend} compact position marker has no TP/SL creation surface and uses an attached close chip`, async ({ page }) => {
    await open_trading_demo(page, backend);
    const probe = await page.evaluate(() => {
      const trading = window.__chart.trading();
      trading.apply_snapshot({
        instrument: { tick_size: 0.25, price_precision: 2, quantity_precision: 0 },
        positions: [{ id: "position-only", side: "long", average_price: 100, quantity: 2 }],
      });
      window.__creation_intents = [];
      trading.subscribe_intents((intent) => window.__creation_intents.push(intent));
      const overlay = document.querySelector("#chart_container canvas:last-of-type").getBoundingClientRect();
      const width = window.__chart.time_scale().width();
      const entry_y = window.__main.price_to_coordinate(100);
      return {
        overlay: { left: overlay.left, top: overlay.top },
        width,
        entry_y,
        empty_left: trading.hit_at(40, entry_y),
        marker: trading.hit_at(width - 200, entry_y),
        close_x: window.__close_x("position-only", entry_y),
        close: trading.hit_at(window.__close_x("position-only", entry_y), entry_y),
      };
    });
    expect(probe.empty_left).toBeNull();
    expect(probe.marker).toMatchObject({ id: "position-only", kind: "position_line" });
    expect(probe.close).toMatchObject({ id: "position-only", kind: "cancel_button" });

    await page.mouse.move(probe.overlay.left + probe.width - 200, probe.overlay.top + probe.entry_y);
    await page.mouse.down();
    await page.mouse.move(
      probe.overlay.left + probe.width - 200,
      probe.overlay.top + probe.entry_y - 40,
      { steps: 6 },
    );
    await page.mouse.up();
    expect(await page.evaluate(() => ({
      intents: window.__creation_intents,
      preview: window.__chart.trading().preview(),
    }))).toEqual({ intents: [], preview: null });

    await page.mouse.click(probe.overlay.left + probe.close_x, probe.overlay.top + probe.entry_y);
    expect(await page.evaluate(() => window.__creation_intents)).toEqual([
      expect.objectContaining({
        action: "close_position",
        position_id: "position-only",
      }),
    ]);
  });
}
