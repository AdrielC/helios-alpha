export type CommandAction =
  | "pause_strategy"
  | "resume_strategy"
  | "pause_before_stage"
  | "cancel_order"
  | "flatten_position"
  | "activate_kill_switch";

export interface CommandIntent {
  readonly action: CommandAction;
  readonly targetId: string;
  readonly reason: string;
  readonly confirmation: string;
}

export interface CommandAuthority {
  readonly state: "unavailable" | "authenticated" | "expired";
  readonly operator?: string;
  readonly expiresAt?: string;
  readonly detail: string;
}

export interface CommandReceipt {
  readonly schemaVersion: 1;
  readonly commandId: string;
  readonly idempotencyKey: string;
  readonly action: CommandAction;
  readonly targetId: string;
  readonly status: "accepted" | "completed" | "rejected";
  readonly submittedAt: string;
  readonly message: string;
  readonly expectedSequence: number;
}

export interface CommandPort {
  readonly name: string;
  describe(): Promise<CommandAuthority>;
  execute(intent: CommandIntent, expectedSequence: number): Promise<CommandReceipt>;
}

class UnavailableCommandPort implements CommandPort {
  readonly name = "UnavailableCommandPort";

  async describe(): Promise<CommandAuthority> {
    return {
      state: "unavailable",
      detail: "No authenticated command service is configured for this deployment",
    };
  }

  async execute(): Promise<CommandReceipt> {
    throw new Error("Command service unavailable");
  }
}

interface CommandSession {
  readonly schemaVersion: 1;
  readonly operator: string;
  readonly expiresAt: string;
  readonly csrfToken: string;
}

interface HttpCommandOptions {
  readonly commandUrl: string;
  readonly sessionUrl: string;
}

function sameOrigin(candidate: string, field: string): string {
  const url = new URL(candidate, window.location.href);
  if (url.origin !== window.location.origin) throw new Error(`${field} must be same-origin`);
  return url.href;
}

function record(value: unknown, path: string): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new Error(`Invalid command response at ${path}`);
  }
  return value as Record<string, unknown>;
}

function text(value: unknown, path: string): string {
  if (typeof value !== "string" || value.length === 0) {
    throw new Error(`Invalid command response at ${path}`);
  }
  return value;
}

function parseSession(value: unknown): CommandSession {
  const session = record(value, "session");
  if (session.schemaVersion !== 1) throw new Error("Unsupported command session schema");
  const expiresAt = text(session.expiresAt, "session.expiresAt");
  if (!Number.isFinite(Date.parse(expiresAt))) {
    throw new Error("Invalid command response at session.expiresAt");
  }
  return {
    schemaVersion: 1,
    operator: text(session.operator, "session.operator"),
    expiresAt,
    csrfToken: text(session.csrfToken, "session.csrfToken"),
  };
}

function parseReceipt(value: unknown, idempotencyKey: string): CommandReceipt {
  const receipt = record(value, "receipt");
  if (receipt.schemaVersion !== 1) throw new Error("Unsupported command receipt schema");
  const action = text(receipt.action, "receipt.action") as CommandAction;
  const actions: readonly CommandAction[] = [
    "pause_strategy",
    "resume_strategy",
    "pause_before_stage",
    "cancel_order",
    "flatten_position",
    "activate_kill_switch",
  ];
  if (!actions.includes(action)) throw new Error("Invalid command response at receipt.action");
  const status = text(receipt.status, "receipt.status");
  if (!["accepted", "completed", "rejected"].includes(status)) {
    throw new Error("Invalid command response at receipt.status");
  }
  const expectedSequence = receipt.expectedSequence;
  if (
    typeof expectedSequence !== "number" ||
    !Number.isSafeInteger(expectedSequence) ||
    expectedSequence < 0
  ) {
    throw new Error("Invalid command response at receipt.expectedSequence");
  }
  const echoedKey = text(receipt.idempotencyKey, "receipt.idempotencyKey");
  if (echoedKey !== idempotencyKey) throw new Error("Command receipt idempotency key mismatch");
  const submittedAt = text(receipt.submittedAt, "receipt.submittedAt");
  if (!Number.isFinite(Date.parse(submittedAt))) {
    throw new Error("Invalid command response at receipt.submittedAt");
  }
  return {
    schemaVersion: 1,
    commandId: text(receipt.commandId, "receipt.commandId"),
    idempotencyKey: echoedKey,
    action,
    targetId: text(receipt.targetId, "receipt.targetId"),
    status: status as CommandReceipt["status"],
    submittedAt,
    message: text(receipt.message, "receipt.message"),
    expectedSequence,
  };
}

