import { invoke } from "@tauri-apps/api/core";
import { Disc3, ListMusic, UserRound } from "lucide-react";
import { useEffect, useState } from "react";

const coverCache = new Map<string, string>();

type MediaArtworkProps = {
  coverArt?: string;
  alt: string;
  className?: string;
  shape?: "square" | "circle";
  fallback?: "album" | "artist" | "playlist";
};

export function MediaArtwork({
  coverArt,
  alt,
  className,
  shape = "square",
  fallback = "album",
}: MediaArtworkProps) {
  const [source, setSource] = useState(() => coverArt ? coverCache.get(coverArt) : undefined);
  // Load and failure are recorded against the URL they happened to, not as
  // flags the effect resets. A cover already in the image cache can fire
  // `load` before this component's effect runs, and resetting a plain
  // `loaded` flag there left that cover at opacity 0 for good. That is why
  // artwork that had shown before went blank on the next page that used it.
  const [loadedSource, setLoadedSource] = useState<string>();
  const [failedSource, setFailedSource] = useState<string>();
  const loaded = source !== undefined && loadedSource === source;
  const failed = source !== undefined && failedSource === source;

  useEffect(() => {
    let active = true;
    if (!coverArt) {
      setSource(undefined);
      return;
    }
    const cached = coverCache.get(coverArt);
    if (cached) {
      setSource(cached);
      return;
    }
    invoke<string>("media_url", { kind: "cover", id: coverArt })
      .then((url) => {
        if (!active) return;
        coverCache.set(coverArt, url);
        setSource(url);
      })
      .catch(() => { if (active) setSource(undefined); });
    return () => { active = false };
  }, [coverArt]);

  const Icon = fallback === "artist" ? UserRound : fallback === "playlist" ? ListMusic : Disc3;
  // `media-artwork` is always present and any caller class is added to it, not
  // substituted for it. It used to be the default value of `className`, so the
  // sixteen call sites that pass their own size class silently dropped it —
  // along with `overflow: hidden` and the rule that makes the <img> fill its
  // box. Those covers came back from the server at 800px and rendered at
  // natural size, which is what turned the library rail into a grey smear.
  const classes = ["media-artwork", className, shape === "circle" ? "media-artwork--circle" : ""];
  return (
    <span className={classes.filter(Boolean).join(" ")}>
      {source && !failed ? (
        <img
          alt={alt}
          className={loaded ? "is-loaded" : undefined}
          draggable={false}
          onError={() => setFailedSource(source)}
          onLoad={() => setLoadedSource(source)}
          // An image the webview already holds can be complete before React
          // attaches `onLoad`, and then no load event arrives at all.
          ref={(image) => { if (image?.complete && image.naturalWidth > 0 && loadedSource !== source) setLoadedSource(source); }}
          src={source}
        />
      ) : (
        <Icon aria-hidden="true" />
      )}
    </span>
  );
}
