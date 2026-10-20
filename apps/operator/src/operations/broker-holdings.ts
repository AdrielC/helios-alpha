import { onBeforeUnmount, onMounted, ref, shallowRef } from "vue";

export interface BrokerHolding {
  asset: string;
  quantity: string;
  sellable_quantity: string;
  quantity_in_open_sells: string;
  managed_by_helios: false;
  cost_basis: string | null;
}
export interface BrokerObservation {
  provider: "robinhood";
  source: "operator_import_of_connected_mcp_responses";
  observed_ms: number;
  positions_observed_ms: number;
  read_at_ms: number;
  account: { crypto: string };
  positions: BrokerHolding[];
  quotes: BrokerQuote[];
  open_order_count: number;
  portfolio: {
    currency: "USD";
    total_value: string;
    cash: string;
    crypto_value: string;
    crypto_buying_power: string | null;
  };
}

export interface BrokerQuote { symbol: string; bid: string; ask: string; mark: string | null; quoted_ms: number; book_valid: boolean }
export interface BrokerSync {
  state: "offline" | "connected" | "syncing" | "authorizing" | "authorization_required" | "degraded";
  interval_ms?: number; last_success_ms?: number | null; next_poll_ms?: number | null; authorization_url?: string;
  strategy?: StrategyObservation;
  account_permissions?: { observed_ms: number; options_level: "option_level_2" | "option_level_3" | "none"; approval_required: boolean };
}
export interface StrategyBook {
  id: string; engine: string; state: string; mode: string; updated_ms: number;
  feed_connected: boolean; active_news: number; signals: { symbol: string; reason: string }[];
}
export interface StrategyObservation {
  observed_ms: number; mode: string; paused: boolean; live_admitted: boolean; live_blockers: string[];
  paper_orders: number; paper_positions: number; model_calls_reserved: number; model_daily_limit: number; books: StrategyBook[];
}
function identifier(value: unknown): string {
  if (typeof value !== "string" || !/^[A-Za-z0-9_.:-]{1,100}$/.test(value)) throw new Error("Invalid status identifier");
  return value;
}
function flag(value: unknown): boolean {
  if (typeof value !== "boolean") throw new Error("Invalid status flag");
  return value;
}
export function parseStrategy(value: unknown): StrategyObservation | undefined {
  if (value == null) return undefined;
  const data = object(value);
  if (!Array.isArray(data.books) || data.books.length > 20 || !Array.isArray(data.live_blockers) || data.live_blockers.length > 20) throw new Error("Invalid strategy status");
  return {
    observed_ms: timestamp(data.observed_ms), mode: identifier(data.mode), paused: flag(data.paused), live_admitted: flag(data.live_admitted),
    live_blockers: data.live_blockers.map(identifier), paper_orders: timestamp(data.paper_orders), paper_positions: timestamp(data.paper_positions),
    model_calls_reserved: timestamp(data.model_calls_reserved), model_daily_limit: timestamp(data.model_daily_limit),
    books: data.books.map(value => {
      const book = object(value);
      if (!Array.isArray(book.signals) || book.signals.length > 100) throw new Error("Invalid strategy signals");
      return { id: identifier(book.id), engine: identifier(book.engine), mode: identifier(book.mode), state: identifier(book.state), updated_ms: timestamp(book.updated_ms), feed_connected: flag(book.feed_connected), active_news: timestamp(book.active_news), signals: book.signals.map(value => { const signal = object(value); return { symbol: identifier(signal.symbol), reason: identifier(signal.reason) }; }) };
    }),
  };
}
const decimal = /^-?\d{1,18}(?:\.\d{1,18})?$/;
function object(value: unknown): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error("Invalid account observation");
  return value as Record<string, unknown>;
}
function amount(value: unknown, nonnegative = false): string {
  if (typeof value !== "string" || !decimal.test(value) || (nonnegative && value.startsWith("-"))) throw new Error("Invalid account amount");
  return value;
}
function timestamp(value: unknown): number {
  if (typeof value !== "number" || !Number.isSafeInteger(value) || value < 0) throw new Error("Invalid account timestamp");
  return value;
}
export function parseBrokerObservation(value: unknown): BrokerObservation | null {
  const envelope = object(value);
  if (envelope.execution_authority !== false) throw new Error("Invalid account authority");
  if (["not_connected", "unavailable"].includes(String(envelope.state)) && envelope.observation === null) return null;
  if (!["available", "stale"].includes(String(envelope.state))) throw new Error("Invalid account state");
  const data = object(envelope.observation), account = object(data.account), portfolio = object(data.portfolio);
  if (data.provider !== "robinhood" || data.source !== "operator_import_of_connected_mcp_responses"
    || portfolio.currency !== "USD" || typeof account.crypto !== "string" || !/^••••[A-Za-z0-9]{4}$/.test(account.crypto)
    || !Array.isArray(data.positions) || data.positions.length > 100) throw new Error("Invalid account observation");
  return {
    provider: data.provider, source: data.source,
    observed_ms: timestamp(data.observed_ms), positions_observed_ms: timestamp(data.positions_observed_ms), read_at_ms: timestamp(data.read_at_ms),
    account: { crypto: account.crypto },
    positions: data.positions.map((value) => {
      const row = object(value);
      if (typeof row.asset !== "string" || !/^[A-Z0-9]{1,12}$/.test(row.asset) || row.managed_by_helios !== false) throw new Error("Invalid holding");
      return { asset: row.asset, quantity: amount(row.quantity, true), sellable_quantity: amount(row.sellable_quantity, true), quantity_in_open_sells: amount(row.quantity_in_open_sells, true), managed_by_helios: false, cost_basis: row.cost_basis == null ? null : amount(row.cost_basis, true) };
    }),
    quotes: (Array.isArray(data.quotes) ? data.quotes : []).map((value) => {
      const q = object(value);
      if (typeof q.symbol !== "string" || !/^[A-Z0-9-]{1,20}$/.test(q.symbol) || typeof q.book_valid !== "boolean") throw new Error("Invalid quote");
      return { symbol: q.symbol, bid: amount(q.bid, true), ask: amount(q.ask, true), mark: q.mark == null ? null : amount(q.mark, true), quoted_ms: timestamp(q.quoted_ms), book_valid: q.book_valid };
    }),
    open_order_count: timestamp(data.open_order_count ?? 0),
    portfolio: { currency: "USD", total_value: amount(portfolio.total_value), cash: amount(portfolio.cash), crypto_value: amount(portfolio.crypto_value), crypto_buying_power: portfolio.crypto_buying_power === null ? null : amount(portfolio.crypto_buying_power) },
  };
}

