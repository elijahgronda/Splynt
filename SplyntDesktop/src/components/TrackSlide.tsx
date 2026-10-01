import type { ReactNode } from "react";
import { useEffect, useRef, useState } from "react";

type TrackSlideProps = {
  /// Changing this is what starts a transition. Use the track id, not the queue
  /// index — a shuffle reorder must not look like a skip.
  slideKey: string;
  /// 1 when the queue moved forward (the outgoing track leaves to the left),
  /// -1 when it moved back.
  direction: 1 | -1;
  className?: string;
  /// Full-bleed slide for surfaces that are the background themselves, such as
  /// the expanded player canvas. The compact form fades as it travels; the
  /// cover form travels the whole width at full opacity, like a card.
  variant?: "compact" | "cover";
  children: ReactNode;
};

type SlideState = {
  key: string;
  direction: 0 | 1 | -1;
  leaving?: { key: string; node: ReactNode };
};

/// Long enough to outlast the slowest slide below. The leaving layer is inert
/// and off-screen well before this fires; the timer only stops it accumulating.
const RETIRE_MS = 620;

/// Cross-slides one track's content out while the next comes in, the way every
/// desktop music player animates a skip. The leaving layer renders the node
/// captured from the previous commit, so it keeps the old artwork, tint and
/// copy on its way out instead of snapping to the new track first.
export function TrackSlide({ slideKey, direction, className, variant = "compact", children }: TrackSlideProps) {
  const [state, setState] = useState<SlideState>({ key: slideKey, direction: 0 });
  const rendered = useRef<ReactNode>(children);

  if (state.key !== slideKey) {
    // Adjusting state during render is the supported way to react to a changed
    // input without a wasted commit — and it has to happen here, because the
    // node being captured is the one this render is about to replace.
    setState({ key: slideKey, direction, leaving: { key: state.key, node: rendered.current } });
  }
  rendered.current = children;

  useEffect(() => {
    if (!state.leaving) return;
    const timer = window.setTimeout(() => {
      setState((value) => value.leaving ? { key: value.key, direction: value.direction } : value);
    }, RETIRE_MS);
    return () => window.clearTimeout(timer);
  }, [state.key, state.leaving]);

  const classes = ["track-slide", `track-slide--${variant}`, className].filter(Boolean).join(" ");
  return (
    <div className={classes} data-slide={state.direction || undefined}>
      {state.leaving && (
        <div aria-hidden="true" className="track-slide__layer track-slide__layer--leaving" inert key={state.leaving.key}>
          {state.leaving.node}
        </div>
      )}
      <div className="track-slide__layer track-slide__layer--current" key={state.key}>{children}</div>
    </div>
  );
}
