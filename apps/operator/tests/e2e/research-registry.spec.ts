import { expect, test, type Page } from "@playwright/test";
import { initialOperationsSnapshot } from "../../src/operations/operations-port";

async function workspace(page: Page) {
  const at = Date.now();
  const definition = "d".repeat(64);
  const forecast = (provider: string, title: string) => ({
    id: `${provider}:contract`,
    event_key: `${provider}:event`,
    provider,
    title,
    topic: "bitcoin",
    symbols: ["BTC-USD"],
    resolution: "Resolves from the named reference at the deadline.",
    url: `https://${provider}.com/markets/test`,
    closes_ms: at + 86400_000,
    state: "current",
    kind: "market_implied",
    method: "prediction-market-midpoint/v1",
    version_id: "version-2",
    version: 2,
    definition_id: definition,
    issued_ms: at - 60000,
    observed_ms: at - 60000,
    confirmed_ms: at,
    probability: 0.57,
    current_probability: 0.58,
    current_observed_ms: at,
    bid: 0.56,
    ask: 0.58,
    depth_usd: 800,
    observation_seq: 12,
    rule_revision: 3,
    policy: {
      revision: 3,
      max_cache_age_seconds: 45,
      max_spread_pp: 5,
      min_depth_usd: 100,
      forecast_interval_seconds: 300,
      forecast_min_interval_seconds: 60,
      forecast_move_pp: 1,
    },
    outcome: null,
    score: null,
    history: [
      {
        id: "v2",
        version: 2,
        issued_ms: at - 60000,
        observed_ms: at - 60000,
        probability: 0.57,
        definition_id: definition,
        rule_revision: 3,
      },
      {
        id: "v1",
        version: 1,
        issued_ms: at - 360000,
        observed_ms: at - 360000,
        probability: 0.51,
        definition_id: definition,
        rule_revision: 3,
      },
    ],
  });
  const forecasts = [
    forecast("kalshi", "Bitcoin above $80,000?"),
    forecast("polymarket", "Bitcoin above $85,000?"),
  ];
  const events = forecasts.map((f) => ({
    id: f.event_key,
    source_id: "event",
    provider: f.provider,
    title: f.provider === "kalshi" ? "Bitcoin tomorrow" : "Bitcoin this month",
    first_seen_ms: at - 400000,
    last_seen_ms: at,
    closes_ms: f.closes_ms,
    state: "watching",
    topics: ["bitcoin"],
    symbols: ["BTC-USD"],
    contracts: [
      {
        id: f.id,
        title: f.title,
        closes_ms: f.closes_ms,
        state: "watching",
        probability: 0.58,
        observed_ms: at,
        forecast_id: f.id,
        definition_id: definition,
        topic: "bitcoin",
        symbols: ["BTC-USD"],
      },
    ],
  }));
  const data = {
    schema_version: 1,
    online: true,
    observed_ms: at,
    execution_authority: false,
    events,
    forecasts,
    total_events: 2,
    total_contracts: 2,
    total_forecasts: 2,
    retained_versions: 4,
    retention_days: 30,
    display_limit: 100,
    issuance_interval_seconds: 300,
    issuance_move_pp: 1,
  };
  const control = { fail: false };
  await page.clock.install({ time: at });
  await page.addInitScript(() => {
    window.__HELIOS_OPERATIONS__ = {
      snapshotUrl: "/test/snapshot",
      timeSeriesCatalogUrl: "/test/catalog",
      forecastBundlesUrl: "/test/forecasts",
      timeSeriesQueryUrl: "/test/query",
    };
  });
  await page.route("**/test/snapshot", (r) =>
    r.fulfill({
      json: {
        ...initialOperationsSnapshot,
        dataClass: "observed",
        provider: "Fixture",
        mode: "paper",
        observedAt: new Date(at).toISOString(),
        signals: [],
        positions: [],
        orders: [],
        fills: [],
        activity: [],
      },
    }),
  );
  await page.route("**/test/catalog", (r) => r.fulfill({ json: [] }));
  await page.route("**/test/forecasts", (r) => r.fulfill({ json: [] }));
  await page.route("**/api/v1/research/registry", (r) =>
    control.fail ? r.fulfill({ status: 503 }) : r.fulfill({ json: data }),
  );
  await page.goto("/#events");
  await expect(
    page.getByRole("heading", { name: "Events", exact: false }),
  ).toBeVisible();
  return { data, control };
}

test("event contracts link to immutable forecast evidence and back", async ({
  page,
}) => {
  await workspace(page);
  await page.getByRole("button", { name: /Bitcoin tomorrow/ }).click();
  const detail = page.getByRole("dialog");
  await expect(
    detail.getByText("Bitcoin above $80,000?", { exact: true }),
  ).toBeVisible();
  await detail.getByRole("button", { name: "View forecast" }).click();
  await expect(
    detail.getByRole("heading", { name: "Bitcoin above $80,000?" }),
  ).toBeVisible();
  await expect(
    detail.getByText("57.0%", { exact: true }).first(),
  ).toBeVisible();
  await expect(
    detail.getByText("Outcome unverified · score pending"),
  ).toBeVisible();
  await expect(detail.getByRole("cell", { name: "51.0%" })).toBeVisible();
  await detail.getByText("Contract terms & provenance").click();
  await expect(
    detail.getByText("Resolves from the named reference at the deadline."),
  ).toBeVisible();
  await detail.getByRole("button", { name: "View event" }).click();
  await expect(
    detail.getByRole("heading", { name: "Bitcoin tomorrow" }),
  ).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(detail).not.toBeVisible();
  await page.getByRole("tab", { name: "Forecasts" }).click();
  await page.getByRole("button", { name: "Research definitions" }).click();
  await expect(
    page.getByRole("heading", { name: "Research definitions" }),
  ).toBeVisible();
  await page.getByRole("button", { name: "Forecast registry" }).click();
  await expect(
    page.getByRole("region", { name: "Forecast registry" }),
  ).toBeVisible();
});

test("filters and mobile layout retain honest offline history", async ({
  page,
}) => {
  const { control } = await workspace(page);
  await page.setViewportSize({ width: 390, height: 844 });
  await page.getByRole("textbox", { name: "Search events" }).fill("month");
  await expect(
    page.getByRole("button", { name: /Bitcoin this month/ }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: /Bitcoin tomorrow/ }),
  ).toHaveCount(0);
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= window.innerWidth,
    ),
  ).toBe(true);
  await page.getByRole("tab", { name: "Forecasts" }).click();
  await page
    .getByRole("combobox", { name: "Registry source" })
    .selectOption("kalshi");
  await expect(
    page.getByRole("button", { name: /Bitcoin above \$80,000/ }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: /Bitcoin above \$85,000/ }),
  ).toHaveCount(0);
  control.fail = true;
  await page.clock.fastForward(20000);
  await expect(page.getByRole("status")).toContainText(
    "Registry refresh interrupted",
  );
  await page.getByRole("button", { name: /Bitcoin above \$80,000/ }).click();
  await expect(
    page.getByRole("dialog").getByText("offline", { exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("dialog").getByRole("cell", { name: "51.0%" }),
  ).toBeVisible();
  expect(
    await page
      .getByRole("dialog")
      .evaluate((e) => e.getBoundingClientRect().width <= window.innerWidth),
  ).toBe(true);
});
