"use strict";

// Renders the per-bot return series written by the daily_pnl_snapshot Lambda.
// Data lives under ./data/: index.json (list of bot ids) + <bot_id>.json
// (a BotReturnSeries). No build step, no external dependencies.

const DATA = "./data";
const $ = (id) => document.getElementById(id);

const fmt = (v) =>
  (v ?? 0).toLocaleString(undefined, { maximumFractionDigits: 2 });
const fmtDate = (sec) => new Date(sec * 1000).toISOString().slice(0, 10);

async function getJSON(url) {
  const r = await fetch(url, { cache: "no-cache" });
  if (!r.ok) throw new Error(`${url} -> HTTP ${r.status}`);
  return r.json();
}

async function boot() {
  let ids;
  try {
    const idx = await getJSON(`${DATA}/index.json`);
    ids = Array.isArray(idx) ? idx : idx.bots || [];
  } catch (e) {
    $("chart").innerHTML = `<div class="msg">No data yet. (${e.message})</div>`;
    return;
  }
  if (!ids.length) {
    $("chart").innerHTML = `<div class="msg">No bots have data yet.</div>`;
    return;
  }

  const sel = $("bot");
  sel.innerHTML = ids
    .map((id) => `<option value="${id}">${id}</option>`)
    .join("");
  sel.onchange = () => load(sel.value);
  load(ids[0]);
}

async function load(botId) {
  $("chart").innerHTML = `<div class="msg">Loading ${botId}…</div>`;
  let series;
  try {
    series = await getJSON(`${DATA}/${botId}.json`);
  } catch (e) {
    $("chart").innerHTML = `<div class="msg">Failed to load ${botId}: ${e.message}</div>`;
    return;
  }
  render(series);
}

function render(s) {
  const pts = (s.points || []).slice().sort((a, b) => a.ts - b.ts);
  const last = pts[pts.length - 1] || {};

  // Headline stats.
  const cumPnl = last.cumulative_pnl ?? 0;
  const stats = [
    ["Current equity", fmt(s.current_equity)],
    ["Cumulative PnL", fmt(cumPnl)],
    ["Wallet balance", fmt(last.balance)],
    ["Net deposits", fmt(last.cumulative_net_deposit)],
  ];
  const statsEl = $("stats");
  statsEl.hidden = false;
  statsEl.innerHTML = stats
    .map(
      ([k, v]) =>
        `<div class="stat"><div class="k">${k}</div><div class="v">${v}</div></div>`
    )
    .join("");

  $("footer").innerHTML = s.generated_at
    ? `${s.exchange || "bybit"} · ${pts.length} days · updated ${fmtDate(s.generated_at)} UTC`
    : "";

  if (pts.length < 2) {
    $("chart").innerHTML = `<div class="msg">Not enough data to plot yet.</div>`;
    return;
  }
  $("chart").innerHTML = chartSVG(pts, s.config_switches || []);
  wireHover(pts);
}

// --- SVG line chart (cumulative PnL + balance, config-switch markers) ---

const W = 880,
  H = 360,
  M = { l: 56, r: 16, t: 18, b: 30 };
const PW = W - M.l - M.r,
  PH = H - M.t - M.b;

function scales(pts) {
  const t0 = pts[0].ts,
    t1 = pts[pts.length - 1].ts;
  let vmin = Infinity,
    vmax = -Infinity;
  for (const p of pts) {
    for (const v of [p.cumulative_pnl, p.balance]) {
      if (v < vmin) vmin = v;
      if (v > vmax) vmax = v;
    }
  }
  if (vmin === vmax) {
    vmin -= 1;
    vmax += 1;
  }
  const pad = (vmax - vmin) * 0.08;
  vmin -= pad;
  vmax += pad;
  const x = (t) => M.l + ((t - t0) / (t1 - t0 || 1)) * PW;
  const y = (v) => M.t + (1 - (v - vmin) / (vmax - vmin)) * PH;
  return { x, y, t0, t1, vmin, vmax };
}

function path(pts, sc, key) {
  return pts
    .map((p, i) => `${i ? "L" : "M"}${sc.x(p.ts).toFixed(1)} ${sc.y(p[key]).toFixed(1)}`)
    .join(" ");
}