export class HttpCommandPort implements CommandPort {
  readonly name = "HttpCommandPort";
  private readonly commandUrl: string;
  private readonly sessionUrl: string;
  private session: CommandSession | undefined;

  constructor(options: HttpCommandOptions) {
    this.commandUrl = sameOrigin(options.commandUrl, "commandUrl");
    this.sessionUrl = sameOrigin(options.sessionUrl, "commandSessionUrl");
  }

  async describe(): Promise<CommandAuthority> {
    try {
      const session = await this.loadSession();
      const expired = Date.parse(session.expiresAt) <= Date.now();
      return {
        state: expired ? "expired" : "authenticated",
        operator: session.operator,
        expiresAt: session.expiresAt,
        detail: expired
          ? "Command session expired. Reauthenticate before issuing a command"
          : "Commands require a reason, typed confirmation, and current snapshot sequence",
      };
    } catch (error) {
      this.session = undefined;
      return {
        state: "unavailable",
        detail: error instanceof Error ? error.message : "Command session unavailable",
      };
    }
  }

  async execute(intent: CommandIntent, expectedSequence: number): Promise<CommandReceipt> {
    if (intent.reason.trim().length < 12) {
      throw new Error("Command reason must contain at least 12 characters");
    }
    const session = await this.requireSession();
    const idempotencyKey = crypto.randomUUID();
    const response = await fetch(this.commandUrl, {
      method: "POST",
      credentials: "same-origin",
      cache: "no-store",
      headers: {
        Accept: "application/json",
        "Content-Type": "application/json",
        "Idempotency-Key": idempotencyKey,
        "If-Match": `"${expectedSequence}"`,
        "X-Helios-Command": "1",
        "X-Helios-CSRF": session.csrfToken,
      },
      body: JSON.stringify({
        schemaVersion: 1,
        ...intent,
        expectedSequence,
      }),
    });
    if (!response.ok) {
      if (response.status === 409 || response.status === 412) {
        throw new Error("Snapshot changed before command admission. Refresh and review again");
      }
      throw new Error(`Command service rejected the request with HTTP ${response.status}`);
    }
    const receipt = parseReceipt(await response.json(), idempotencyKey);
    if (
      receipt.action !== intent.action ||
      receipt.targetId !== intent.targetId ||
      receipt.expectedSequence !== expectedSequence
    ) {
      throw new Error("Command receipt does not match the reviewed intent");
    }
    return receipt;
  }

  private async loadSession(): Promise<CommandSession> {
    const response = await fetch(this.sessionUrl, {
      credentials: "same-origin",
      cache: "no-store",
      headers: { Accept: "application/json" },
    });
    if (!response.ok) {
      throw new Error(`Command session failed with HTTP ${response.status}`);
    }
    this.session = parseSession(await response.json());
    return this.session;
  }

  private async requireSession(): Promise<CommandSession> {
    const expiresSoon =
      !this.session || Date.parse(this.session.expiresAt) <= Date.now() + 5_000;
    const session = expiresSoon ? await this.loadSession() : this.session;
    if (!session || Date.parse(session.expiresAt) <= Date.now()) {
      throw new Error("Command session expired");
    }
    return session;
  }
}

export function createCommandPort(): CommandPort {
  const config = typeof window !== "undefined" ? window.__HELIOS_OPERATIONS__ : undefined;
  if (config?.commandUrl && config.commandSessionUrl) {
    return new HttpCommandPort({
      commandUrl: config.commandUrl,
      sessionUrl: config.commandSessionUrl,
    });
  }
  return new UnavailableCommandPort();
}
