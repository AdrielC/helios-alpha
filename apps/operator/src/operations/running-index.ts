import type { StrategyBook, StrategyObservation } from "./broker-holdings";
import type { TimeSeriesDescriptor } from "./time-series-port";

export const readable = (value: string) => ({ event_radar: "Event radar", prediction_markets: "Prediction markets", news_shock: "News shock", coinbase: "Coinbase Exchange", coindesk: "CoinDesk", cointelegraph: "Cointelegraph", decrypt: "Decrypt", "fed-monetary": "Federal Reserve", "sec-press": "SEC press releases", kraken: "Kraken blog" }[value] ?? value.replaceAll("_", " "));
export function bookState(book: StrategyBook, strategy: StrategyObservation | undefined, now: number): string {
  if (!strategy || now < strategy.observed_ms || now - strategy.observed_ms > 15_000 || now < book.updated_ms || now - book.updated_ms > 30_000) return "Delayed";
  return strategy.paused && book.mode !== "research" ? "Paused" : readable(book.state);
}
export function bookSeries(book: StrategyBook, catalog: readonly TimeSeriesDescriptor[], symbol?: string): string[] {
  const symbols = symbol ? [symbol] : [...new Set([...book.signals.map(signal => signal.symbol), ...Object.keys(book.quote_age_ms ?? {})])];
  return catalog.filter(series => series.sourceNames?.includes(book.reference_source ?? "") && symbols.some(value => series.sourceNames?.includes(value))).map(series => series.id);
}
export function ageLabel(at: number | null | undefined, now: number): string {
  if (at == null || at > now) return "Unknown";
  const seconds = Math.floor((now - at) / 1000);
  if (seconds < 60) return `${seconds}s ago`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m ago`;
  return `${Math.floor(seconds / 3600)}h ago`;
}

export const isEventBook = (book: StrategyBook) => book.engine === "event_radar";
