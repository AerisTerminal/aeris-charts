import { test, expect } from "@playwright/test";
import { PNG } from "pngjs";

async function open_trading_demo(page, backend = "canvas2d") {
  await page.goto(`/?feature=trading&backend=${backend}`);
  await page.waitForFunction(() => window.__feature_lab?.active_ids().includes("trading-bracket"));
  await page.evaluate(() => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))));
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

test("trading lines use dedicated hits and render semantic regions through the shared frame", async ({ page }) => {
  await open_trading_demo(page);
  const hit = await page.evaluate(() => {
    const trading = window.__chart.trading();
    const target = trading.state().orders.find((order) => order.id === "demo-target");
    return trading.hit_at(80, window.__main.price_to_coordinate(target.price));
  });
  expect(hit).toMatchObject({
    object_type: "order",
    id: "demo-target",
    kind: "order_line",
  });

  const url = await page.evaluate(() => window.__chart.take_screenshot().toDataURL("image/png"));
  const image = PNG.sync.read(Buffer.from(url.split(",")[1], "base64"));
  expect(count_near(image, [62, 99, 221]), "position line/label pixels").toBeGreaterThan(100);
  expect(count_near(image, [8, 153, 129]), "profit line/label pixels").toBeGreaterThan(100);
  expect(count_near(image, [247, 82, 95]), "risk line/label pixels").toBeGreaterThan(100);
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
      from: { x: overlay.left + 100, y: overlay.top + window.__main.price_to_coordinate(order.price) },
      to: { x: overlay.left + 100, y: overlay.top + window.__main.price_to_coordinate(order.price + 1.25) },
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

test("cancel control emits intent without removing authoritative order", async ({ page }) => {
  await open_trading_demo(page);
  const probe = await page.evaluate(() => {
    window.__cancel_intents = [];
    window.__chart.trading().subscribe_intents((intent) => window.__cancel_intents.push(intent));
    const order = window.__chart.trading().state().orders.find((item) => item.id === "demo-stop");
    const overlay = document.querySelector("#chart_container canvas:last-of-type").getBoundingClientRect();
    return {
      x: overlay.left + window.__chart.time_scale().width() - 10,
      y: overlay.top + window.__main.price_to_coordinate(order.price),
    };
  });
  await page.mouse.click(probe.x, probe.y);
  expect(await page.evaluate(() => window.__cancel_intents)).toEqual([
    expect.objectContaining({ action: "cancel_order", order_id: "demo-stop" }),
  ]);
  expect(await page.evaluate(() => window.__chart.trading().state().orders.some((order) => order.id === "demo-stop"))).toBe(true);
});