// Display arithmetic never rounds crypto quantities through USD micros or floats.
export function brokerMoney(value: string | null): string {
  if (value === null) return "Unavailable";
  const negative = value.startsWith("-");
  const [whole, fraction = ""] = value.replace(/^-/, "").split(".");
  const digits = fraction.padEnd(3, "0");
  const cents = BigInt(whole) * 100n + BigInt(digits.slice(0, 2)) + (Number(digits[2]) >= 5 ? 1n : 0n);
  return `${negative ? "-" : ""}$${new Intl.NumberFormat("en-US").format(cents / 100n)}.${(cents % 100n).toString().padStart(2, "0")}`;
}

// Fixed decimal arithmetic for valuation; crypto quantities stay exact strings.
const SCALE = 10n ** 18n;
export function decimalUnits(value: string): bigint {
  const [whole, fraction = ""] = value.replace(/^-/, "").split(".");
  return (value.startsWith("-") ? -1n : 1n) * (BigInt(whole) * SCALE + BigInt(fraction.padEnd(18, "0")));
}
function decimalString(units: bigint): string {
  const magnitude = units < 0n ? -units : units;
  return `${units < 0n ? "-" : ""}${magnitude / SCALE}.${(magnitude % SCALE).toString().padStart(18, "0")}`;
}
export function holdingValues(position: BrokerHolding, quotes: BrokerQuote[]) {
  const quote = quotes.find(q => q.symbol.replace("-", "") === position.asset + "USD");
  // The broker's indicative mark remains useful when routed bid/ask quotes
  // cross. Never manufacture a midpoint from that invalid order book.
  const mark = quote?.mark && decimalUnits(quote.mark) > 0n ? quote.mark
    : quote?.book_valid ? decimalString((decimalUnits(quote.bid) + decimalUnits(quote.ask)) / 2n) : null;
  const units = decimalUnits(position.quantity);
  const value = mark === null ? null : decimalString(units * decimalUnits(mark) / SCALE);
  const average = position.cost_basis === null || units === 0n ? null : decimalString(decimalUnits(position.cost_basis) * SCALE / units);
  const pnl = position.cost_basis === null || value === null ? null : decimalString(decimalUnits(value) - decimalUnits(position.cost_basis));
  return { quote, mark, value, average, pnl };
}
export function decimalPercent(part: string, total: string): number {
  const denominator = decimalUnits(total);
  return denominator <= 0n ? 0 : Number(decimalUnits(part) * 1000n / denominator) / 10;
}

