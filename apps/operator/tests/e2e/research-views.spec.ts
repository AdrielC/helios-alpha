import { expect, test, type Page } from "@playwright/test";
import fixture from "../../../../tests/fixtures/research-view.json" with { type: "json" };
import { initialOperationsSnapshot } from "../../src/operations/operations-port";

const at = fixture.observed_ms;
const book = () => ({ ...structuredClone(fixture), online: true });

async function workspace(page: Page, books = [book()]) {
  const control = { books, fail: false };
  await page.clock.install({ time: at });
  await page.addInitScript(() => { window.__HELIOS_OPERATIONS__ = {
    snapshotUrl: "/test/snapshot", timeSeriesCatalogUrl: "/test/catalog",
    forecastBundlesUrl: "/test/forecasts", timeSeriesQueryUrl: "/test/query",
  }; });
  await page.route("**/test/snapshot", r => r.fulfill({ json: {
    ...initialOperationsSnapshot, dataClass: "observed", provider: "Fixture", mode: "paper",
    observedAt: new Date(at).toISOString(), signals: [], positions: [], orders: [], fills: [], activity: [],
  } }));
  for (const path of ["catalog", "forecasts"]) await page.route(`**/test/${path}`, r => r.fulfill({ json: [] }));
  await page.route("**/api/v1/research/views", r => control.fail
    ? r.fulfill({ status: 503 })
    : r.fulfill({ json: {
      schema_version: 1, execution_authority: false, available: true,
      refresh_ms: 5000, rejected_count: 0, books: control.books,
    } }));
  await page.goto("/#overview");
  return control;
}

test("registered books render with no domain component and follow registration changes", async ({ page }) => {
  const battery = { ...book(), id: "battery-study", title: "Battery dispatch", mark: "BD" };
  battery.sections[2].rows[0].values.note = "<script>window.injected = true</script>";
  const control = await workspace(page, [book(), battery]);
  const views = page.getByTestId("research-view");
  await expect(views).toHaveCount(2);
  const rates = views.filter({ hasText: "Rate revisions" });
  await rates.locator("summary").click();
  await expect(rates.getByRole("heading", { name: "Policy rate" })).toBeVisible();
  await expect(rates.getByText("4.25%", { exact: true })).toHaveCount(2);
  await expect(rates.getByRole("button", { name: /buy|submit|trade/i })).toHaveCount(0);
  const batteries = views.filter({ hasText: "Battery dispatch" });
  await batteries.locator("summary").click();
  await expect(batteries.getByText("<script>window.injected = true</script>", { exact: true })).toBeVisible();
  expect(await page.evaluate(() => "injected" in window)).toBe(false);
  control.books = [battery];
  await page.clock.fastForward(6000);
  await expect(views).toHaveCount(1);
  await expect(batteries).toHaveAttribute("open", "");
  control.books = [];
  await page.clock.fastForward(6000);
  await expect(views).toHaveCount(0);
  await expect(page.getByText("Natural gas", { exact: true })).toHaveCount(0);
});

test("freshness expires independently of the collector and failed polls keep honest cached data", async ({ page }) => {
  const control = await workspace(page);
  const view = page.getByTestId("research-view");
  await view.locator("summary").click();
  await expect(view.getByText("Current", { exact: true })).toBeVisible();
  await page.clock.fastForward(12000);
  await expect(view.getByText("Dated observation", { exact: true })).toBeVisible();
  await expect(view.getByText("Capturing", { exact: true })).toBeVisible();
  control.fail = true;
  await page.clock.fastForward(6000);
  await expect(view.getByText("Capture offline", { exact: true })).toBeVisible();
  await expect(page.getByText(/Research refresh interrupted/)).toBeVisible();
  await expect(view.getByText("4.25%", { exact: true })).toHaveCount(2);
  await page.setViewportSize({ width: 390, height: 844 });
  const bounds = await view.boundingBox();
  expect(bounds?.width).toBeLessThanOrEqual(390);
  expect(bounds?.x).toBeGreaterThanOrEqual(0);
});

test("an empty registry creates no phantom book; unsupported renderer cannot mount", async ({ page }) => {
  const control = await workspace(page, []);
  await expect(page.getByTestId("research-view")).toHaveCount(0);
  const unsupported = book();
  unsupported.sections[0].kind = "remote-script";
  control.books = [unsupported];
  await page.clock.fastForward(6000);
  await expect(page.getByText(/Research refresh interrupted/)).toBeVisible();
  await expect(page.getByTestId("research-view")).toHaveCount(0);
});
