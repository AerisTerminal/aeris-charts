import { test, expect } from "@playwright/test";
import fs from "node:fs";

const matrix = JSON.parse(fs.readFileSync(
  new URL("../fixtures/g1/recharts-3.10.1/matrix.json", import.meta.url),
  "utf8",
));

test("versioned Recharts matrix has 17 scoped executable rows", async ({ page }) => {
  expect(matrix.reference).toMatchObject({
    version: "3.10.1",
    sourceRevision: "ffb918798051ef040bb7f9922d3850c9c189f39f",
    packageLicense: "MIT",
    sourceLicense: "MIT",
    implementationCopied: false,
  });
  expect(matrix.budgetPolicy).toBe(7);
  expect(matrix.rows).toHaveLength(17);
  expect(new Set(matrix.rows.map((row) => row.id)).size).toBe(17);
  for (const row of matrix.rows) {
    expect(["Open", "Partial", "Verified"]).toContain(row.status);
    for (const field of [
      "owner", "dependency", "referenceFixture", "referenceApi", "referenceEvidence", "aerisFixture",
      "aerisApi", "supportedCombinations", "differences",
    ]) expect(row[field], `${row.id}.${field}`).toBeTruthy();
    await page.goto(row.referenceFixture);
    await expect(page.locator("body")).toHaveAttribute("data-ready", "true");
    await expect(page.locator("section")).toHaveAttribute("data-case", row.id);
    await expect(page.locator(".chart svg.recharts-surface").last()).toBeVisible();
    expect(row.referenceEvidence.length, `${row.id}.referenceEvidence`).toBeGreaterThan(0);
    for (const api of row.referenceEvidence) {
      await expect(page.locator(`[data-reference-api="${api}"]`), `${row.id}: ${api}`).toBeVisible();
    }
  }
});

test("public lifecycle fixture creates general-first and one-engine mixed panes", async ({ page }) => {
  const pageErrors = [];
  page.on("pageerror", (error) => pageErrors.push(error.message));
  await page.goto("/g1-lifecycle.html");
  await expect(page.locator("body")).toHaveAttribute("data-ready", "true");
  const initial = await page.evaluate(() => ({
    financialPanes: window.__g1Lifecycle.financial.panes().length,
    financialSchema: window.__g1Lifecycle.financial.exportState().schema_version,
    generalPanes: window.__g1Lifecycle.general.panes().length,
    generalDomain: window.__g1Lifecycle.general.exportState().panes[0].horizontal_domain,
    generalSeries: window.__g1Lifecycle.general.panes()[0].get_series().length,
    restoredState: window.__g1Lifecycle.restoredGeneral.exportState(),
    restoredSeries: window.__g1Lifecycle.restoredGeneral.panes()[0].get_series().map((series) => ({
      kind: series.kind,
      visible: series.options().visible,
    })),
    restoredAxes: window.__g1Lifecycle.restoredGeneral.axes(0).map((axis) => ({
      id: axis.id,
      visible: axis.options().visible,
    })),
    restoredOrder: window.__g1Lifecycle.restoredGeneral.general_series_order(0).map((series) => series.kind),
    sourceState: window.__g1Lifecycle.generalState,
    mixedPanes: window.__g1Lifecycle.mixed.panes().length,
    mixedFinancialClose: window.__g1Lifecycle.mixedFinancial.data()[0].close,
  }));
  expect(initial).toEqual({
    financialPanes: 1,
    financialSchema: 1,
    generalPanes: 1,
    generalDomain: { Category: { scale: "Band" } },
    generalSeries: 2,
    restoredState: initial.sourceState,
    restoredSeries: [
      { kind: "xy_line", visible: false },
      { kind: "column", visible: true },
    ],
    restoredAxes: [
      { id: "general-x", visible: true },
      { id: "general-y", visible: true },
    ],
    restoredOrder: ["xy_line", "column"],
    sourceState: initial.sourceState,
    mixedPanes: 2,
    mixedFinancialClose: 22,
  });
  expect(initial.restoredState.schema_version).toBe(2);
  expect(initial.restoredState.panes[0]).toEqual(initial.sourceState.panes[0]);
  expect(initial.restoredSeries.every(({ kind }) => kind !== "candlestick")).toBe(true);

  await page.getByRole("button", { name: "Try invalid update" }).click();
  await expect(page.getByRole("status")).toContainText("prior mixed state retained");
  expect(await page.evaluate(() => window.__g1Lifecycle.mixed.panes().length)).toBe(2);
  await page.getByRole("button", { name: "Remove general pane" }).click();
  await expect(page.getByRole("status")).toContainText("Financial pane and view retained");
  expect(await page.evaluate(() => ({
    panes: window.__g1Lifecycle.mixed.panes().length,
    close: window.__g1Lifecycle.mixedFinancial.data()[0].close,
  }))).toEqual({ panes: 1, close: 22 });
  await page.getByRole("button", { name: "Readd general pane" }).click();
  await expect(page.getByRole("status")).toContainText("financial and general panes");
  expect(await page.evaluate(() => window.__g1Lifecycle.mixed.panes().length)).toBe(2);
  expect(pageErrors).toEqual([]);
});
