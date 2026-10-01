import { parseExternalSource, trackMetadataKey } from "./externalSource";
import type { ConnectedLibrary, SongSummary } from "../types";

type StoredLike = { key: string; song: SongSummary; savedAt: number };

function storageKey(library: ConnectedLibrary) {
  return `splice.external-likes.v1:${encodeURIComponent(`${library.server.displayHost}|${library.server.username}`.toLowerCase())}`;
}

function read(library: ConnectedLibrary): StoredLike[] {
  try {
    const value = JSON.parse(localStorage.getItem(storageKey(library)) ?? "[]") as unknown;
    if (!Array.isArray(value)) return [];
    return value.filter((item): item is StoredLike => Boolean(
      item && typeof item === "object" && "key" in item && "song" in item && typeof item.key === "string",
    ));
  } catch {
    return [];
  }
}

export function externalLikedSongs(library: ConnectedLibrary) {
  return read(library).map((item) => ({ ...item.song, starred: new Date(item.savedAt).toISOString() }));
}

export function isExternalLiked(library: ConnectedLibrary, song: SongSummary) {
  if (!parseExternalSource(song.id)) return false;
  const key = trackMetadataKey(song);
  return read(library).some((item) => item.key === key);
}

export function setExternalLiked(library: ConnectedLibrary, song: SongSummary, liked: boolean) {
  const key = trackMetadataKey(song);
  const current = read(library).filter((item) => item.key !== key);
  if (liked) current.unshift({ key, song: { ...song, starred: undefined }, savedAt: Date.now() });
  try {
    localStorage.setItem(storageKey(library), JSON.stringify(current.slice(0, 10_000)));
  } catch {
    // A failed local mutation is reflected by the next state reconciliation.
  }
}

export function mergeExternalLikes(library: ConnectedLibrary, songs: SongSummary[]) {
  const external = externalLikedSongs(library);
  const externalKeys = new Set(external.map(trackMetadataKey));
  return [...songs.filter((song) => !externalKeys.has(trackMetadataKey(song))), ...external];
}