export function useBrokerHoldings() {
  const observation = shallowRef<BrokerObservation | null>(null);
  const sync = shallowRef<BrokerSync>({ state: "offline" });
  const error = ref("");
  const loading = ref(false);
  const now = ref(Date.now());
  let request: AbortController | undefined;
  let poll: ReturnType<typeof setInterval> | undefined;
  let clock: ReturnType<typeof setInterval> | undefined;
  let disposed = false;
  async function refresh(): Promise<void> {
    if (loading.value || disposed) return;
    loading.value = true;
    request = new AbortController();
    const timeout = setTimeout(() => request?.abort(), 8000);
    const syncRead = fetch("/api/v1/operations/broker-sync", { cache: "no-store", signal: request.signal }).then(async response => {
      const value = object(await response.json());
      if (!response.ok || value.execution_authority !== false || !["offline", "connected", "syncing", "authorizing", "authorization_required", "degraded"].includes(String(value.state))) throw new Error();
      const next: BrokerSync = { state: value.state as BrokerSync["state"] };
      // Strategy health can fail independently; it must not hide valid holdings.
      try { next.strategy = parseStrategy(value.strategy); } catch { next.strategy = undefined; }
      if (value.account_permissions) {
        const p = object(value.account_permissions);
        if (["option_level_2", "option_level_3", "none"].includes(String(p.options_level))) next.account_permissions = { observed_ms: timestamp(p.observed_ms), options_level: p.options_level as "option_level_2" | "option_level_3" | "none", approval_required: flag(p.approval_required) };
      }
      for (const key of ["interval_ms", "last_success_ms", "next_poll_ms"] as const) if (value[key] != null) next[key] = timestamp(value[key]);
      if (typeof value.authorization_url === "string") {
        const url = new URL(value.authorization_url);
        if (url.protocol === "https:" && (url.hostname === "robinhood.com" || url.hostname.endsWith(".robinhood.com"))) next.authorization_url = url.href;
      }
      if (!disposed) sync.value = next;
    }).catch(() => { if (!disposed) sync.value = { state: "offline" }; });
    try {
      const response = await fetch("/api/v1/operations/broker-observation", { cache: "no-store", signal: request.signal });
      if (!response.ok) throw new Error("Account read unavailable");
      const next = parseBrokerObservation(await response.json());
      if (disposed) return;
      if (next) { observation.value = next; error.value = ""; }
      else error.value = "Account data is unavailable. This does not mean your account is empty.";
    } catch {
      if (!disposed) error.value = "Could not reload account data. Displaying the last successful account read.";
    } finally {
      await syncRead;
      clearTimeout(timeout);
      loading.value = false;
    }
  }
  async function brokerAction(action: "connect" | "refresh") {
    try {
      const response = await fetch(`/api/v1/operations/broker-sync/${action}`, { method: "POST", headers: { "Content-Type": "application/json" }, body: "{}", signal: AbortSignal.timeout(8000) });
      if (!response.ok) throw new Error();
      sync.value = { ...sync.value, state: "syncing" };
      error.value = "";
    } catch { error.value = "The account sync service is unavailable. Your last account read is preserved."; }
  }
  onMounted(() => {
    void refresh();
    poll = setInterval(() => { void refresh(); }, 3000);
    clock = setInterval(() => { now.value = Date.now(); }, 1000);
  });
  onBeforeUnmount(() => { disposed = true; request?.abort(); clearInterval(poll); clearInterval(clock); });
  return { observation, sync, error, loading, now, refresh, brokerAction };
}
