import { useEffect, useState } from "react";

export type TooltipState = { text: string; left: number; top: number; below: boolean };

const TARGETS = "button[title], [data-tooltip]";
const DELAY_MS = 400;

/// Spotify-style tooltips for every titled button. The native `title` is moved
/// to `data-tooltip` on first hover so the browser's own tooltip never shows
/// alongside ours; React puts `title` back whenever the label changes, and the
/// next hover moves it again.
export function useTooltips() {
  const [tip, setTip] = useState<TooltipState>();
  useEffect(() => {
    let timer: number | undefined;
    let current: HTMLElement | null = null;
    const hide = () => {
      window.clearTimeout(timer);
      current = null;
      setTip(undefined);
    };
    const over = (event: PointerEvent) => {
      const element = event.target instanceof Element ? event.target.closest<HTMLElement>(TARGETS) : null;
      if (element === current) return;
      hide();
      if (!element) return;
      const title = element.getAttribute("title");
      if (title) {
        element.dataset.tooltip = title;
        element.removeAttribute("title");
      }
      const text = element.dataset.tooltip;
      if (!text) return;
      current = element;
      timer = window.setTimeout(() => {
        if (current !== element || !element.isConnected) return;
        const rect = element.getBoundingClientRect();
        const below = rect.top < 64;
        setTip({ text: element.dataset.tooltip ?? text, left: rect.left + rect.width / 2, top: below ? rect.bottom + 8 : rect.top - 8, below });
      }, DELAY_MS);
    };
    document.addEventListener("pointerover", over);
    document.addEventListener("pointerdown", hide, true);
    document.addEventListener("keydown", hide, true);
    window.addEventListener("scroll", hide, true);
    window.addEventListener("blur", hide);
    return () => {
      window.clearTimeout(timer);
      document.removeEventListener("pointerover", over);
      document.removeEventListener("pointerdown", hide, true);
      document.removeEventListener("keydown", hide, true);
      window.removeEventListener("scroll", hide, true);
      window.removeEventListener("blur", hide);
    };
  }, []);
  return tip;
}
