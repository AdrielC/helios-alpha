import { expect, test } from "@playwright/test";

test("operational gateway serves observed state through the production ports", async ({ page }) => {
  test.skip(!process.env.HELIOS_OPERATOR_BASE_URL, "Requires a running helio-operatord");
  await page.goto("/#overview");
  await expect(page.getByRole("heading", { name: "Market Atlas" })).toBeVisible();
  await expect(page.locator(".atlas-state")).toHaveCount(0);

  const proof = await page.evaluate(async () => {
    const [health, snapshot, catalog, bundles] = await Promise.all([
      fetch("/api/v1/health").then((response) => response.json()),
      fetch("/api/v1/operations/snapshot").then((response) => response.json()),
      fetch("/api/v1/series/catalog").then((response) => response.json()),
      fetch("/api/v1/forecasts").then((response) => response.json()),
    ]);
    return {
      health,
      snapshot,
      catalogCount: catalog.length,
      bundleIds: bundles.map((bundle: { id: string }) => bundle.id),
      provider: window.__HELIOS_OPERATIONS__?.snapshotUrl,
    };
  });
  expect(proof.health.status).toBe("ok");
  expect(proof.snapshot.provider).toBe("helio-operatord");
  expect(proof.snapshot.dataClass).toBe("observed");
  expect(proof.catalogCount).toBeGreaterThanOrEqual(8);
  expect(proof.bundleIds).toContain("space-weather-impact");
  expect(proof.provider).toBe("/api/v1/operations/snapshot");
});
