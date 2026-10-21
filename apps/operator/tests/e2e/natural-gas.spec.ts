import { expect, test } from "@playwright/test";
import { initialOperationsSnapshot } from "../../src/operations/operations-port";

test("gas research distinguishes dated premiums, warming signals and execution", async ({ page }) => {
  const at = Date.now();
  await page.clock.install({ time: at });
  await page.addInitScript(() => { window.__HELIOS_OPERATIONS__ = {
    snapshotUrl: "/test/snapshot", timeSeriesCatalogUrl: "/test/catalog",
    forecastBundlesUrl: "/test/forecasts", timeSeriesQueryUrl: "/test/query",
  }; });
  await page.route("**/test/snapshot", r => r.fulfill({ json: { ...initialOperationsSnapshot,
    dataClass: "observed", provider: "Fixture", mode: "paper", observedAt: new Date(at).toISOString(),
    signals: [], positions: [], orders: [], fills: [], activity: [] } }));
  for (const path of ["catalog", "forecasts"]) await page.route(`**/test/${path}`, r => r.fulfill({ json: [] }));
  await page.route("**/api/v1/research/natural-gas", r => r.fulfill({ json: {
    schema_version: 1, observed_ms: at, execution_authority: false, online: true,
    counts: { evidence: 3, quotes: 1, decisions: 0 }, sources: [ { source: "eia-storage", state: "receiving", checked_ms: at } ],
    storage: { period: "2026-10-09", release_date: "2026-Oct-15", storage_bcf: 3567, change_bcf: 67, seasonal_average_bcf: 3522 },
    weather: [{ kind: "hdd", total: 52, period: "2026-10-20" }], signals: [],
    premiums: [{ contract_id: "fixture", kind: "call", expiry: "2026-10-30", strike_cents: 1050,
      bid_cents: 30, ask_cents: 40, premium_cents: 4000, spread_cost_cents: 1000,
      observed_ms: at-3600000, fresh: false }], hypotheses: [], decision_enabled: false,
    max_quote_age_ms: 10000, exit_profiles: [{ id: "runner", take_profit_bps: 20000, stop_loss_bps: 3500, holding_ms: 14400000 }],
    backtest: { blockers: ["Historical option bars contain no bid/ask fills."] },
  } }));
  await page.goto("/#overview");
  const desk = page.getByTestId("natural-gas-desk");
  await desk.locator("summary").click();
  await expect(desk.getByRole("heading", { name: "Option premiums" })).toBeVisible();
  await expect(desk.getByText("$40.00", { exact: true })).toBeVisible();
  await expect(desk.getByText("Dated quote", { exact: true })).toBeVisible();
  await expect(desk.getByText("Profitability has not been established.")).toBeVisible();
  await expect(desk.getByText(/Building the first comparable forecast pair/)).toBeVisible();
  await expect(desk.getByRole("button", { name: /buy|submit|trade/i })).toHaveCount(0);
  await page.clock.fastForward(20000);
  await expect(desk.getByText("Capture offline", { exact: true })).toBeVisible();
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(desk).toBeVisible();
  const bounds = await desk.boundingBox();
  expect(bounds?.width).toBeLessThanOrEqual(390);
});
