import type { SongSummary } from "../types";

const LIMIT = 50;

function key(scope: string) {
  return `splice.recentlyPlayed:${encodeURIComponent(scope)}`;
}

export function readRecentlyPlayed(scope: string): SongSummary[] {
  try {
    const parsed: unknown = JSON.parse(localStorage.getItem(key(scope)) ?? "[]");
    return Array.isArray(parsed)
      ? parsed.filter((item): item is SongSummary => Boolean(item) && typeof item === "object" && typeof (item as SongSummary).id === "string").slice(0, LIMIT)
      : [];
  } catch {
    return [];
  }
}

/// Newest first, one entry per song.
export function rememberPlayed(scope: string, current: SongSummary[], song: SongSummary) {
  const next = [song, ...current.filter((item) => item.id !== song.id)].slice(0, LIMIT);
  try { localStorage.setItem(key(scope), JSON.stringify(next)); } catch { /* the list still applies this session */ }
  return next;
}
