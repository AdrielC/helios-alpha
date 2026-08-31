/*
 * Runtime wiring is supplied by the deployment, not committed to the bundle.
 * Example:
 * window.__HELIOS_OPERATIONS__ = {
 *   snapshotUrl: "/api/v1/operations/snapshot",
 *   streamUrl: "/api/v1/operations/stream",
 *   timeSeriesCatalogUrl: "/api/v1/series/catalog",
 *   forecastBundlesUrl: "/api/v1/forecasts",
 *   timeSeriesQueryUrl: "/api/v1/series/query",
 *   investigationUrl: "/api/v1/investigations",
 *   commandSessionUrl: "/api/v1/command/session",
 *   commandUrl: "/api/v1/commands"
 * };
 */
