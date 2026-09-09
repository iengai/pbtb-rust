import { type EquityPoint, equitySVG } from "./equitySvg";

export function EquityChart({ points }: { points: EquityPoint[] }) {
  const svg = equitySVG(points);
  if (!svg) return <div className="msg">Not enough backtest points to plot.</div>;
  return (
    <>
      <div className="chart" dangerouslySetInnerHTML={{ __html: svg }} />
      <div className="legend">
        <span>
          <i className="swatch" style={{ background: "var(--pnl)" }} /> Equity
        </span>
        <span>
          <i className="swatch" style={{ background: "var(--balance)" }} /> Balance
        </span>
      </div>
    </>
  );
}
