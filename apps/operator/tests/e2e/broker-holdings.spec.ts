import { expect, test, type Page } from "@playwright/test";

function account(positions = [{ asset: "ETH", quantity: "1.234567891", sellable_quantity: "1.200000001", quantity_in_open_sells: "0.034567890", managed_by_helios: false, cost_basis: "22" }]) {
  const now = Date.now();
  return { state: "available", execution_authority: false, observation: {
    provider: "robinhood", source: "operator_import_of_connected_mcp_responses",
    observed_ms: now, positions_observed_ms: now, read_at_ms: now,
    account: { crypto: "••••9876" }, positions,
    quotes: [{ symbol: "ETHUSD", bid: "20", ask: "22", mark: "21", quoted_ms: now, book_valid: true }], open_order_count: 2,
    portfolio: { currency: "USD", total_value: "125.555", cash: "100.123", crypto_value: "25.432", crypto_buying_power: null },
  } };
}
async function syncRoute(page: Page, state = "connected") {
  await page.route("**/api/v1/operations/broker-sync", route => route.fulfill({ json: { state, execution_authority: false, last_success_ms: state === "connected" ? Date.now() : null, interval_ms: 30000, next_poll_ms: Date.now() + 30000 } }));
}

test("refresh requests broker sync and updates exact quantities through an empty account", async ({ page }) => {
  let response = account(), refreshes = 0;
  await syncRoute(page);
  await page.route("**/api/v1/operations/broker-observation", route => route.fulfill({ json: response }));
  await page.route("**/api/v1/operations/broker-sync/refresh", route => { expect(route.request().method()).toBe("POST"); refreshes++; return route.fulfill({ status: 202, json: { accepted: true } }); });
  await page.goto("/#positions");
  const section = page.getByRole("region", { name: /Robinhood holdings/ });
  await expect(section.getByRole("cell", { name: "1.234567891 ETH", exact: true })).toBeVisible();
  await expect(section.getByText("$25.43", { exact: true })).toBeVisible();
  await expect(section.getByRole("cell", { name: "$25.93", exact: true })).toBeVisible();
  await expect(section.getByRole("cell", { name: "+$3.93", exact: true })).toBeVisible();
  await expect(section.getByText("Not supplied by broker", { exact: true })).toBeVisible();
  await expect(section.getByRole("button", { name: "Trade", exact: true })).toHaveCount(0);
  response = account([{ asset: "SOL", quantity: "9.876543219", sellable_quantity: "9.876543219", quantity_in_open_sells: "0", managed_by_helios: false, cost_basis: "0" }]);
  await section.getByRole("button", { name: "Refresh", exact: true }).click();
  await expect.poll(() => refreshes).toBe(1);
  await expect(section.getByRole("cell", { name: "9.876543219 SOL", exact: true })).toBeVisible();
  await expect(section.getByText("1.234567891 ETH", { exact: true })).toHaveCount(0);
  response = account([]);
  await expect(section.getByText("Robinhood reported no crypto holdings at this account read.")).toBeVisible();
});

test("expired reads and network failure retain known holdings without claiming fresh data", async ({ page }) => {
  const response = account(); response.observation.observed_ms -= 120_000; response.observation.positions_observed_ms -= 120_000;
  let fail = false;
  await syncRoute(page);
  await page.route("**/api/v1/operations/broker-observation", route => fail ? route.fulfill({ status: 503, body: "unavailable" }) : route.fulfill({ json: response }));
  await page.goto("/#positions");
  const section = page.getByRole("region", { name: /Robinhood holdings/ });
  await expect(section.getByText("Account delayed", { exact: true })).toBeVisible();
  fail = true;
  await expect(section.getByRole("status").filter({ hasText: "Account update unavailable" })).toContainText("Could not reload");
  await expect(section.getByRole("cell", { name: "1.234567891 ETH", exact: true })).toBeVisible();
  await expect(page.getByText("Live", { exact: true })).toHaveCount(0);
});

test("disconnected account shows connection action, not a zero balance", async ({ page }) => {
  await syncRoute(page, "authorization_required");
  await page.route("**/api/v1/operations/broker-observation", route => route.fulfill({ json: { state: "not_connected", execution_authority: false, observation: null } }));
  let connects = 0;
  await page.route("**/api/v1/operations/broker-sync/connect", route => { connects++; return route.fulfill({ status: 202, json: { accepted: true } }); });
  await page.goto("/#positions");
  const section = page.getByRole("region", { name: /Robinhood holdings/ });
  await expect(section.getByRole("status")).toContainText("does not mean your account is empty");
  await expect(section.getByText("$0.00", { exact: true })).toHaveCount(0);
  await expect(section.getByRole("button", { name: "Refresh", exact: true })).toBeDisabled();
  await section.getByRole("button", { name: "Connect Robinhood", exact: true }).click();
  await expect.poll(() => connects).toBe(1);
});

