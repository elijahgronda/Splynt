import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useMemo, useState } from "react";
import { clampEqualizer, flatEqualizer, type EqualizerSettings } from "./equalizer";

/// Bitrate cap in kbps. 0 is "Original": no transcode, no cap, no target
/// format — the same tier model the iOS settings store uses.
export const qualityTiers = [128, 256, 320, 0] as const;
export type QualityTier = (typeof qualityTiers)[number];

export const audioFormats = ["mp3", "aac", "opus", "wav"] as const;
export type AudioFormat = (typeof audioFormats)[number];

export function tierLabel(tier: QualityTier) {
  return tier === 128 ? "Low" : tier === 256 ? "Medium" : tier === 320 ? "High" : "Original";
}

export function tierDetail(tier: QualityTier) {
  return tier === 0 ? "As stored" : `${tier} kbps`;
}

export function transcodes(tier: QualityTier) {
  return tier !== 0;
}

/// WAV is uncompressed PCM, so a bitrate cap is meaningless and asking a server
/// for format=wav&maxBitRate=256 is the unproduceable combination that yields
/// no audio. The cap is dropped rather than the request being made.
export function acceptsBitRateCap(format: AudioFormat) {
  return format !== "wav";
}

/// The shelves Home can show. Jump Back In is local navigation history, just
/// like iOS, so it stays useful offline. Rows that require the native iOS
/// recommendation engine remain absent rather than being shipped as stubs.
export const homeRows = ["recentlyPlayed", "recentlyAdded", "jumpBackIn", "onRepeat", "genreMixes", "albumsFeaturingLiked", "discover"] as const;
export type HomeRow = (typeof homeRows)[number];

export const homeRowLabels: Record<HomeRow, string> = {
  recentlyPlayed: "Recently played",
  recentlyAdded: "Recently added",
  jumpBackIn: "Jump back in",
  onRepeat: "On repeat",
  genreMixes: "Genre mixes",
  albumsFeaturingLiked: "Albums featuring songs you like",
  discover: "Discover something different",
};

export const lyricsTextSizes = ["small", "standard", "large"] as const;
export type LyricsTextSize = (typeof lyricsTextSizes)[number];

export const lyricsSources = ["auto", "server", "lrclib"] as const;
export type LyricsSource = (typeof lyricsSources)[number];

export const shuffleModes = ["fewerRepeats", "random"] as const;
export type ShuffleMode = (typeof shuffleModes)[number];

export type DesktopSettings = {
  shuffleMode: ShuffleMode;
  libraryGridView: boolean;
  lyricsAutoScroll: boolean;
  lyricsTextSize: LyricsTextSize;
  lyricsSource: LyricsSource;
  hideExternalPlaylists: boolean;
  streamQuality: QualityTier;
  streamFormat: AudioFormat;
  downloadQuality: QualityTier;
  downloadFormat: AudioFormat;
  dataSaver: boolean;
  offlineMode: boolean;
  crossfadeSeconds: number;
  gapless: boolean;
  autoplay: boolean;
  equalizer: EqualizerSettings;
  hideExplicitContent: boolean;
  keepPlayingInBackground: boolean;
  showTrackQuality: boolean;
  homeRowOrder: HomeRow[];
  hiddenHomeRows: HomeRow[];
};

export const defaultSettings: DesktopSettings = {
  shuffleMode: "fewerRepeats",
  libraryGridView: true,
  lyricsAutoScroll: true,
  lyricsTextSize: "standard",
  lyricsSource: "auto",
  hideExternalPlaylists: false,
  streamQuality: 0,
  streamFormat: "mp3",
  downloadQuality: 0,
  downloadFormat: "mp3",
  dataSaver: false,
  offlineMode: false,
  crossfadeSeconds: 0,
  gapless: true,
  autoplay: true,
  equalizer: flatEqualizer,
  hideExplicitContent: false,
  keepPlayingInBackground: false,
  showTrackQuality: false,
  homeRowOrder: [...homeRows],
  hiddenHomeRows: [],
};

function storageKey(scope: string) {
  return `splice.settings.v1:${encodeURIComponent(scope)}`;
}

