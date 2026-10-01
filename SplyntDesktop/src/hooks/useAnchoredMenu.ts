import { useLayoutEffect, useState } from "react";
import type { CSSProperties, RefObject } from "react";

const EDGE = 8;

/// Opens a menu at the pointer and flips it up or left when it would run off
/// the window, the way a native context menu does. Measured before paint, so
/// it never flashes in the wrong place.
export function anchoredPosition(x: number, y: number, width: number, height: number, viewport = { width: window.innerWidth, height: window.innerHeight }) {
  const left = x + width > viewport.width - EDGE ? Math.max(EDGE, x - width) : x;
  const top = y + height > viewport.height - EDGE ? Math.max(EDGE, y - height) : y;
  return { left, top };
}

export function useAnchoredMenu(ref: RefObject<HTMLElement | null>, x: number, y: number): CSSProperties {
  const [style, setStyle] = useState<CSSProperties>({ left: x, top: y, visibility: "hidden" });
  useLayoutEffect(() => {
    const element = ref.current;
    if (!element) return;
    const { width, height } = element.getBoundingClientRect();
    setStyle(anchoredPosition(x, y, width, height));
  }, [ref, x, y]);
  return style;
}