test("filtering, account separation and unknown valuation remain honest", async ({ page }) => {
  const response = account(); response.observation.quotes = []; delete (response.observation.positions[0] as { cost_basis?: string }).cost_basis;
  await syncRoute(page);
  await page.route("**/api/v1/operations/broker-observation", route => route.fulfill({ json: response }));
  await page.goto("/#positions");
  const section = page.getByRole("region", { name: /Robinhood holdings/ });
  await expect(section.getByRole("cell", { name: "—", exact: true })).toHaveCount(4);
  await section.getByLabel("Filter holdings").fill("SOL");
  await expect(section.getByText("No assets match your filter.")).toBeVisible();
  await page.getByRole("navigation", { name: "Position account" }).getByRole("button", { name: /Shadow 01/ }).click();
  await expect(section).toHaveCount(0);
  await expect(page.getByRole("heading", { name: /shadow positions/ })).toBeVisible();
});

test("broker holdings fit mobile and keep valuation visible", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await syncRoute(page);
  await page.route("**/api/v1/operations/broker-observation", route => route.fulfill({ json: account() }));
  await page.goto("/#positions");
  await expect(page.getByRole("heading", { name: /Robinhood holdings/ })).toBeVisible();
  const layout = await page.evaluate(() => ({ width: document.documentElement.scrollWidth,
    overflow: [...document.querySelectorAll("header,section,main,nav,div")].filter(element => element.getBoundingClientRect().right > innerWidth + 1).map(element => ({ tag: element.tagName, class: element.className, width: element.getBoundingClientRect().width, overflow: getComputedStyle(element).overflowX })).slice(0, 12) }));
  expect(layout.width, JSON.stringify(layout.overflow)).toBeLessThanOrEqual(391);
  await expect(page.getByLabel("Robinhood holdings. Scroll horizontally for quantities.")).toBeVisible();
});


test("crossed broker bid/ask retains its explicit indicative mark, never invents a midpoint", async ({ page }) => {
  const response = account();
  response.observation.quotes[0] = { symbol: "ETHUSD", bid: "22", ask: "20", mark: "21", quoted_ms: Date.now(), book_valid: false };
  await syncRoute(page);
  await page.route("**/api/v1/operations/broker-observation", route => route.fulfill({ json: response }));
  await page.goto("/#positions");
  const section = page.getByRole("region", { name: /Robinhood holdings/ });
  await expect(section.getByRole("cell", { name: "$25.93", exact: true })).toBeVisible();
  await expect(section.getByText("Broker bid/ask is crossed. Mark and P&L are indicative.")).toBeVisible();
  delete (response.observation.quotes[0] as { mark?: string }).mark;
  await expect(section.getByRole("cell", { name: "—", exact: true })).toHaveCount(2);
});

test("real account separates manual exits from dynamic paper books and options permissions", async ({ page }) => {
  const strategy = { observed_ms: Date.now(), mode: "paper", paused: false, live_admitted: false,
    live_blockers: ["strategy_evidence_not_admitted"], paper_orders: 7, paper_positions: 2,
    model_calls_reserved: 3, model_daily_limit: 12,
    books: [{ id: "private-test-book", engine: "momentum", state: "running", mode: "paper", updated_ms: Date.now(),
      feed_connected: true, active_news: 0, signals: [{ symbol: "SOL-USD", reason: "no_price_confirmation" }] }] };
  await page.route("**/api/v1/operations/broker-sync", route => route.fulfill({ json: { state: "connected", execution_authority: false, last_success_ms: Date.now(), strategy,
    account_permissions: { observed_ms: Date.now(), options_level: "option_level_2", approval_required: true } } }));
  await page.route("**/api/v1/operations/broker-observation", route => route.fulfill({ json: account() }));
  await page.goto("/#positions");
  const monitor = page.getByRole("region", { name: "Crypto strategy monitor" });
  await expect(monitor.getByText("0 live strategies", { exact: true })).toBeVisible();
  await expect(monitor.getByRole("heading", { name: "Momentum", exact: true })).toBeVisible();
  await expect(monitor.getByText("SOL-USD", { exact: true })).toBeVisible();
  await expect(monitor.getByText("Waiting for price confirmation", { exact: true })).toBeVisible();
  await expect(page.getByText("Manual position", { exact: true })).toBeVisible();
  await page.getByText("Account permissions", { exact: true }).click();
  await expect(page.getByText("Level 2", { exact: true })).toBeVisible();
  await expect(page.getByRole("link", { name: "Trade ETH at Robinhood" })).toHaveAttribute("href", "https://robinhood.com/us/en/crypto/eth/");
  await expect(page.getByRole("button", { name: "shadow order" })).toHaveCount(0);
  strategy.books[0].updated_ms -= 90_000;
  await expect(monitor.getByText("Heartbeat delayed", { exact: true })).toBeVisible();
  await expect(monitor.getByText("running", { exact: true })).toHaveCount(0);
  strategy.observed_ms -= 90_000;
  await expect(monitor.getByRole("status")).toContainText("unavailable or delayed");
  await expect(monitor.getByText("0 live strategies", { exact: true })).toHaveCount(0);
});

