"use strict";

// Renders the per-bot return series written by the daily_pnl_snapshot Lambda.
// Data lives under ./data/: index.json (list of bot ids) + <bot_id>.json (a
// BotReturnSeries). The series is normalized — a time-weighted return index and
// cumulative return %, no absolute balances. No build step, no dependencies.

const DATA = "./data";
const $ = (id) => document.getElementById(id);

const fmtPct = (v) => `${(v ?? 0).toFixed(2)}%`;
const fmtDate = (sec) => new Date(sec * 1000).toISOString().slice(0, 10);

// Preset look-back windows, stock-chart style. `days: null` means all history.
const RANGES = [
  { k: "30D", days: 30 },
  { k: "90D", days: 90 },
];
let RANGE_I = 1; // default: 90D
let CURRENT = null; // the loaded BotReturnSeries

async function getJSON(url) {
  const r = await fetch(url, { cache: "no-cache" });
  if (!r.ok) throw new Error(`${url} -> HTTP ${r.status}`);
  return r.json();
}

async function boot() {
  let bots;
  try {
    const idx = await getJSON(`${DATA}/index.json`);
    bots = normalizeIndex(idx);
  } catch (e) {
    $("chart").innerHTML = `<div class="msg">No data yet. (${e.message})</div>`;
    return;
  }
  if (!bots.length) {
    $("chart").innerHTML = `<div class="msg">No bots have data yet.</div>`;
    return;
  }

  // The option value is the stable id (the fetch key); the visible text is the
  // current display name. A rename only changes the text, never the id.
  const sel = $("bot");
  sel.innerHTML = bots
    .map((b) => `<option value="${b.id}">${escapeXml(b.name)}</option>`)
    .join("");
  sel.onchange = () => load(sel.value);
  buildRanges();
  load(bots[0].id);
}

// index.json is a list of { id, name }. Tolerate a legacy bare-string array (id
// used as its own name) so an old published index still renders.
function normalizeIndex(idx) {
  const arr = Array.isArray(idx) ? idx : idx.bots || [];
  return arr
    .map((e) => (typeof e === "string" ? { id: e, name: e } : e))
    .filter((e) => e && e.id);
}

// The look-back selector. Picking a window re-draws the same loaded series; it
// never refetches.
function buildRanges() {
  const el = $("ranges");
  el.innerHTML = RANGES.map(
    (r, i) => `<button type="button" data-i="${i}" class="range${i === RANGE_I ? " on" : ""}">${r.k}</button>`,
  ).join("");
  el.onclick = (e) => {
    const b = e.target.closest("button[data-i]");
    if (!b) return;
    RANGE_I = +b.dataset.i;
    for (const x of el.querySelectorAll("button")) x.classList.toggle("on", x === b);
    if (CURRENT) draw();
  };
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
  CURRENT = s;
  draw();
}

// Draw CURRENT for the selected look-back window. The window is re-based to its
// first point (0% at the window start), so every preset reads as "return over
// this period" — the way a stock chart shows 1M / 3M / 1Y.
function draw() {
  const s = CURRENT;
  const all = (s.points || []).slice().sort((a, b) => a.ts - b.ts);
  const statsEl = $("stats");

  if (all.length < 2) {
    statsEl.hidden = true;
    $("footer").innerHTML = "";
    $("chart").innerHTML = `<div class="msg">Not enough data to plot yet.</div>`;
    return;
  }

  const range = RANGES[RANGE_I];
  const lastTs = all[all.length - 1].ts;
  let win = all;
  if (range.days != null) {
    const cutoff = lastTs - range.days * 86400;
    win = all.filter((p) => p.ts >= cutoff);
    if (win.length < 2) win = all.slice(-2); // window shorter than history: show what we have
  }

  // Re-base the cumulative index to the window start.
  const base = win[0].index || 100;
  const view = win.map((p) => ({ ts: p.ts, return_pct: (p.index / base - 1) * 100 }));

  const last = view[view.length - 1];
  const peak = view.reduce((m, p) => Math.max(m, p.return_pct), -Infinity);
  const label = range.days == null ? "Total" : range.k;
  const stats = [
    [`${label} return`, fmtPct(last.return_pct)],
    ["Peak", fmtPct(peak)],
    ["Days", String(view.length)],
  ];
  statsEl.hidden = false;
  statsEl.innerHTML = stats
    .map(([k, v]) => `<div class="stat"><div class="k">${k}</div><div class="v">${v}</div></div>`)
    .join("");

  $("footer").innerHTML = s.generated_at
    ? `${s.exchange || "bybit"} · ${view.length} days shown · time-weighted, deposit-adjusted · updated ${fmtDate(s.generated_at)} UTC`
    : "";

  const switches = (s.config_switches || []).filter(
    (c) => c.ts >= view[0].ts && c.ts <= last.ts,
  );
  $("chart").innerHTML = chartSVG(view, switches);
  wireHover(view);
}

// --- SVG line chart (cumulative return %, config-switch markers) ---

const W = 880,
  H = 360,
  M = { l: 56, r: 16, t: 18, b: 30 };
const PW = W - M.l - M.r,
  PH = H - M.t - M.b;

