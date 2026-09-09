import { useT } from "../i18n/locale";
import { type EquityPoint, equitySVG } from "./equitySvg";

export function EquityChart({ points }: { points: EquityPoint[] }) {
  const t = useT();
  const svg = equitySVG(points, t.returns.equity.aria);
  if (!svg) return <div className="msg">{t.returns.equity.notEnough}</div>;
  return (
    <>
      <div className="chart" dangerouslySetInnerHTML={{ __html: svg }} />
      <div className="legend">
        <span>
          <i className="swatch" style={{ background: "var(--pnl)" }} /> {t.returns.equity.equity}
        </span>
        <span>
          <i className="swatch" style={{ background: "var(--balance)" }} /> {t.returns.equity.balance}
        </span>
      </div>
    </>
  );
}
