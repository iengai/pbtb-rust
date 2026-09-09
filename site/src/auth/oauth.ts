// OAuth 2.1 authorization-code + PKCE against the AuthKit issuer, as a public
// client. The token is requested with `resource=<API URL>` so its `aud` is the
// API and the server's audience check passes; the SDK's own flow mints a token
// for a different audience, which is why this is hand-rolled.

import { challengeFor, randomToken } from "./pkce";

export const ISSUER = String(import.meta.env.VITE_OAUTH_ISSUER ?? "").replace(/\/$/, "");
export const CLIENT_ID = String(import.meta.env.VITE_OAUTH_CLIENT_ID ?? "");
// The Function URL with its trailing slash: the token's `aud` must equal it
// byte for byte, so it is never normalized here.
export const API_URL = String(import.meta.env.VITE_API_URL ?? "");
export const SCOPES = "openid bots:read bots:write";

export const REDIRECT_URI = `${window.location.origin}${import.meta.env.BASE_URL}callback`;

const FLOW_KEY = "pbtb.oauth.flow";
const TOKEN_KEY = "pbtb.token";
const REASON_KEY = "pbtb.login.reason";

export type Session = {
  access_token: string;
  id_token?: string;
  expires_at: number; // unix seconds
  claims: Claims;
};

export type Claims = {
  sub?: string;
  exp?: number;
  aud?: string | string[];
  scope?: string;
  email?: string;
};

export type LoginReason = "unlinked" | "expired" | "scope" | "signed_out";

export function configured(): boolean {
  return Boolean(ISSUER && CLIENT_ID && API_URL);
}

export async function beginLogin(): Promise<void> {
  const state = randomToken(16);
  const verifier = randomToken(48);
  sessionStorage.setItem(FLOW_KEY, JSON.stringify({ state, verifier }));
  const url = new URL(`${ISSUER}/oauth2/authorize`);
  url.searchParams.set("response_type", "code");
  url.searchParams.set("client_id", CLIENT_ID);
  url.searchParams.set("redirect_uri", REDIRECT_URI);
  url.searchParams.set("scope", SCOPES);
  url.searchParams.set("resource", API_URL);
  url.searchParams.set("state", state);
  url.searchParams.set("code_challenge", await challengeFor(verifier));
  url.searchParams.set("code_challenge_method", "S256");
  window.location.assign(url.toString());
}

// Exchange the code on the callback URL for a token. Throws with a message the
// callback page shows verbatim.
export async function completeLogin(search: string): Promise<Session> {
  const params = new URLSearchParams(search);
  const flowRaw = sessionStorage.getItem(FLOW_KEY);
  sessionStorage.removeItem(FLOW_KEY);
  if (params.get("error")) {
    throw new Error(params.get("error_description") || params.get("error") || "login refused");
  }
  const code = params.get("code");
  if (!code) throw new Error("the callback carried no authorization code");
  const flow = flowRaw ? (JSON.parse(flowRaw) as { state: string; verifier: string }) : null;
  if (!flow || params.get("state") !== flow.state) {
    throw new Error("login state mismatch — start the sign-in again from this tab");
  }
  const body = new URLSearchParams({
    grant_type: "authorization_code",
    code,
    redirect_uri: REDIRECT_URI,
    client_id: CLIENT_ID,
    code_verifier: flow.verifier,
    resource: API_URL,
  });
  const r = await fetch(`${ISSUER}/oauth2/token`, {
    method: "POST",
    headers: { "Content-Type": "application/x-www-form-urlencoded" },
    body,
  });
  const json = (await r.json().catch(() => ({}))) as {
    access_token?: string;
    id_token?: string;
    expires_in?: number;
    error?: string;
    error_description?: string;
  };
  if (!r.ok || !json.access_token) {
    throw new Error(json.error_description || json.error || `token endpoint answered HTTP ${r.status}`);
  }
  const claims = decodeClaims(json.access_token);
  const idClaims = json.id_token ? decodeClaims(json.id_token) : {};
  const expires_at =
    claims.exp ?? Math.floor(Date.now() / 1000) + (json.expires_in ?? 3600);
  const session: Session = {
    access_token: json.access_token,
    id_token: json.id_token,
    expires_at,
    claims: { ...claims, email: claims.email ?? idClaims.email },
  };
  saveSession(session);
  return session;
}

export function decodeClaims(jwt: string): Claims {
  try {
    const payload = jwt.split(".")[1] ?? "";
    const json = atob(payload.replace(/-/g, "+").replace(/_/g, "/"));
    return JSON.parse(decodeURIComponent(escape(json))) as Claims;
  } catch {
    return {};
  }
}

export function loadSession(): Session | null {
  try {
    const raw = sessionStorage.getItem(TOKEN_KEY);
    if (!raw) return null;
    const s = JSON.parse(raw) as Session;
    if (!s.access_token || s.expires_at <= Math.floor(Date.now() / 1000)) {
      sessionStorage.removeItem(TOKEN_KEY);
      return null;
    }
    return s;
  } catch {
    return null;
  }
}

export function saveSession(s: Session): void {
  sessionStorage.setItem(TOKEN_KEY, JSON.stringify(s));
}

export function clearSession(reason?: LoginReason): void {
  sessionStorage.removeItem(TOKEN_KEY);
  if (reason) sessionStorage.setItem(REASON_KEY, reason);
}

export function takeLoginReason(): LoginReason | null {
  const r = sessionStorage.getItem(REASON_KEY) as LoginReason | null;
  sessionStorage.removeItem(REASON_KEY);
  return r;
}