function chartSVG(pts, switches) {
  const sc = scales(pts);

  // Horizontal grid + y labels.
  let grid = "";
  const ROWS = 5;
  for (let i = 0; i <= ROWS; i++) {
    const v = sc.vmin + ((sc.vmax - sc.vmin) * i) / ROWS;
    const y = sc.y(v).toFixed(1);
    grid += `<line x1="${M.l}" y1="${y}" x2="${W - M.r}" y2="${y}" stroke="var(--grid)"/>`;
    grid += `<text x="${M.l - 8}" y="${y}" text-anchor="end" dominant-baseline="middle" fill="var(--muted)" font-size="11">${fmt(v)}</text>`;
  }

  // X labels (a handful of evenly-spaced dates).
  let xlab = "";
  const COLS = 5;
  for (let i = 0; i <= COLS; i++) {
    const t = sc.t0 + ((sc.t1 - sc.t0) * i) / COLS;
    const x = sc.x(t).toFixed(1);
    xlab += `<text x="${x}" y="${H - 10}" text-anchor="middle" fill="var(--muted)" font-size="11">${fmtDate(t)}</text>`;
  }

  // Config-switch vertical markers.
  let sw = "";
  for (const c of switches) {
    if (c.ts < sc.t0 || c.ts > sc.t1) continue;
    const x = sc.x(c.ts).toFixed(1);
    sw += `<line x1="${x}" y1="${M.t}" x2="${x}" y2="${M.t + PH}" stroke="var(--switch)" stroke-width="1.5" stroke-dasharray="4 3" opacity="0.8"/>`;
    sw += `<text x="${x}" y="${M.t - 5}" text-anchor="middle" fill="var(--switch)" font-size="10">${escapeXml(c.template_name)}</text>`;
  }

  const balPath = `<path d="${path(pts, sc, "balance")}" fill="none" stroke="var(--balance)" stroke-width="1.5" opacity="0.7"/>`;
  const pnlPath = `<path d="${path(pts, sc, "cumulative_pnl")}" fill="none" stroke="var(--pnl)" stroke-width="2"/>`;

  return `<svg viewBox="0 0 ${W} ${H}" role="img" aria-label="return curve">
    ${grid}${xlab}${sw}${balPath}${pnlPath}
    <line id="cursor" x1="0" y1="${M.t}" x2="0" y2="${M.t + PH}" stroke="var(--accent)" stroke-width="1" opacity="0"/>
    <circle id="dotP" r="3.5" fill="var(--pnl)" opacity="0"/>
    <circle id="dotB" r="3" fill="var(--balance)" opacity="0"/>
    <rect id="hit" x="${M.l}" y="${M.t}" width="${PW}" height="${PH}" fill="transparent"/>
  </svg>`;
}

function wireHover(pts) {
  const svg = $("chart").querySelector("svg");
  const hit = $("hit"),
    cursor = $("cursor"),
    dotP = $("dotP"),
    dotB = $("dotB"),
    tip = $("tip");
  const sc = scales(pts);

  const show = (on) => {
    for (const el of [cursor, dotP, dotB]) el.setAttribute("opacity", on ? "1" : "0");
    tip.style.opacity = on ? "1" : "0";
  };

  hit.addEventListener("mousemove", (e) => {
    const rect = svg.getBoundingClientRect();
    const vx = ((e.clientX - rect.left) / rect.width) * W;
    // Nearest point by x.
    let best = 0,
      bd = Infinity;
    for (let i = 0; i < pts.length; i++) {
      const d = Math.abs(sc.x(pts[i].ts) - vx);
      if (d < bd) {
        bd = d;
        best = i;
      }
    }
    const p = pts[best];
    const px = sc.x(p.ts);
    cursor.setAttribute("x1", px);
    cursor.setAttribute("x2", px);
    dotP.setAttribute("cx", px);
    dotP.setAttribute("cy", sc.y(p.cumulative_pnl));
    dotB.setAttribute("cx", px);
    dotB.setAttribute("cy", sc.y(p.balance));
    show(true);
    tip.innerHTML =
      `<div class="d">${fmtDate(p.ts)}</div>` +
      `<div class="row"><span>PnL</span><b>${fmt(p.cumulative_pnl)}</b></div>` +
      `<div class="row"><span>Balance</span><b>${fmt(p.balance)}</b></div>`;
    tip.style.left = Math.min(e.clientX + 14, window.innerWidth - 160) + "px";
    tip.style.top = e.clientY + 14 + "px";
  });
  hit.addEventListener("mouseleave", () => show(false));
}

function escapeXml(s) {
  return String(s).replace(/[<>&]/g, (c) => ({ "<": "&lt;", ">": "&gt;", "&": "&amp;" }[c]));
}

boot();
