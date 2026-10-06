import type { SongSummary } from "../types";

/// One play on this computer. `at` is milliseconds since the epoch.
export type LoggedPlay = { song: SongSummary; at: number };

const DAYS = 90;
const LIMIT = 5_000;

function key(scope: string) {
  return `splice.playLog:${encodeURIComponent(scope)}`;
}

function recent(plays: LoggedPlay[], now = Date.now()) {
  const cutoff = now - DAYS * 86_400_000;
  return plays.filter((play) => play.at >= cutoff).slice(-LIMIT);
}

/// Plays logged on this computer for this account, oldest first, for the
/// last 90 days. This is what Listening History shows, and what Stats falls
/// back to when the server keeps no record of its own.
export function readPlayLog(scope: string): LoggedPlay[] {
  try {
    const parsed: unknown = JSON.parse(localStorage.getItem(key(scope)) ?? "[]");
    if (!Array.isArray(parsed)) return [];
    return recent(parsed.filter((item): item is LoggedPlay =>
      Boolean(item) && typeof item === "object"
      && typeof (item as LoggedPlay).at === "number"
      && typeof (item as LoggedPlay).song?.id === "string"));
  } catch {
    return [];
  }
}

/// Adds a play. iOS counts a play once it finished or ran past 30%, and the
/// shell calls this at that same line.
export function logPlay(scope: string, song: SongSummary, at = Date.now()) {
  const next = recent([...readPlayLog(scope), { song, at }], at);
  try { localStorage.setItem(key(scope), JSON.stringify(next)); } catch { /* history misses this play */ }
  return next;
}
