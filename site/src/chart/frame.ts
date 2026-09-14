// A chart's drawing box. The SVG is drawn at the width its container has, one
// viewBox unit to a CSS pixel, so axis text and hit areas keep their size on a
// phone instead of shrinking with a fixed-width drawing scaled down to fit.
export type Margins = { l: number; r: number; t: number; b: number };
export type Frame = { W: number; H: number; M: Margins; PW: number; PH: number; cols: number };

// The width a chart is drawn at until its container has been measured.
export const DEFAULT_WIDTH = 900;
const MIN_WIDTH = 280;

// The stylesheet's phone breakpoint. A chart's height follows it rather than
// the measured width: the viewport's width is the same with or without a
// scrollbar, so a taller chart cannot bring in a scrollbar that narrows it back
// into the shorter height.
export const NARROW_QUERY = "(max-width: 760px)";

// On a phone a chart is taller for its width, so the curve is not flattened
// into a strip; a narrow chart has fewer date ticks, so the labels do not run
// into each other.
export function frameOf(width: number, M: Margins, narrow: boolean): Frame {
  const W = Math.max(MIN_WIDTH, Math.round(width));
  const H = narrow ? 240 : 300;
  const cols = W >= 640 ? 5 : W >= 420 ? 4 : 3;
  return { W, H, M, PW: W - M.l - M.r, PH: H - M.t - M.b, cols };
}
