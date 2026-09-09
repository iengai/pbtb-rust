import { type ReactNode, useEffect } from "react";
import { Link as RouterLink } from "react-router-dom";
import { ApiError, AuthRefused, errorText } from "../api/client";
import type { Phase } from "../api/types";
import { sparklinePoints } from "../chart/returnCurve";
import { type Lang, useT } from "../i18n/locale";
import type { Messages } from "../i18n/messages";
import { Alert, Chevron } from "./icons";

export function PhasePill({ phase, enabled }: { phase: Phase | null; enabled?: boolean }) {
  const t = useT();
  const tone =
    phase === "running" ? "ok" : phase === "starting" || phase === "stopping" ? "warn" : "bad";
  const label = phase
    ? t.common.phase[phase]
    : enabled
      ? t.common.phase.unknown
      : t.common.phase.stopped;
  return (
    <span className={`pill ${phase ? tone : "bad"}`}>
      <i />
      {label}
    </span>
  );
}

export function Pill({ tone, children }: { tone?: "ok" | "warn" | "bad"; children: ReactNode }) {
  return (
    <span className={`pill${tone ? ` ${tone}` : ""}`}>
      <i />
      {children}
    </span>
  );
}

export function Badge({ children }: { children: ReactNode }) {
  return <span className="badge">{children}</span>;
}

export function Chips({ items, max, tight }: { items: string[]; max?: number; tight?: boolean }) {
  const shown = max && items.length > max ? items.slice(0, max) : items;
  const rest = items.length - shown.length;
  return (
    <div className={`chips${tight ? " tight" : ""}`}>
      {shown.map((c) => (
        <Badge key={c}>{c}</Badge>
      ))}
      {rest > 0 && <Badge>+{rest}</Badge>}
    </div>
  );
}

export function Crumbs({ items }: { items: { to?: string; label: ReactNode }[] }) {
  return (
    <div className="crumbs">
      {items.map((it, i) => (
        <span key={i} style={{ display: "contents" }}>
          {i > 0 && <Chevron size={14} />}
          {it.to ? <RouterLink to={it.to}>{it.label}</RouterLink> : <span>{it.label}</span>}
        </span>
      ))}
    </div>
  );
}

export function Tile({ k, v, tone }: { k: string; v: ReactNode; tone?: "up" | "down" }) {
  const color = tone === "up" ? "var(--pnl)" : tone === "down" ? "var(--pnl-neg)" : undefined;
  return (
    <div className="tile">
      <div className="k">{k}</div>
      <div className="v" style={{ color }}>
        {v}
      </div>
    </div>
  );
}

export function Sparkline({
  pts,
  up,
  w = 88,
  h = 26,
}: {
  pts: { ts: number; v: number }[];
  up: boolean;
  w?: number;
  h?: number;
}) {
  const points = sparklinePoints(pts, w, h);
  if (!points) return <span className="muted">—</span>;
  return (
    <svg viewBox={`0 0 ${w} ${h}`} width={w} height={h} style={{ display: "block" }} aria-hidden="true">
      <polyline
        points={points}
        fill="none"
        stroke={up ? "var(--pnl)" : "var(--pnl-neg)"}
        strokeWidth="1.5"
        strokeLinejoin="round"
      />
    </svg>
  );
}

// The API's `{error}` text verbatim, except for the fetch failure the client
// makes itself (status 0) and a 409 naming the bot's current phase, both ours
// to translate. A retryable fault (503) offers a retry.
export function errorMessage(error: unknown, t: Messages): string {
  if (error instanceof ApiError && error.status === 0) return t.common.networkError;
  if (error instanceof ApiError && error.status === 403) {
    const b = error.body;
    if (b.error === "insufficient_level") return t.common.insufficientLevel(Number(b.required), Number(b.current));
    if (b.error === "quota_exceeded") return t.common.quotaExceeded(Number(b.limit));
  }
  if (error instanceof ApiError && error.status === 409 && typeof error.body.status === "string") {
    const status = error.body.status;
    const phase = (t.common.phase as Record<string, string>)[status] ?? status.replace(/_/g, " ");
    return t.common.busyConflict(phase);
  }
  return errorText(error);
}

export function ErrorBanner({
  error,
  onRetry,
  onDismiss,
}: {
  error: unknown;
  onRetry?: () => void;
  onDismiss?: () => void;
}) {
  const t = useT();
  if (!error) return null;
  // Refusals route to the login page; a banner would flash beneath it.
  if (error instanceof AuthRefused) return null;
  const retryable = error instanceof ApiError && error.retryable;
  return (
    <div className="banner error" role="alert">
      <div className="ico">
        <Alert />
      </div>
      <div className="body">{errorMessage(error, t)}</div>
      {retryable && onRetry && (
        <button type="button" className="btn act" onClick={onRetry} style={{ height: 30 }}>
          {t.common.retry}
        </button>
      )}
      {!retryable && onDismiss && (
        <button type="button" className="btn ghost act" onClick={onDismiss} style={{ height: 30 }}>
          {t.common.dismiss}
        </button>
      )}
    </div>
  );
}

export function Notice({ icon, children, tone }: { icon?: ReactNode; children: ReactNode; tone?: "info" }) {
  return (
    <div className={`banner${tone ? ` ${tone}` : ""}`}>
      {icon && <div className="ico">{icon}</div>}
      <div className="body">{children}</div>
    </div>
  );
}

// `what` is already in the reader's language: the caller takes it from its own
// area of the catalog.
export function Loading({ what }: { what?: string }) {
  const t = useT();
  return <div className="msg">{what ? t.common.loadingWhat(what) : t.common.loading}</div>;
}

export function Modal({
  title,
  onClose,
  children,
}: {
  title: string;
  onClose: () => void;
  children: ReactNode;
}) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [onClose]);
  return (
    <div className="overlay" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="modal" role="dialog" aria-modal="true" aria-label={title}>
        <h2>{title}</h2>
        {children}
      </div>
    </div>
  );
}

export function relativeTime(sec: number | null | undefined, lang: Lang): string {
  if (!sec) return "—";
  const d = Math.max(0, Math.floor(Date.now() / 1000) - sec);
  const rtf = new Intl.RelativeTimeFormat(lang === "zh" ? "zh-CN" : "en", { numeric: "auto" });
  if (d < 60) return rtf.format(-d, "second");
  if (d < 3600) return rtf.format(-Math.floor(d / 60), "minute");
  if (d < 86400) return rtf.format(-Math.floor(d / 3600), "hour");
  return rtf.format(-Math.floor(d / 86400), "day");
}

export function runtimeLabel(rt: string): string {
  return rt === "rs" ? "rs · pb-runner" : "py · passivbot";
}

// What to call a template on screen. A template is addressed by its id — the
// S3 key, in the URL, fixed for its life — and read by its title, which is
// per-language data on the template itself. One published before titles has
// only its id to go by.
export function templateTitle(
  tpl: { name?: string; title?: string | null; title_zh?: string | null },
  lang: Lang,
): string {
  const localized = lang === "zh" ? tpl.title_zh : tpl.title;
  return localized || tpl.title || tpl.name || "—";
}

// A template's engine line, from the version the API/backtest reports.
export function engineLabel(version: string | null | undefined): string {
  if (!version) return "—";
  return version.startsWith("v") ? version : `v${version}`;
}