function scales(pts) {
  const t0 = pts[0].ts,
    t1 = pts[pts.length - 1].ts;
  let vmin = 0,
    vmax = 0; // always include the 0% baseline
  for (const p of pts) {
    if (p.return_pct < vmin) vmin = p.return_pct;
    if (p.return_pct > vmax) vmax = p.return_pct;
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

function linePath(pts, sc) {
  return pts
    .map((p, i) => `${i ? "L" : "M"}${sc.x(p.ts).toFixed(1)} ${sc.y(p.return_pct).toFixed(1)}`)
    .join(" ");
}

// Return % at an arbitrary ts, linearly interpolated between daily points, so a
// config-switch marker sits exactly on the curve.
function returnAt(ts, pts) {
  const n = pts.length;
  if (ts <= pts[0].ts) return pts[0].return_pct;
  if (ts >= pts[n - 1].ts) return pts[n - 1].return_pct;
  for (let i = 1; i < n; i++) {
    if (pts[i].ts >= ts) {
      const a = pts[i - 1],
        b = pts[i];
      const f = (ts - a.ts) / (b.ts - a.ts || 1);
      return a.return_pct + f * (b.return_pct - a.return_pct);
    }
  }
  return pts[n - 1].return_pct;
}

function chartSVG(pts, switches) {
  const sc = scales(pts);
  const up = pts[pts.length - 1].return_pct >= 0;

  // Horizontal grid + y labels (percent).
  let grid = "";
  const ROWS = 5;
  for (let i = 0; i <= ROWS; i++) {
    const v = sc.vmin + ((sc.vmax - sc.vmin) * i) / ROWS;
    const y = sc.y(v).toFixed(1);
    grid += `<line x1="${M.l}" y1="${y}" x2="${W - M.r}" y2="${y}" stroke="var(--grid)"/>`;
    grid += `<text x="${M.l - 8}" y="${y}" text-anchor="end" dominant-baseline="middle" fill="var(--muted)" font-size="11">${v.toFixed(1)}%</text>`;
  }

  // Emphasized 0% baseline.
  const zeroY = sc.y(0).toFixed(1);
  const baseline = `<line x1="${M.l}" y1="${zeroY}" x2="${W - M.r}" y2="${zeroY}" stroke="var(--muted)" stroke-width="1" opacity="0.5"/>`;

  // X labels.
  let xlab = "";
  const COLS = 5;
  for (let i = 0; i <= COLS; i++) {
    const t = sc.t0 + ((sc.t1 - sc.t0) * i) / COLS;
    xlab += `<text x="${sc.x(t).toFixed(1)}" y="${H - 10}" text-anchor="middle" fill="var(--muted)" font-size="11">${fmtDate(t)}</text>`;
  }

  // Config-switch markers: a dot sitting ON the return curve at each switch,
  // with a native hover tooltip naming the config it switched to.
  let sw = "";
  for (const c of switches) {
    if (c.ts < sc.t0 || c.ts > sc.t1) continue;
    const x = sc.x(c.ts).toFixed(1);
    const y = sc.y(returnAt(c.ts, pts)).toFixed(1);
    sw += `<circle cx="${x}" cy="${y}" r="5" fill="var(--switch)" stroke="var(--panel)" stroke-width="2"><title>→ ${escapeXml(c.template_name)} · ${fmtDate(c.ts)}</title></circle>`;
  }

  const color = up ? "var(--pnl)" : "var(--pnl-neg)";
  const line = `<path d="${linePath(pts, sc)}" fill="none" stroke="${color}" stroke-width="2"/>`;

  return `<svg viewBox="0 0 ${W} ${H}" role="img" aria-label="return curve">
    ${grid}${baseline}${xlab}${sw}${line}
    <line id="cursor" x1="0" y1="${M.t}" x2="0" y2="${M.t + PH}" stroke="var(--accent)" stroke-width="1" opacity="0"/>
    <circle id="dot" r="3.5" fill="${color}" opacity="0"/>
    <rect id="hit" x="${M.l}" y="${M.t}" width="${PW}" height="${PH}" fill="transparent"/>
  </svg>`;
}

function wireHover(pts) {
  const svg = $("chart").querySelector("svg");
  const hit = $("hit"),
    cursor = $("cursor"),
    dot = $("dot"),
    tip = $("tip");
  const sc = scales(pts);

  const show = (on) => {
    for (const el of [cursor, dot]) el.setAttribute("opacity", on ? "1" : "0");
    tip.style.opacity = on ? "1" : "0";
  };

  hit.addEventListener("mousemove", (e) => {
    const rect = svg.getBoundingClientRect();
    const vx = ((e.clientX - rect.left) / rect.width) * W;
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
    dot.setAttribute("cx", px);
    dot.setAttribute("cy", sc.y(p.return_pct));
    show(true);
    tip.innerHTML =
      `<div class="d">${fmtDate(p.ts)}</div>` +
      `<div class="row"><span>Return</span><b>${fmtPct(p.return_pct)}</b></div>`;
    tip.style.left = Math.min(e.clientX + 14, window.innerWidth - 150) + "px";
    tip.style.top = e.clientY + 14 + "px";
  });
  hit.addEventListener("mouseleave", () => show(false));
}

function escapeXml(s) {
  return String(s).replace(/[<>&]/g, (c) => ({ "<": "&lt;", ">": "&gt;", "&": "&amp;" }[c]));
}

boot();
