import { expect, test, type Page } from "@playwright/test";
import { initialOperationsSnapshot } from "../../src/operations/operations-port";
import type { TimeSeriesRequest } from "../../src/operations/time-series-port";

const fixedNow = new Date("2026-10-20T22:00:00Z");
const descriptors = [
  { id: "paper-price", label: "Paper price", shortLabel: "Price", domain: "market", unit: "USD", precision: 2, color: "#78a9ef", render: "line", provenance: "Paper market adapter", freshness: "not connected", defaultVisible: true },
  { id: "observed-source", label: "Observed source", shortLabel: "Source", domain: "source", unit: "nT", precision: 2, color: "#cf73ff", render: "line", provenance: "Independent observation adapter", freshness: "not connected", defaultVisible: false },
];

async function observedWorkspace(page: Page) {
  await page.clock.install({ time: fixedNow });
  await page.addInitScript(() => {
    window.__HELIOS_OPERATIONS__ = {
      snapshotUrl: "/test/snapshot", timeSeriesCatalogUrl: "/test/catalog",
      forecastBundlesUrl: "/test/forecasts", timeSeriesQueryUrl: "/test/query",
    };
  });
  await page.route("**/test/snapshot", route => route.fulfill({ json: {
    ...initialOperationsSnapshot, provider: "Test observed gateway", dataClass: "observed", mode: "paper",
    observedAt: "2026-10-19T22:00:00Z", orders: [], fills: [], activity: [], signals: [], positions: [],
  } }));
  await page.route("**/test/catalog", route => route.fulfill({ json: descriptors }));
  await page.route("**/test/forecasts", route => route.fulfill({ json: [] }));
  const state = { fail: false, empty: false, value: 10, requests: [] as TimeSeriesRequest[] };
  await page.route("**/test/query", route => {
    const request = route.request().postDataJSON() as TimeSeriesRequest;
    state.requests.push(request);
    if (state.fail) return route.fulfill({ status: 503, body: "Unavailable" });
    return route.fulfill({ json: {
      schemaVersion: 2, sequence: state.requests.length, from: request.from, to: request.to, markers: [],
      series: descriptors.map(descriptor => ({ descriptor, points: descriptor.id === "paper-price" || state.empty ? [] : [5, 4, 3, 2].map(minutes => ({
        kind: "scalar", timestamp: new Date(Date.parse(request.to) - minutes * 60_000).toISOString(),
        availableAt: new Date(Date.parse(request.to) - minutes * 60_000 + 1000).toISOString(), value: state.value + minutes,
      })) })),
    } });
  });
  return state;
}

test("observed Atlas follows its own clock and selects populated channels", async ({ page }) => {
  const state = await observedWorkspace(page);
  await page.goto("/#overview");
  await expect(page.getByLabel("Visible time series").getByText("Observed source", { exact: true })).toBeVisible();
  expect(Date.parse(state.requests[0].to) - fixedNow.getTime()).toBeGreaterThanOrEqual(0);
  expect(Date.parse(state.requests[0].to) - fixedNow.getTime()).toBeLessThan(10_000);
  await expect(page.getByLabel("Observation coverage")).toContainText("4 samples");
  await expect(page.getByText("No execution event at this time.")).toBeVisible();
  await expect(page.locator(".chart-host canvas").first()).toBeVisible();

  await page.getByRole("tab", { name: /Activity/ }).click();
  await expect(page.getByLabel("Workspace data source")).toContainText("paper ledger");
  await expect(page.getByRole("heading", { name: "No activity recorded" })).toBeVisible();
});

test("polling preserves the chart, retains observations on failure, and pauses for inspection", async ({ page }) => {
  const state = await observedWorkspace(page);
  await page.goto("/#overview");
  const chart = page.locator(".chart-host canvas").first();
  await expect(chart).toBeVisible();
  await chart.evaluate(element => element.setAttribute("data-test-retained", "true"));
  state.value = 20;
  await page.clock.fastForward(31_000);
  await expect(page.getByLabel("Visible time series")).toContainText("22 nT");
  await expect(chart).toHaveAttribute("data-test-retained", "true");
  expect(Date.parse(state.requests.at(-1)!.to)).toBeGreaterThan(fixedNow.getTime());

  state.fail = true;
  await page.clock.fastForward(31_000);
  await expect(page.getByRole("status").filter({ hasText: "Update interrupted" })).toBeVisible();
  await expect(page.getByLabel("Visible time series")).toContainText("22 nT");
  await expect(chart).toHaveAttribute("data-test-retained", "true");
  await page.getByRole("button", { name: "Pause updates" }).click();
  const pausedCount = state.requests.length;
  await page.clock.fastForward(60_000);
  expect(state.requests).toHaveLength(pausedCount);
  state.fail = false;
  await page.getByRole("button", { name: "Resume updates" }).click();
  await expect(page.getByRole("status").filter({ hasText: "Update interrupted" })).toHaveCount(0);
});

test("empty channels show an actionable empty state without manufactured samples", async ({ page }) => {
  const errors: string[] = [];
  page.on("pageerror", error => errors.push(error.message));
  const state = await observedWorkspace(page);
  state.empty = true;
  await page.goto("/#overview");
  await expect(page.getByText("No samples in this window", { exact: true })).toBeVisible();
  await expect(page.getByRole("button", { name: /Available data/ })).toBeDisabled();
  await expect(page.getByRole("button", { name: "Analyze selection", exact: true })).toBeDisabled();
  await page.getByRole("button", { name: "Focus selection", exact: true }).click();
  await page.getByRole("button", { name: "Reset view", exact: true }).click();
  await page.getByRole("button", { name: "View the last day" }).click();
  await expect(page.getByRole("group", { name: "Loaded history window" }).getByRole("button", { name: "1d", exact: true })).toHaveAttribute("aria-pressed", "true");
  await expect(page.getByLabel("Observation coverage")).toContainText("0 samples");
  expect(errors).toEqual([]);
});
