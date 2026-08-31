import { expect, test, type Page } from "@playwright/test";

async function openOperator(page: Page): Promise<void> {
  await page.goto("/#overview");
  await expect(page.getByRole("heading", { name: "Market Atlas" })).toBeVisible();
  await expect(page.getByLabel("Synchronized financial time series")).toBeVisible();
}

test("workspace navigation opens durable panes and the order ticket", async ({ page }) => {
  await openOperator(page);

  await page.getByRole("tab", { name: /Positions/ }).click();
  await expect(page).toHaveURL(/#positions$/);
  await expect(page.getByRole("heading", { name: "Positions" })).toBeVisible();

  await page.getByRole("button", { name: "New order" }).click();
  const dialog = page.getByRole("dialog");
  await expect(dialog).toBeVisible();
  await expect(dialog.getByRole("heading", { name: "New order" }).first()).toBeVisible();
  await dialog.getByLabel("Instrument").fill("SPY");
  await dialog.getByLabel("Quantity").fill("1");
  await dialog.getByLabel("Limit price").fill("500");
  await dialog.getByRole("button", { name: "Review order" }).click();
  await expect(dialog.getByText("Command service required")).toBeVisible();
  await expect(dialog.getByRole("button", { name: "Submit order" })).toBeDisabled();
  await dialog.getByRole("button", { name: "Close order ticket" }).click();
  await expect(dialog).toBeHidden();

  await page.getByRole("tab", { name: /Sources/ }).click();
  await expect(page).toHaveURL(/#sources$/);
  await expect(page.getByRole("heading", { name: "Sources" })).toBeVisible();
  await expect(page.getByRole("navigation", { name: "Data sources" })).toBeVisible();
});

test("market atlas composes, reorders, and scrubs observations", async ({ page }) => {
  await openOperator(page);

  await page.getByRole("button", { name: /Observations/ }).click();
  await expect(page.getByText("Workspace composition")).toBeVisible();
  await page.getByRole("searchbox", { name: "Search registered observations" }).fill("Gross risk");
  const riskSeries = page.locator(".series-options article").filter({ hasText: "Gross risk utilization" });
  await riskSeries.getByRole("button", { name: /Gross risk utilization/ }).click();
  await expect(page.getByLabel("Visible time series").getByText("Gross risk utilization")).toBeVisible();

  const history = page.getByRole("group", { name: "Loaded history window" });
  await history.getByRole("button", { name: "4h" }).click();
  await expect(history.getByRole("button", { name: "4h" })).toHaveAttribute("aria-pressed", "true");

  const firstMoveDown = page.getByRole("button", { name: /^Move .* down$/ }).first();
  await expect(firstMoveDown).toBeEnabled();
  await firstMoveDown.click();

  const cursor = page.getByRole("slider", { name: "Evidence cursor" });
  const timeBefore = await page.locator(".scrubber-actions time").textContent();
  await cursor.evaluate((element: HTMLInputElement) => {
    element.value = "820";
    element.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await expect.poll(async () => page.locator(".scrubber-actions time").textContent()).not.toBe(timeBefore);

  await page.getByRole("button", { name: "Focus selection" }).click();
  await expect(page.getByRole("button", { name: "Reset view" })).toBeEnabled();
});

test("navigation collapse persists across reload", async ({ page }) => {
  await openOperator(page);
  const toggle = page.getByRole("button", { name: "Collapse workspace navigation" });
  await toggle.click();
  await expect(page.getByRole("button", { name: "Expand workspace navigation" })).toBeVisible();
  await page.reload();
  await expect(page.getByRole("button", { name: "Expand workspace navigation" })).toBeVisible();
});

test("operator shell avoids accidental mobile page overflow", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await openOperator(page);

  const dimensions = await page.evaluate(() => ({
    viewport: window.innerWidth,
    page: document.documentElement.scrollWidth,
    atlasClient: document.querySelector<HTMLElement>(".atlas-scroll")?.clientWidth ?? 0,
    atlasScroll: document.querySelector<HTMLElement>(".atlas-scroll")?.scrollWidth ?? 0,
  }));
  expect(dimensions.page).toBeLessThanOrEqual(dimensions.viewport + 1);
  expect(dimensions.atlasScroll).toBeGreaterThan(dimensions.atlasClient);
  await expect(page.getByRole("tablist", { name: "Operations panes" })).toHaveAttribute("aria-orientation", "horizontal");

  await page.getByRole("tab", { name: /Positions/ }).click();
  await expect(page.getByLabel(/Positions\. Scroll horizontally/)).toBeVisible();
});

test("desktop market atlas visual baseline", async ({ page }) => {
  await openOperator(page);
  await expect(page).toHaveScreenshot("market-atlas.png", { fullPage: false });
});
