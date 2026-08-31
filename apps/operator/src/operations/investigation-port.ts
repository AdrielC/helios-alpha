import type { OperationsContext } from "./operations-port";

export interface InvestigationCitation {
  readonly id: string;
  readonly label: string;
  readonly timestamp: string;
  readonly sourceId: string;
}

export interface InvestigationRequest {
  readonly schemaVersion: 1;
  readonly context: OperationsContext;
  readonly snapshotSequence: number;
  readonly from: string;
  readonly to: string;
  readonly cursor: string;
  readonly markerId?: string;
  readonly seriesIds: readonly string[];
}

export interface InvestigationResult {
  readonly schemaVersion: 1;
  readonly investigationId: string;
  readonly generatedAt: string;
  readonly modelId: string;
  readonly summary: string;
  readonly limitation: string;
  readonly suggestedSeriesIds: readonly string[];
  readonly citations: readonly InvestigationCitation[];
}

export interface InvestigationPort {
  readonly name: string;
  investigate(request: InvestigationRequest): Promise<InvestigationResult>;
}

export class DemoInvestigationPort implements InvestigationPort {
  readonly name = "DemoInvestigationPort";

  async investigate(request: InvestigationRequest): Promise<InvestigationResult> {
    return {
      schemaVersion: 1,
      investigationId: `demo-${request.snapshotSequence}-${Date.parse(request.cursor)}`,
      generatedAt: new Date().toISOString(),
      modelId: "synthetic-evidence-guide-v1",
      summary: "The signal posterior rose while source quality fell, then risk utilization and P&L changed after the first fill. Inspect spread and source latency before attributing the move to the signal alone.",
      limitation: "Synthetic demonstration. The result is read-only and does not establish causality or execution authority.",
      suggestedSeriesIds: ["bid-ask-spread", "source-latency", "participation"],
      citations: [
        { id: "cite-posterior", label: "Signal posterior", timestamp: request.cursor, sourceId: "signal-posterior" },
        { id: "cite-source", label: "Source quality", timestamp: request.cursor, sourceId: "source-quality" },
        { id: "cite-marker", label: request.markerId ? "Selected lifecycle event" : "Selected interval", timestamp: request.cursor, sourceId: request.markerId ?? "timeline" },
      ],
    };
  }
}

export class HttpInvestigationPort implements InvestigationPort {
  readonly name = "HttpInvestigationPort";
  private readonly url: string;

  constructor(candidate: string) {
    const url = new URL(candidate, window.location.href);
    if (url.origin !== window.location.origin) throw new Error("investigationUrl must be same-origin");
    this.url = url.href;
  }

  async investigate(request: InvestigationRequest): Promise<InvestigationResult> {
    const response = await fetch(this.url, {
      method: "POST",
      credentials: "same-origin",
      headers: { Accept: "application/json", "Content-Type": "application/json" },
      body: JSON.stringify(request),
    });
    if (!response.ok) throw new Error(`Investigation service failed with HTTP ${response.status}`);
    const value = await response.json() as Partial<InvestigationResult>;
    if (value.schemaVersion !== 1 || typeof value.investigationId !== "string" || typeof value.summary !== "string" || !Array.isArray(value.citations) || !Array.isArray(value.suggestedSeriesIds)) {
      throw new Error("Invalid investigation result");
    }
    return value as InvestigationResult;
  }
}

export function createInvestigationPort(): InvestigationPort {
  const config = typeof window !== "undefined" ? window.__HELIOS_OPERATIONS__ : undefined;
  return config?.investigationUrl ? new HttpInvestigationPort(config.investigationUrl) : new DemoInvestigationPort();
}
