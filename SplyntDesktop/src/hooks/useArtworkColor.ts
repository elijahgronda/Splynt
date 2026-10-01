import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";

const cache = new Map<string, string>();

/// Samples the cover to tint the collection hero, the way every one of these
/// apps does. The proxy sends Access-Control-Allow-Origin, so an anonymous
/// request keeps the canvas readable; anything that fails falls back to the
/// Splynt green wash rather than blocking the page.
export function useArtworkColor(coverArt?: string) {
  const [color, setColor] = useState<string | undefined>(() => coverArt ? cache.get(coverArt) : undefined);

  useEffect(() => {
    if (!coverArt) {
      setColor(undefined);
      return;
    }
    const cached = cache.get(coverArt);
    if (cached) {
      setColor(cached);
      return;
    }
    let active = true;
    void invoke<string>("media_url", { kind: "cover", id: coverArt })
      .then((url) => new Promise<string | undefined>((resolve) => {
        const image = new Image();
        image.crossOrigin = "anonymous";
        image.onload = () => resolve(sample(image));
        image.onerror = () => resolve(undefined);
        image.src = url;
      }))
      .then((sampled) => {
        if (!active || !sampled) return;
        cache.set(coverArt, sampled);
        setColor(sampled);
      })
      .catch(() => undefined);
    return () => { active = false };
  }, [coverArt]);

  return color;
}

function sample(image: HTMLImageElement) {
  try {
    const size = 24;
    const canvas = document.createElement("canvas");
    canvas.width = size;
    canvas.height = size;
    const context = canvas.getContext("2d", { willReadFrequently: true });
    if (!context) return undefined;
    context.drawImage(image, 0, 0, size, size);
    const { data } = context.getImageData(0, 0, size, size);
    let red = 0;
    let green = 0;
    let blue = 0;
    let weight = 0;
    for (let offset = 0; offset < data.length; offset += 4) {
      const [r, g, b, alpha] = [data[offset], data[offset + 1], data[offset + 2], data[offset + 3]];
      if (alpha < 128) continue;
      // Weight by saturation so a mostly grey sleeve does not average to mud.
      const saturation = Math.max(r, g, b) - Math.min(r, g, b);
      const pixelWeight = 1 + saturation / 32;
      red += r * pixelWeight;
      green += g * pixelWeight;
      blue += b * pixelWeight;
      weight += pixelWeight;
    }
    if (!weight) return undefined;
    return clampForText(red / weight, green / weight, blue / weight);
  } catch {
    // A tainted canvas is not worth reporting; the hero keeps its default wash.
    return undefined;
  }
}

/// Keeps the tint dark enough for white type to stay legible on it.
function clampForText(red: number, green: number, blue: number) {
  const luminance = (0.2126 * red + 0.7152 * green + 0.0722 * blue) / 255;
  const scale = luminance > 0.45 ? 0.45 / luminance : 1;
  const channel = (value: number) => Math.round(Math.max(0, Math.min(255, value * scale)));
  return `rgb(${channel(red)}, ${channel(green)}, ${channel(blue)})`;
}
