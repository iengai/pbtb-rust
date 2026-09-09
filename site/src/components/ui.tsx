import { type ReactNode, useEffect } from "react";
import { Link as RouterLink } from "react-router-dom";
import { ApiError, AuthRefused, errorText } from "../api/client";
import type { Phase } from "../api/types";
import { sparklinePoints } from "../chart/returnCurve";
import { Alert, Chevron } from "./icons";

export function PhasePill({ phase, enabled }: { phase: Phase | null; enabled?: boolean }) {
  const tone =
    phase === "running" ? "ok" : phase === "starting" || phase === "stopping" ? "warn" : "bad";
  const label = phase ? phase[0]!.toUpperCase() + phase.slice(1) : enabled ? "Unknown" : "Stopped";
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

// The API's `{error}` text verbatim. A retryable fault (503) offers a retry.
export function ErrorBanner({
  error,
  onRetry,
  onDismiss,
}: {
  error: unknown;
  onRetry?: () => void;
  onDismiss?: () => void;
}) {
  if (!error) return null;
  // Refusals route to the login page; a banner would flash beneath it.
  if (error instanceof AuthRefused) return null;
  const retryable = error instanceof ApiError && error.retryable;
  return (
    <div className="banner error" role="alert">
      <div className="ico">
        <Alert />
      </div>
      <div className="body">{errorText(error)}</div>
      {retryable && onRetry && (
        <button type="button" className="btn act" onClick={onRetry} style={{ height: 30 }}>
          Retry
        </button>
      )}
      {!retryable && onDismiss && (
        <button type="button" className="btn ghost act" onClick={onDismiss} style={{ height: 30 }}>
          Dismiss
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

export function Loading({ what }: { what?: string }) {
  return <div className="msg">Loading{what ? ` ${what}` : ""}…</div>;
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

export function relativeTime(sec: number | null | undefined): string {
  if (!sec) return "—";
  const d = Math.max(0, Math.floor(Date.now() / 1000) - sec);
  if (d < 60) return "just now";
  if (d < 3600) return `${Math.floor(d / 60)} min ago`;
  if (d < 86400) return `${Math.floor(d / 3600)} h ago`;
  const days = Math.floor(d / 86400);
  return `${days} day${days === 1 ? "" : "s"} ago`;
}

export function runtimeLabel(rt: string): string {
  return rt === "rs" ? "rs · pb-runner" : "py · passivbot";
}

// A template's engine line, from the version the API/backtest reports.
export function engineLabel(version: string | null | undefined): string {
  if (!version) return "—";
  return version.startsWith("v") ? version : `v${version}`;
}
