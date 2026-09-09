// The `/api/v1` client. One token, one host; a refusal from the API is routed
// to the auth layer so the console shows the login page instead of a dead
// screen, and every other error keeps the server's `{error}` text verbatim.

import { API_URL } from "../auth/oauth";
import type {
  BotDetail,
  BotSummary,
  Me,
  StartStatus,
  StopStatus,
  TemplateDescription,
} from "./types";

export class ApiError extends Error {
  status: number;
  retryable: boolean;
  body: Record<string, unknown>;
  constructor(status: number, body: Record<string, unknown>, fallback: string) {
    super(typeof body.error === "string" ? body.error : fallback);
    this.status = status;
    this.retryable = body.retryable === true;
    this.body = body;
  }
}

/**
 * The server refused the token. For "expired" and "scope" the auth layer has
 * dropped the session; for "unlinked" it keeps it, because the token is good
 * and what is missing is an account — the signup page's job.
 */
export class AuthRefused extends Error {
  kind: "unlinked" | "expired" | "scope";
  constructor(kind: "unlinked" | "expired" | "scope") {
    super(
      kind === "unlinked"
        ? "This Google account has no account here yet."
        : kind === "scope"
          ? "The session lacks a scope this action needs — sign in again."
          : "The session has expired — sign in again.",
    );
    this.kind = kind;
  }
}

type Handlers = {
  token: () => Promise<string | null>;
  refuse: (reason: "unlinked" | "expired" | "scope") => void;
};
let handlers: Handlers = { token: async () => null, refuse: () => {} };
export function setAuthHandlers(h: Handlers): void {
  handlers = h;
}

async function request<T>(method: string, path: string, body?: unknown): Promise<T> {
  const token = await handlers.token();
  if (!token) {
    handlers.refuse("expired");
    throw new AuthRefused("expired");
  }
  const init: RequestInit = {
    method,
    headers: { Authorization: `Bearer ${token}` },
    cache: "no-store",
  };
  if (body !== undefined) {
    init.headers = { ...init.headers, "Content-Type": "application/json" };
    init.body = JSON.stringify(body);
  }
  let r: Response;
  try {
    r = await fetch(`${API_URL}api/v1${path}`, init);
  } catch (e) {
    throw new ApiError(0, { error: `network error: ${(e as Error).message}`, retryable: true }, "network error");
  }
  const json = (await r.json().catch(() => ({}))) as Record<string, unknown>;
  if (r.ok) return json as T;

  if (r.status === 401) {
    handlers.refuse("expired");
    throw new AuthRefused("expired");
  }
  if (r.status === 403) {
    // A 403 with `error="insufficient_scope"` is a good token short of a scope;
    // one without any error code is a verified subject nobody has linked.
    const www = r.headers.get("www-authenticate") ?? "";
    const kind = /error="insufficient_scope"/.test(www) ? "scope" : "unlinked";
    handlers.refuse(kind);
    throw new AuthRefused(kind);
  }
  throw new ApiError(r.status, json, `HTTP ${r.status}`);
}

export const api = {
  me: () => request<Me>("GET", "/me"),
  /** Create the account behind the signed-in subject, or find the one it has. */
  signup: () =>
    request<{ status: "created" | "existing"; user_id: string; vip_level: number }>("POST", "/signup"),
  bindTicket: () =>
    request<{ token: string; url: string | null; expires_in: number }>("POST", "/me/telegram/bind-ticket"),
  unbindTelegram: () => request<{ released: number }>("DELETE", "/me/telegram"),

  listBots: () => request<{ bots: BotSummary[] }>("GET", "/bots"),
  getBot: (id: string) => request<BotDetail>("GET", `/bots/${encodeURIComponent(id)}`),
  addBot: (body: { name: string; api_key: string; secret_key: string; overwrite?: boolean }) =>
    request<{ status: "added" | "overwritten"; bot: BotSummary }>("POST", "/bots", body),
  deleteBot: (id: string) =>
    request<{ status: "deleted" }>("DELETE", `/bots/${encodeURIComponent(id)}`, { confirm: id }),
  startBot: (id: string) =>
    request<{ status: StartStatus; task_id?: string }>("POST", `/bots/${encodeURIComponent(id)}/start`),
  stopBot: (id: string) =>
    request<{ status: StopStatus; task_id?: string }>("POST", `/bots/${encodeURIComponent(id)}/stop`),
  setRisk: (id: string, long: number, short: number) =>
    request<{ status: "updated" }>("PUT", `/bots/${encodeURIComponent(id)}/risk`, { long, short }),
  setSide: (id: string, side: "long" | "short", enabled: boolean) =>
    request<{ status: "updated" }>("PUT", `/bots/${encodeURIComponent(id)}/sides`, { side, enabled }),
  setRuntime: (id: string, runtime: "py" | "rs") =>
    request<{ status: "updated" | "unchanged" }>("PUT", `/bots/${encodeURIComponent(id)}/runtime`, {
      runtime,
    }),
  applyTemplate: (id: string, name: string) =>
    request<{ status: "applied" }>("POST", `/bots/${encodeURIComponent(id)}/template`, { name }),
  unstuck: (id: string) => request<never>("POST", `/bots/${encodeURIComponent(id)}/unstuck`),

  listTemplates: () => request<{ templates: string[] }>("GET", "/templates"),
  getTemplate: (name: string) =>
    request<TemplateDescription>("GET", `/templates/${encodeURIComponent(name)}`),
};

/** A 409 from start/stop that the server marks `retry: true`. */
export function isRetryConflict(e: unknown): boolean {
  return e instanceof ApiError && e.status === 409 && e.body.retry === true;
}

export function errorText(e: unknown): string {
  if (e instanceof ApiError) {
    if (e.status === 409 && typeof e.body.status === "string") {
      return `The bot is ${String(e.body.status).replace(/_/g, " ")} right now — try again in a moment.`;
    }
    return e.message;
  }
  return e instanceof Error ? e.message : String(e);
}
