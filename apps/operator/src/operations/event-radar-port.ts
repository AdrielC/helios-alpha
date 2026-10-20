export interface EventQuote {
  observed_ms: number;
  received_ms: number;
  changed_ms: number | null;
  probability: number;
  bid: number;
  ask: number;
  depth_usd: number;
  spread_pp: number;
}
export interface EventMarket {
  id: string;
  market_id: string;
  event_id: string;
  provider: string;
  title: string;
  resolution?: string;
  url: string;
  topic: string;
  symbols: string[];
  closes_ms: number;
  volume_24h: number;
  volume_unit: string;
  state: string;
  delta_pp: number | null;
  latest: EventQuote | null;
  history: { at_ms: number; probability: number }[];
  samples: number;
}
export interface EventSignal {
  id: string;
  market_id: string;
  provider: string;
  title: string;
  observed_ms: number;
  received_ms: number;
  delta_pp: number;
  probability: number;
  previous_probability: number;
  rule_revision: number;
  expires_ms: number;
  delivered: boolean;
  stream_sequence: number | null;
  symbols: string[];
  execution_authority: false;
}
export interface EventRadarSnapshot {
  schema_version: 1;
  online: boolean;
  observed_ms: number;
  execution_authority: false;
  config: {
    revision: number;
    enabled: boolean;
    poll_seconds: number;
    lookback_seconds: number;
    move_pp: number;
    reset_pp: number;
    cooldown_seconds: number;
    max_spread_pp: number;
    min_depth_usd: number;
    retention_days: number;
  };
  markets: EventMarket[];
  triggers: EventSignal[];
  recorded_observations: number;
  registered_events: number;
  pending_signals: number;
  sources: {
    id: string;
    state: string;
    last_ok_ms: number | null;
    markets: number;
    quotes: number;
    error?: string;
  }[];
  transport: { state: string; subject: string };
  golem: { state: string; last_ok_ms?: number };
  last_command: { id: string; state: string } | null;
  news: {
    news_id: string;
    source: string;
    title: string;
    url: string;
    published_ms: number | null;
    observed_ms: number;
    eligible: boolean;
    intake_reason: string;
  }[];
}
export async function loadEventRadar(): Promise<EventRadarSnapshot | null> {
  const response = await fetch("/api/v1/events/radar", {
    cache: "no-store",
    signal: AbortSignal.timeout(5000),
  });
  if (!response.ok) throw new Error("Event observations could not refresh.");
  const data = await response.json();
  if (data.schema_version !== 1 || data.execution_authority !== false)
    throw new Error("Invalid event observation.");
  if (!data.config) return null;
  if (
    typeof data.config.enabled !== "boolean" ||
    !Number.isSafeInteger(data.config.revision) ||
    !Array.isArray(data.markets) ||
    data.markets.length > 40 ||
    !Array.isArray(data.triggers) ||
    data.triggers.length > 30 ||
    !Array.isArray(data.sources) ||
    !Array.isArray(data.news)
  )
    throw new Error("Invalid event registry.");
  for (const market of data.markets)
    if (
      typeof market.title !== "string" ||
      !Array.isArray(market.symbols) ||
      !Array.isArray(market.history) ||
      market.history.length > 120 ||
      market.history.some(
        (point: { at_ms: number; probability: number }) =>
          !Number.isFinite(point.at_ms) ||
          !Number.isFinite(point.probability) ||
          point.probability < 0 ||
          point.probability > 1,
      ) ||
      (market.latest &&
        (!Number.isFinite(market.latest.probability) ||
          market.latest.probability <= 0 ||
          market.latest.probability >= 1))
    )
      throw new Error("Invalid event quote.");
  return data;
}
export async function setEventDetection(
  enabled: boolean,
  revision: number,
): Promise<string> {
  const response = await fetch("/api/v1/events/radar/detection", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ enabled, expected_revision: revision }),
    signal: AbortSignal.timeout(5000),
  });
  if (!response.ok)
    throw new Error(
      "Detection settings changed or the collector is offline. Refresh and try again.",
    );
  const data = await response.json();
  if (typeof data.id !== "string" || data.state !== "queued")
    throw new Error("Detection change could not be confirmed.");
  return data.id;
}
export function eventUrl(value: string): string | undefined {
  try {
    const url = new URL(value);
    return url.protocol === "https:" &&
      [
        "polymarket.com",
        "kalshi.com",
        "www.federalreserve.gov",
        "www.sec.gov",
        "blog.kraken.com",
        "www.coindesk.com",
        "cointelegraph.com",
        "decrypt.co",
      ].includes(url.hostname)
      ? url.href
      : undefined;
  } catch {
    return undefined;
  }
}
