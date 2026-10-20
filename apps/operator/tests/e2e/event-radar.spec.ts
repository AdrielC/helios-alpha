import { expect, test, type Page } from "@playwright/test";
import { initialOperationsSnapshot } from "../../src/operations/operations-port";

async function workspace(page: Page) {
  const at = Date.now();
  const contract = (provider: string, id: string, title: string) => ({
    id: `${provider}:${id}`,
    market_id: id,
    event_id: "test-event",
    provider,
    title,
    resolution:
      "Fixture contract resolves against its specified reference on its closing date.",
    url: `https://${provider}.com/markets/test`,
    topic: "bitcoin",
    symbols: ["BTC-USD"],
    closes_ms: at + 86400_000,
    volume_24h: 1000,
    volume_unit: "contracts",
    state: "watching",
    delta_pp: 2.5,
    samples: 21,
    latest: {
      observed_ms: at,
      received_ms: at,
      changed_ms: null,
      probability: 0.525,
      bid: 0.52,
      ask: 0.53,
      depth_usd: 200,
      spread_pp: 1,
    },
    history: [
      { at_ms: at - 600000, probability: 0.5 },
      { at_ms: at, probability: 0.525 },
    ],
  });
  const snapshot = {
    schema_version: 1,
    online: true,
    execution_authority: false,
    observed_ms: at,
    config: {
      revision: 1,
      enabled: true,
      poll_seconds: 30,
      lookback_seconds: 600,
      move_pp: 5,
      reset_pp: 2,
      cooldown_seconds: 900,
      max_spread_pp: 5,
      min_depth_usd: 100,
      retention_days: 30,
    },
    markets: [
      contract(
        "kalshi",
        "btc-80000",
        "Bitcoin price tomorrow? · $80,000 or above",
      ),
      contract(
        "polymarket",
        "btc-85000",
        "Will Bitcoin exceed $85,000 this month?",
      ),
    ],
    triggers: [],
    recorded_observations: 42,
    registered_events: 2,
    pending_signals: 0,
    sources: ["kalshi", "polymarket"].map((id) => ({
      id,
      state: "receiving",
      last_ok_ms: at,
      markets: 1,
      quotes: 1,
    })),
    transport: { state: "ready", subject: "helios.events.signals.v1" },
    golem: { state: "recorded" },
    last_command: null as { id: string; state: string } | null,
    news: [],
  };
  await page.clock.install({ time: at });
  await page.addInitScript(() => {
    window.__HELIOS_OPERATIONS__ = {
      snapshotUrl: "/test/snapshot",
      timeSeriesCatalogUrl: "/test/catalog",
      forecastBundlesUrl: "/test/forecasts",
      timeSeriesQueryUrl: "/test/query",
    };
  });
  await page.route("**/test/snapshot", (route) =>
    route.fulfill({
      json: {
        ...initialOperationsSnapshot,
        provider: "Fixture",
        dataClass: "observed",
        mode: "paper",
        observedAt: new Date(at).toISOString(),
        orders: [],
        fills: [],
        positions: [],
        activity: [],
        signals: [],
      },
    }),
  );
  await page.route("**/test/catalog", (route) => route.fulfill({ json: [] }));
  await page.route("**/test/forecasts", (route) => route.fulfill({ json: [] }));
  await page.route("**/test/query", (route) =>
    route.fulfill({
      json: {
        schemaVersion: 2,
        sequence: 1,
        ...route.request().postDataJSON(),
        series: [],
        markers: [],
      },
    }),
  );
  const control = { fail: false, updates: [] as unknown[] };
  await page.route("**/api/v1/events/radar", (route) =>
    control.fail
      ? route.fulfill({ status: 503 })
      : route.fulfill({ json: snapshot }),
  );
  await page.route("**/api/v1/events/radar/detection", (route) => {
    const update = route.request().postDataJSON();
    control.updates.push(update);
    snapshot.config.enabled = update.enabled;
    snapshot.config.revision++;
    snapshot.last_command = { id: "test-command", state: "applied" };
    return route.fulfill({
      status: 202,
      json: { id: "test-command", state: "queued" },
    });
  });
  await page.route("**/api/v1/operations/broker-sync", (route) =>
    route.fulfill({
      json: {
        state: "connected",
        execution_authority: false,
        strategy: {
          observed_ms: at,
          mode: "paper",
          paused: false,
          live_admitted: false,
          live_blockers: [],
          paper_orders: 0,
          paper_positions: 0,
          model_calls_reserved: 0,
          model_daily_limit: 0,
          books: [
            {
              id: "test-research",
              engine: "event_radar",
              state: "running",
              mode: "research",
              updated_ms: at,
              feed_connected: true,
              active_news: 0,
              reference_source: "prediction_markets",
              reference_channel: "public_rest",
              signals: [],
            },
          ],
        },
      },
    }),
  );
  await page.goto("/#overview");
  await page.getByRole("button", { name: /Event radar test-research/ }).click();
  return { snapshot, control };
}

test("event registry preserves contracts, opens history, filters and fits mobile", async ({
  page,
}) => {
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await workspace(page);
  const radar = page.getByRole("region", { name: "Event radar" });
  await expect(radar).toContainText("42");
  await radar
    .getByRole("button", { name: "Bitcoin price tomorrow? · $80,000 or above" })
    .click();
  const details = page.getByRole("dialog", { name: "Selected event" });
  await expect(details).toBeVisible();
  await expect(
    details.getByRole("img", { name: "Observed probability history" }),
  ).toBeVisible();
  await details.getByText("Resolution terms", { exact: true }).click();
  await expect(details).toContainText("Fixture contract resolves");
  await page.keyboard.press("Escape");
  await expect(details).toHaveCount(0);
  await radar.getByRole("button", { name: "polymarket", exact: true }).click();
  await expect(
    radar.getByRole("button", { name: /Bitcoin price tomorrow/ }),
  ).toHaveCount(0);
  await radar.getByRole("searchbox").fill("does-not-exist");
  await expect(radar).toContainText("No contracts match this view.");
  await radar.getByRole("searchbox").fill("");
  await page.setViewportSize({ width: 390, height: 844 });
  await expect
    .poll(() =>
      page.evaluate(
        () => document.documentElement.scrollWidth <= window.innerWidth,
      ),
    )
    .toBe(true);
  await radar.getByRole("button", { name: /Will Bitcoin exceed/ }).click();
  await expect(details).toBeVisible();
  await expect
    .poll(() =>
      page.evaluate(
        () => document.documentElement.scrollWidth <= window.innerWidth,
      ),
    )
    .toBe(true);
  expect(errors).toEqual([]);
});

test("detector switch waits for acknowledgement; failures preserve data and age offline", async ({
  page,
}) => {
  const { control } = await workspace(page);
  const radar = page.getByRole("region", { name: "Event radar" });
  const toggle = radar.getByRole("switch");
  await expect(toggle).toHaveAttribute("aria-checked", "true");
  await toggle.click();
  await expect(toggle).toHaveAttribute("aria-checked", "false");
  await expect(toggle).toHaveText("Detection paused");
  expect(control.updates).toEqual([{ enabled: false, expected_revision: 1 }]);
  control.fail = true;
  await page.clock.fastForward(18000);
  await expect(radar.getByRole("alert")).toContainText("could not refresh");
  await expect(radar).toContainText("Collector offline");
  await expect(toggle).toBeDisabled();
  await expect(radar).toContainText("$80,000 or above");
  await expect(radar.getByText("offline", { exact: true })).toHaveCount(2);
});
