import type { LoggedPlay } from "./playLog";
import type { ListeningSnapshot, SongSummary } from "../types";

/// Where the numbers come from, in the order iOS prefers them. Navidrome's
/// own record covers every device that scrobbles to it, so desktop and
/// iPhone show the same figures. The local log is the last resort.
export type StatsSource = "navidromeHistory" | "navidromeTotals" | "deviceHistory";
export type StatsPeriod = "allTime" | "thisYear" | "ninetyDays";

export const sourceTitle: Record<StatsSource, string> = {
  navidromeHistory: "From Navidrome · all your devices",
  navidromeTotals: "From Navidrome · all your devices · lifetime only",
  deviceHistory: "From this computer",
};

export const periodTitle: Record<StatsPeriod, string> = { allTime: "All time", thisYear: "This year", ninetyDays: "90 days" };

/// One play, or for lifetime counters `count` plays with no date.
type Play = { song: SongSummary; genre?: string; at?: number; count: number };

export type Ranked = { id: string; name: string; detail?: string; coverArt?: string; plays: number; song?: SongSummary };
export type DayCount = { day: string; plays: number };
export type Streak = { days: number; endedOn: string };

export type ListeningStats = {
  source: StatsSource;
  period: StatsPeriod;
  hasTimeline: boolean;
  plays: number;
  secondsListened: number;
  distinctTracks: number;
  distinctArtists: number;
  distinctAlbums: number;
  topTracks: Ranked[];
  topArtists: Ranked[];
  topAlbums: Ranked[];
  topGenres: Ranked[];
  /// 24 entries, local hour of day. Empty without a timeline.
  hours: number[];
  /// 7 entries, Sunday first. Empty without a timeline.
  weekdays: number[];
  peakDay?: DayCount;
  longestStreak?: Streak;
  currentStreak?: Streak;
  /// Scrobbles whose song no longer resolves against the server's catalogue.
  /// Shown rather than hidden, since it says how complete the rest is.
  unresolvedPlays: number;
};

export type StatsInput = { source: StatsSource; plays: Play[]; unresolved: number };

/// Picks the best source available, as iOS does: timestamped history, then
/// lifetime counters, then this computer's own log.
export function statsInput(snapshot: ListeningSnapshot | null | undefined, log: LoggedPlay[]): StatsInput {
  const songs = new Map((snapshot?.playedSongs ?? []).map((song) => [song.id, song]));
  if (snapshot?.plays?.length) {
    let unresolved = 0;
    const plays: Play[] = [];
    for (const play of snapshot.plays) {
      const song = songs.get(play.trackId);
      if (!song) { unresolved += 1; continue; }
      plays.push({ song, genre: song.genre, at: play.at * 1000, count: 1 });
    }
    return { source: "navidromeHistory", plays, unresolved };
  }
  if (snapshot?.playedSongs?.length) {
    return { source: "navidromeTotals", plays: snapshot.playedSongs.map((song) => ({ song, genre: song.genre, count: song.playCount })), unresolved: 0 };
  }
  return { source: "deviceHistory", plays: log.map((play) => ({ song: play.song, at: play.at, count: 1 })), unresolved: 0 };
}

function dayKey(at: number) {
  const date = new Date(at);
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}`;
}

function inPeriod(at: number, period: StatsPeriod, now: Date) {
  if (period === "allTime") return true;
  if (period === "thisYear") return new Date(at).getFullYear() === now.getFullYear();
  return at >= now.getTime() - 90 * 86_400_000;
}

function rank(groups: Map<string, Ranked>, limit: number) {
  return [...groups.values()].sort((a, b) => b.plays - a.plays || a.name.localeCompare(b.name)).slice(0, limit);
}

function tally(groups: Map<string, Ranked>, id: string, count: number, make: () => Omit<Ranked, "plays">) {
  const existing = groups.get(id);
  if (existing) existing.plays += count;
  else groups.set(id, { ...make(), plays: count });
}

/// Runs of consecutive days with at least one play, from sorted day keys.
function streaks(days: string[], now: Date) {
  let longest: Streak | undefined;
  let run = 0;
  let previous: number | undefined;
  let last: Streak | undefined;
  for (const day of days) {
    const time = new Date(`${day}T12:00:00`).getTime();
    run = previous !== undefined && Math.round((time - previous) / 86_400_000) === 1 ? run + 1 : 1;
    previous = time;
    last = { days: run, endedOn: day };
    if (!longest || run > longest.days) longest = last;
  }
  // A streak is current while it ended today or yesterday; a day without a
  // play yet today has not broken it.
  const yesterday = new Date(now.getTime() - 86_400_000);
  const current = last && (last.endedOn === dayKey(now.getTime()) || last.endedOn === dayKey(yesterday.getTime())) ? last : undefined;
  return { longest, current };
}

export function buildStats(input: StatsInput, period: StatsPeriod, now = new Date()): ListeningStats {
  const hasTimeline = input.source !== "navidromeTotals";
  // Lifetime counters carry no dates, so they only answer for all time.
  const effective = hasTimeline ? period : "allTime";
  const plays = input.plays.filter((play) => play.at === undefined || inPeriod(play.at, effective, now));
  const tracks = new Map<string, Ranked>();
  const artists = new Map<string, Ranked>();
  const albums = new Map<string, Ranked>();
  const genres = new Map<string, Ranked>();
  const hours = hasTimeline ? Array<number>(24).fill(0) : [];
  const weekdays = hasTimeline ? Array<number>(7).fill(0) : [];
  const days = new Map<string, number>();
  let total = 0;
  let seconds = 0;
  for (const { song, genre, at, count } of plays) {
    total += count;
    seconds += count * (song.duration ?? 0);
    tally(tracks, song.id, count, () => ({ id: song.id, name: song.title, detail: song.artist, coverArt: song.coverArt, song }));
    const artistKey = song.artistId || song.artist.toLocaleLowerCase();
    if (song.artist) tally(artists, artistKey, count, () => ({ id: song.artistId ?? "", name: song.artist, coverArt: song.coverArt }));
    const albumKey = song.albumId || `${song.artist}|${song.album}`;
    if (song.album) tally(albums, albumKey, count, () => ({ id: song.albumId ?? "", name: song.album, detail: song.artist, coverArt: song.coverArt }));
    for (const name of (genre ?? "").split(/[;/]/).map((part) => part.trim()).filter(Boolean)) {
      tally(genres, name.toLocaleLowerCase(), count, () => ({ id: name, name }));
    }
    if (at !== undefined) {
      const date = new Date(at);
      hours[date.getHours()] += count;
      weekdays[date.getDay()] += count;
      const day = dayKey(at);
      days.set(day, (days.get(day) ?? 0) + count);
    }
  }
  const sortedDays = [...days.keys()].sort();
  const peak = [...days.entries()].reduce<DayCount | undefined>((best, [day, count]) => !best || count > best.plays ? { day, plays: count } : best, undefined);
  const { longest, current } = streaks(sortedDays, now);
  return {
    source: input.source,
    period: effective,
    hasTimeline,
    plays: total,
    secondsListened: seconds,
    distinctTracks: tracks.size,
    distinctArtists: artists.size,
    distinctAlbums: albums.size,
    topTracks: rank(tracks, 10),
    topArtists: rank(artists, 10),
    topAlbums: rank(albums, 10),
    topGenres: rank(genres, 8),
    hours,
    weekdays,
    peakDay: peak,
    longestStreak: longest,
    currentStreak: current,
    unresolvedPlays: input.unresolved,
  };
}
