export interface RegisteredContract {
  id: string;
  title: string;
  closes_ms: number;
  state: string;
  probability: number | null;
  observed_ms: number | null;
  forecast_id: string | null;
  definition_id: string;
  topic: string;
  symbols: string[];
}
export interface RegisteredEvent {
  id: string;
  source_id: string;
  provider: string;
  title: string;
  first_seen_ms: number;
  last_seen_ms: number;
  closes_ms: number;
  state: string;
  topics: string[];
  symbols: string[];
  contracts: RegisteredContract[];
}
export interface ForecastVersion {
  id: string;
  version: number;
  issued_ms: number;
  observed_ms: number;
  probability: number;
  definition_id: string;
  rule_revision: number;
}
export interface RegisteredForecast {
  id: string;
  event_key: string;
  provider: string;
  title: string;
  topic: string;
  symbols: string[];
  resolution: string;
  url: string;
  closes_ms: number;
  state: string;
  kind: "market_implied";
  method: string;
  version_id: string;
  version: number;
  definition_id: string;
  issued_ms: number;
  observed_ms: number;
  confirmed_ms: number;
  probability: number;
  current_probability: number | null;
  current_observed_ms: number | null;
  bid: number;
  ask: number;
  depth_usd: number;
  observation_seq: number;
  rule_revision: number;
  policy: {
    revision: number;
    max_cache_age_seconds: number;
    max_spread_pp: number;
    min_depth_usd: number;
    forecast_interval_seconds: number;
    forecast_min_interval_seconds: number;
    forecast_move_pp: number;
  };
  outcome: number | null;
  score: number | null;
  history: ForecastVersion[];
}
export interface ResearchRegistry {
  schema_version: 1;
  online: boolean;
  observed_ms: number;
  execution_authority: false;
  events: RegisteredEvent[];
  forecasts: RegisteredForecast[];
  total_events: number;
  total_contracts: number;
  total_forecasts: number;
  retained_versions: number;
  retention_days: number;
  display_limit: number;
  issuance_interval_seconds: number;
  issuance_move_pp: number;
}
export async function loadResearchRegistry(): Promise<ResearchRegistry | null> {
  const response = await fetch("/api/v1/research/registry", {
    cache: "no-store",
    signal: AbortSignal.timeout(5000),
  });
  if (!response.ok) throw new Error("Registry refresh interrupted.");
  const data = await response.json();
  if (data.schema_version !== 1 || data.execution_authority !== false)
    throw new Error("Invalid registry response.");
  if (!Array.isArray(data.events) || !Array.isArray(data.forecasts))
    return null;
  return data;
}
