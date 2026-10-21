export type SectionKind = "metrics" | "table" | "notes";
export type Cell = string | number | boolean | null;
export interface ViewField {
  key: string;
  label: string;
  format: "text" | "number" | "currency" | "percent" | "datetime" | "duration";
  scale: number;
  precision: number;
  unit: string;
  currency: string;
}
export interface ViewRow {
  id: string;
  values: Record<string, Cell>;
  observed_ms: number | null;
}
export interface ViewSection {
  id: string;
  kind: SectionKind;
  title: string;
  description: string;
  empty: string;
  fields: ViewField[];
  rows: ViewRow[];
  max_age_ms: number | null;
}
export interface ResearchView {
  schema_version: 1;
  id: string;
  execution_authority: false;
  title: string;
  subtitle: string;
  mark: string;
  capabilities: string[];
  observed_ms: number;
  lease_ms: number;
  online: boolean;
  sections: ViewSection[];
}
export interface ViewRegistry {
  schema_version: 1;
  execution_authority: false;
  available: boolean;
  refresh_ms: number;
  rejected_count: number;
  books: ResearchView[];
}
export interface RendererProps { section: ViewSection; online: boolean; now: number }

export const sectionKinds: readonly SectionKind[] = ["metrics", "table", "notes"];
const formats = ["text", "number", "currency", "percent", "datetime", "duration"];

export function fresh(at: number | null, maxAge: number | null, now: number): boolean {
  return at !== null && maxAge !== null && at <= now && now - at <= maxAge;
}

export function formatCell(value: Cell | undefined, field: ViewField): string {
  if (value === null || value === undefined) return "—";
  if (field.format === "text") return String(value);
  if (typeof value !== "number" || !Number.isFinite(value)) return "—";
  const amount = value / field.scale;
  if (field.format === "datetime") return formatTime(amount);
  if (field.format === "duration") {
    if (amount >= 86400000) return `${(amount / 86400000).toLocaleString()} ${amount === 86400000 ? 'day' : 'days'}`;
    if (amount >= 3600000) return `${(amount / 3600000).toLocaleString()} ${amount === 3600000 ? 'hour' : 'hours'}`;
    return `${(amount / 60000).toLocaleString()} min`;
  }
  const number = new Intl.NumberFormat(undefined, {
    minimumFractionDigits: field.precision,
    maximumFractionDigits: field.precision,
    ...(field.format === "currency" ? { style: "currency", currency: field.currency } : {}),
  }).format(amount);
  return `${number}${field.format === "percent" ? "%" : ""}${field.unit ? ` ${field.unit}` : ""}`;
}

export function formatTime(at: number): string {
  return new Date(at).toLocaleString(undefined, {
    month: "short", day: "numeric", hour: "numeric", minute: "2-digit", timeZoneName: "short",
  });
}

export async function loadResearchViews(signal: AbortSignal): Promise<ViewRegistry> {
  const response = await fetch("/api/v1/research/views", { cache: "no-store", signal });
  if (!response.ok) throw new Error("Research registry unavailable");
  const data: ViewRegistry = await response.json();
  if (data.schema_version !== 1 || data.execution_authority !== false || !data.available
    || !Array.isArray(data.books) || data.books.length > 32
    || !Number.isInteger(data.refresh_ms) || data.refresh_ms < 1000 || data.refresh_ms > 60000
    || !Number.isInteger(data.rejected_count) || data.rejected_count < 0
    || new Set(data.books.map(b => b.id)).size !== data.books.length) {
    throw new Error("Invalid research registry");
  }
  for (const book of data.books) {
    if (book.schema_version !== 1 || book.execution_authority !== false
      || !/^[a-z][a-z0-9-]{0,63}$/.test(book.id)
      || typeof book.title !== "string" || typeof book.subtitle !== "string" || typeof book.mark !== "string"
      || !Array.isArray(book.capabilities) || !book.capabilities.every(c => typeof c === "string")
      || !Number.isSafeInteger(book.observed_ms) || book.observed_ms < 0
      || !Number.isInteger(book.lease_ms) || book.lease_ms < 1000 || book.lease_ms > 300000
      || typeof book.online !== "boolean" || !Array.isArray(book.sections) || book.sections.length > 16) {
      throw new Error("Invalid research view");
    }
    for (const section of book.sections) {
      if (!sectionKinds.includes(section.kind) || !Array.isArray(section.fields) || !Array.isArray(section.rows)
        || section.fields.length < 1 || section.fields.length > 12 || section.rows.length > 100
        || section.fields.some(f => !formats.includes(f.format) || !Number.isFinite(f.scale) || f.scale <= 0
          || !Number.isInteger(f.precision) || f.precision < 0 || f.precision > 6 || !/^[A-Z]{3}$/.test(f.currency))) {
        throw new Error("Unsupported research renderer");
      }
      for (const row of section.rows) {
        if (!row.values || typeof row.values !== "object"
          || Object.values(row.values).some(v => v !== null && !["string", "number", "boolean"].includes(typeof v))
          || (row.observed_ms !== null && (!Number.isSafeInteger(row.observed_ms) || row.observed_ms < 0))) {
          throw new Error("Invalid research observation");
        }
      }
    }
  }
  return data;
}