for (const viewport of [{ width: 1440, height: 900 }, { width: 1280, height: 720 }, { width: 618, height: 952 }, { width: 390, height: 844 }]) {
  test(`pane navigation resets workspace scroll at ${viewport.width}px`, async ({ page }) => {
    await page.setViewportSize(viewport);
    await syncRoute(page);
    await page.route("**/api/v1/operations/broker-observation", route => route.fulfill({ json: account() }));
    await page.goto("/#positions");
    await expect(page.getByRole("heading", { name: /Robinhood holdings/ })).toBeVisible();
    const frame = () => page.evaluate(() => ({ pageHeight: document.documentElement.scrollHeight, height: innerHeight, pageWidth: document.documentElement.scrollWidth, width: innerWidth }));
    const first = await frame();
    expect(first.pageHeight).toBeLessThanOrEqual(first.height + 1);
    expect(first.pageWidth).toBeLessThanOrEqual(first.width + 1);
    const table = page.locator(".broker-table-scroll");
    if (viewport.width <= 900) {
      const values = await table.evaluate(el => ({ width: el.clientWidth, scroll: el.scrollWidth }));
      expect(values.scroll).toBeLessThanOrEqual(values.width + 1);
      await expect(table.getByRole("cell", { name: "$25.93", exact: true })).toBeVisible();
    }
    await page.locator(".operations-workspace").evaluate(el => { el.scrollTop = el.scrollHeight; });
    await page.getByRole("tab", { name: /Sources/ }).click();
    await expect(page.getByRole("heading", { name: "Sources", exact: true })).toBeVisible();
    await expect.poll(() => page.locator(".operations-workspace").evaluate(el => el.scrollTop)).toBe(0);
    await page.getByRole("tab", { name: /Positions/ }).click();
    await expect.poll(() => page.locator(".operations-workspace").evaluate(el => el.scrollTop)).toBe(0);
    expect((await frame()).pageHeight).toBeLessThanOrEqual(viewport.height + 1);
  });
}

test("local owner identity does not claim a connected execution service", async ({ page }) => {
  await page.route("**/api/v1/operations/operator-profile", route => route.fulfill({ json: { display_name: "Test Owner", role: "owner" } }));
  await page.goto("/#positions");
  await page.getByRole("button", { name: /Test Owner/ }).click();
  const menu = page.locator("#operator-menu");
  await expect(menu.locator("dd").getByText("Owner", { exact: true })).toBeVisible();
  await expect(menu.getByText("Not connected", { exact: true })).toBeVisible();
  await expect(page.getByText("Local viewer", { exact: true })).toHaveCount(0);
});

test("allocation uses broker balances and keeps zero or negative totals unknown", async ({ page }) => {
  const response = account();
  await syncRoute(page);
  await page.route("**/api/v1/operations/broker-observation", route => route.fulfill({ json: response }));
  await page.goto("/#positions");
  const allocation = page.getByLabel("Crypto and cash allocation");
  await expect(allocation).toContainText("20.2%");
  await expect(allocation).toContainText("79.8%");
  response.observation.portfolio.crypto_value = "0";
  response.observation.portfolio.cash = "0";
  await expect(page.getByText("Allocation unavailable", { exact: true })).toBeVisible();
  await expect(allocation).toHaveCount(0);
  response.observation.portfolio.cash = "-100";
  await expect(page.locator(".metric").filter({ hasText: "Available cash" })).toContainText("-$100.00");
  await expect(allocation).toHaveCount(0);
});