function coerce(raw: unknown): DesktopSettings {
  const value = (raw ?? {}) as Partial<DesktopSettings>;
  const tier = (candidate: unknown, fallback: QualityTier): QualityTier =>
    (qualityTiers as readonly number[]).includes(candidate as number) ? candidate as QualityTier : fallback;
  const format = (candidate: unknown, fallback: AudioFormat): AudioFormat =>
    (audioFormats as readonly string[]).includes(candidate as string) ? candidate as AudioFormat : fallback;
  const savedHomeOrder = Array.isArray(value.homeRowOrder)
    ? value.homeRowOrder.filter((row, index, rows): row is HomeRow => (homeRows as readonly string[]).includes(row) && rows.indexOf(row) === index)
    : [];
  const hiddenHomeRows = Array.isArray(value.hiddenHomeRows)
    ? value.hiddenHomeRows.filter((row, index, rows): row is HomeRow => (homeRows as readonly string[]).includes(row) && rows.indexOf(row) === index)
    : [];
  return {
    shuffleMode: (shuffleModes as readonly string[]).includes(value.shuffleMode as string) ? value.shuffleMode as ShuffleMode : defaultSettings.shuffleMode,
    libraryGridView: value.libraryGridView === undefined ? defaultSettings.libraryGridView : Boolean(value.libraryGridView),
    lyricsAutoScroll: value.lyricsAutoScroll === undefined ? defaultSettings.lyricsAutoScroll : Boolean(value.lyricsAutoScroll),
    lyricsTextSize: (lyricsTextSizes as readonly string[]).includes(value.lyricsTextSize as string) ? value.lyricsTextSize as LyricsTextSize : defaultSettings.lyricsTextSize,
    lyricsSource: (lyricsSources as readonly string[]).includes(value.lyricsSource as string) ? value.lyricsSource as LyricsSource : defaultSettings.lyricsSource,
    hideExternalPlaylists: Boolean(value.hideExternalPlaylists),
    streamQuality: tier(value.streamQuality, defaultSettings.streamQuality),
    streamFormat: format(value.streamFormat, defaultSettings.streamFormat),
    downloadQuality: tier(value.downloadQuality, defaultSettings.downloadQuality),
    downloadFormat: format(value.downloadFormat, defaultSettings.downloadFormat),
    dataSaver: Boolean(value.dataSaver),
    offlineMode: Boolean(value.offlineMode),
    crossfadeSeconds: Math.max(0, Math.min(12, Number(value.crossfadeSeconds) || 0)),
    gapless: value.gapless === undefined ? defaultSettings.gapless : Boolean(value.gapless),
    autoplay: value.autoplay === undefined ? defaultSettings.autoplay : Boolean(value.autoplay),
    // Every range and the curve's length are fixed up in one place, so a blob
    // written by an older build cannot reach the filters.
    equalizer: clampEqualizer(value.equalizer),
    hideExplicitContent: Boolean(value.hideExplicitContent),
    keepPlayingInBackground: Boolean(value.keepPlayingInBackground),
    showTrackQuality: Boolean(value.showTrackQuality),
    // Unknown ids are dropped and new ones appended, so adding a row later does
    // not strand a saved order.
    homeRowOrder: [
      ...savedHomeOrder,
      ...homeRows.filter((row) => !savedHomeOrder.includes(row)),
    ],
    hiddenHomeRows,
  };
}

export function readSettings(scope: string): DesktopSettings {
  try {
    return coerce(JSON.parse(localStorage.getItem(storageKey(scope)) ?? "{}"));
  } catch {
    return { ...defaultSettings };
  }
}

/// Data Saver overrides the chosen tier rather than editing it, so turning it
/// off restores whatever the user picked.
export function effectiveStreamQuality(settings: DesktopSettings): QualityTier {
  return settings.dataSaver ? 128 : settings.streamQuality;
}

export function useSettings(scope: string) {
  const [settings, setSettings] = useState<DesktopSettings>(() => readSettings(scope));

  useEffect(() => setSettings(readSettings(scope)), [scope]);

  useEffect(() => {
    try {
      localStorage.setItem(storageKey(scope), JSON.stringify(settings));
    } catch {
      // A full store must not block the preference taking effect this session.
    }
    // The native host owns the upstream request, so it needs the transcode
    // decision and the offline switch, not just the UI.
    const quality = effectiveStreamQuality(settings);
    void invoke("set_playback_prefs", {
      prefs: {
        streamBitRate: transcodes(quality) && acceptsBitRateCap(settings.streamFormat) ? quality : 0,
        streamFormat: transcodes(quality) ? settings.streamFormat : null,
        downloadBitRate: transcodes(settings.downloadQuality) && acceptsBitRateCap(settings.downloadFormat) ? settings.downloadQuality : 0,
        downloadFormat: transcodes(settings.downloadQuality) ? settings.downloadFormat : null,
        offlineMode: settings.offlineMode,
      },
    }).catch(() => undefined);
    // Closing the window follows this preference; Quit always stops playback.
    void invoke("set_background_playback", { enabled: settings.keepPlayingInBackground }).catch(() => undefined);
  }, [scope, settings]);

  const update = useCallback(<K extends keyof DesktopSettings>(key: K, value: DesktopSettings[K]) => {
    setSettings((current) => ({ ...current, [key]: value }));
  }, []);

  const reset = useCallback(() => setSettings({ ...defaultSettings }), []);

  return useMemo(() => ({ settings, update, reset }), [reset, settings, update]);
}

export type SettingsController = ReturnType<typeof useSettings>;
