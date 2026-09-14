import { useEffect, useLayoutEffect, useState } from "react";
import { DEFAULT_WIDTH, NARROW_QUERY } from "./frame";

// The width of the element the returned callback ref is attached to, kept
// current as it resizes, and whether the viewport is at the phone breakpoint.
// A callback ref, because a chart's box mounts only once its data has arrived;
// measured before paint, so the first frame a reader sees is already drawn at
// the real width.
export function useWidth(): [(el: Element | null) => void, number, boolean] {
  const [el, setEl] = useState<Element | null>(null);
  const [width, setWidth] = useState(DEFAULT_WIDTH);
  const [narrow, setNarrow] = useState(() => window.matchMedia(NARROW_QUERY).matches);
  useLayoutEffect(() => {
    if (!el) return;
    const initial = el.getBoundingClientRect().width;
    if (initial) setWidth(Math.round(initial));
    const ro = new ResizeObserver((entries) => {
      const w = entries[0]?.contentRect.width;
      if (w) setWidth(Math.round(w));
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, [el]);
  useEffect(() => {
    const mq = window.matchMedia(NARROW_QUERY);
    const on = () => setNarrow(mq.matches);
    mq.addEventListener("change", on);
    return () => mq.removeEventListener("change", on);
  }, []);
  return [setEl, width, narrow];
}
