import { useEffect, useRef } from "react";
import { type ChartWindow, chartSVG, RANGES, wireHover } from "./returnCurve";

export function RangeSelector({ value, onChange }: { value: number; onChange: (i: number) => void }) {
  return (
    <div className="ranges" role="tablist">
      {RANGES.map((r, i) => (
        <button
          key={r.k}
          type="button"
          role="tab"
          aria-selected={i === value}
          className={`range${i === value ? " on" : ""}`}
          onClick={() => onChange(i)}
        >
          {r.k}
        </button>
      ))}
    </div>
  );
}

// The cumulative-return chart for one selected window. The SVG is produced as
// markup by `chartSVG` and the hover is wired imperatively, so the drawing code
// stays the dependency-free string builder the return-curve site was written as.
export function ReturnChart({ window: win }: { window: ChartWindow }) {
  const box = useRef<HTMLDivElement>(null);
  const tip = useRef<HTMLDivElement>(null);
  const svg = win.kind === "ok" ? chartSVG(win.view, win.switches) : "";

  useEffect(() => {
    if (win.kind !== "ok" || !box.current || !tip.current) return;
    return wireHover(box.current, tip.current, win.view);
  }, [win, svg]);

  if (win.kind === "empty") {
    return (
      <div className="msg">
        {win.message}
        {win.hint && (
          <>
            <br />
            <span className="hint">{win.hint}</span>
          </>
        )}
      </div>
    );
  }
  return (
    <>
      <div ref={box} className="chart" dangerouslySetInnerHTML={{ __html: svg }} />
      <div ref={tip} className="tip" />
    </>
  );
}
