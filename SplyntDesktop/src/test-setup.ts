import "@testing-library/jest-dom/vitest";
import { vi } from "vitest";

Object.defineProperties(HTMLMediaElement.prototype, {
  load: { configurable: true, value: vi.fn() },
  pause: { configurable: true, value: vi.fn() },
  play: { configurable: true, value: vi.fn().mockResolvedValue(undefined) },
});

Object.defineProperty(HTMLElement.prototype, "scrollTo", {
  configurable: true,
  value: vi.fn(),
});

class TestMediaMetadata {
  constructor(_metadata?: MediaMetadataInit) {}
}

Object.defineProperty(globalThis, "MediaMetadata", {
  configurable: true,
  value: TestMediaMetadata,
});

Object.defineProperty(navigator, "mediaSession", {
  configurable: true,
  value: {
    metadata: null,
    playbackState: "none",
    setActionHandler: vi.fn(),
    setPositionState: vi.fn(),
  },
});
