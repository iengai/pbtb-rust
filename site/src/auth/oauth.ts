// OAuth 2.1 authorization-code + PKCE against the AuthKit issuer, as a public
// client. The token is requested with `resource=<API URL>` so its `aud` is the
// API and the server's audience check passes; the SDK's own flow mints a token
// for a different audience, which is why this is hand-rolled.
//
// The issuer's access tokens last five minutes, so `offline_access` is asked
// for and `accessToken()` renews the session from the refresh token; nothing
// else in the app is aware that the token underneath it changes.

import { challengeFor, randomToken } from "./pkce";

export const ISSUER = String(import.meta.env.VITE_OAUTH_ISSUER ?? "").replace(/\/$/, "");
export const CLIENT_ID = String(import.meta.env.VITE_OAUTH_CLIENT_ID ?? "");
// The Function URL with its trailing slash: the token's `aud` must equal it
// byte for byte, so it is never normalized here.
export const API_URL = String(import.meta.env.VITE_API_URL ?? "");
export const SCOPES = "openid offline_access bots:read bots:write";

export const REDIRECT_URI = `${window.location.origin}${import.meta.env.BASE_URL}callback`;

const FLOW_KEY = "pbtb.oauth.flow";
const TOKEN_KEY = "pbtb.token";
const REASON_KEY = "pbtb.login.reason";

export type Session = {
  access_token: string;
  id_token?: string;
  refresh_token?: string;
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
  const json = await postToken(
    new URLSearchParams({
      grant_type: "authorization_code",
      code,
      redirect_uri: REDIRECT_URI,
      client_id: CLIENT_ID,
      code_verifier: flow.verifier,
      resource: API_URL,
    }),
  );
  const session = sessionFrom(json);
  saveSession(session);
  return session;
}

type TokenResponse = {
  access_token?: string;
  id_token?: string;
  refresh_token?: string;
  expires_in?: number;
  error?: string;
  error_description?: string;
};

async function postToken(body: URLSearchParams): Promise<TokenResponse & { access_token: string }> {
  const r = await fetch(`${ISSUER}/oauth2/token`, {
    method: "POST",
    headers: { "Content-Type": "application/x-www-form-urlencoded" },
    body,
  });
  const json = (await r.json().catch(() => ({}))) as TokenResponse;
  if (!r.ok || !json.access_token) {
    throw new Error(json.error_description || json.error || `token endpoint answered HTTP ${r.status}`);
  }
  return { ...json, access_token: json.access_token };
}

// A renewal answers with a fresh access token and, because WorkOS rotates
// them, a fresh refresh token; anything it leaves out is carried over from the
// session being replaced, so the header keeps showing the email the id token
// brought at sign-in.
function sessionFrom(
  json: TokenResponse & { access_token: string },
  previous?: Session,
): Session {
  const claims = decodeClaims(json.access_token);
  const idClaims = json.id_token ? decodeClaims(json.id_token) : {};
  return {
    access_token: json.access_token,
    id_token: json.id_token ?? previous?.id_token,
    refresh_token: json.refresh_token ?? previous?.refresh_token,
    expires_at: claims.exp ?? Math.floor(Date.now() / 1000) + (json.expires_in ?? 3600),
    claims: { ...claims, email: claims.email ?? idClaims.email ?? previous?.claims.email },
  };
}

// Seconds of headroom: a token this close to its expiry is renewed rather than
// handed out, so a request never races the clock.
const SKEW = 60;
let renewal: Promise<Session | null> | null = null;

/**
 * The token to send with an API call, renewed first if it is spent. `null`
 * means there is no session left — the caller shows the login page.
 */
export async function accessToken(): Promise<string | null> {
  const s = loadSession();
  if (!s) return null;
  if (s.expires_at - SKEW > Math.floor(Date.now() / 1000)) return s.access_token;
  // One exchange at a time: a rotated refresh token used twice is spent, and
  // several requests can find the token expired in the same tick.
  renewal ??= renew(s).finally(() => {
    renewal = null;
  });
  return (await renewal)?.access_token ?? null;
}

async function renew(s: Session): Promise<Session | null> {
  if (!s.refresh_token) {
    clearSession();
    return null;
  }
  try {
    const next = sessionFrom(
      await postToken(
        new URLSearchParams({
          grant_type: "refresh_token",
          refresh_token: s.refresh_token,
          client_id: CLIENT_ID,
          resource: API_URL,
        }),
      ),
      s,
    );
    saveSession(next);
    return next;
  } catch {
    // The refresh token is spent or revoked; there is nothing to fall back on.
    clearSession();
    return null;
  }
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
    // An expired access token is not a dead session while a refresh token is
    // there to renew it.
    const spent = s.expires_at <= Math.floor(Date.now() / 1000) && !s.refresh_token;
    if (!s.access_token || spent) {
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
