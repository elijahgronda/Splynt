import type { AlbumSummary, ArtistSummary, SearchResults, SongSummary } from "../types";

export type ExternalSource = {
  provider: string;
  type: "track" | "album" | "artist" | "playlist" | string;
  sourceId: string;
};

export function parseExternalSource(id?: string): ExternalSource | undefined {
  if (!id?.startsWith("ext-")) return undefined;
  const segments = id.split("-");
  if (segments.length < 4 || !segments[1] || !segments[2] || !segments.slice(3).join("-")) return undefined;
  return {
    provider: segments[1],
    type: segments[2],
    sourceId: segments.slice(3).join("-"),
  };
}

export function externalProviderLabel(id?: string) {
  const source = parseExternalSource(id);
  if (!source) return undefined;
  return source.provider
    .split(/[_.\s]+/)
    .filter(Boolean)
    .map((part) => part[0].toUpperCase() + part.slice(1))
    .join(" ");
}

export function trackMetadataKey(track: Pick<SongSummary, "title" | "artist" | "album">) {
  return [track.title, track.artist, track.album].map(normalize).join("|");
}

export function dedupeSearchResults(results: SearchResults): SearchResults {
  return {
    songs: preferNative(results.songs, trackMetadataKey),
    albums: preferNative(results.albums, albumMetadataKey),
    artists: preferNative(results.artists, (artist) => normalize(artist.name)),
  };
}

function albumMetadataKey(album: AlbumSummary) {
  return [album.title, album.artist].map(normalize).join("|");
}

function preferNative<T extends { id: string }>(items: T[], key: (item: T) => string) {
  const nativeKeys = new Set(items.filter((item) => !parseExternalSource(item.id)).map(key));
  const seen = new Set<string>();
  return items.filter((item) => {
    const itemKey = key(item);
    if (parseExternalSource(item.id) && nativeKeys.has(itemKey)) return false;
    const identity = `${parseExternalSource(item.id) ? "external" : "native"}:${itemKey}`;
    if (seen.has(identity)) return false;
    seen.add(identity);
    return true;
  });
}

function normalize(value: string) {
  return value
    .normalize("NFKD")
    .replace(/[\u0300-\u036f]/g, "")
    .replace(/[^a-zA-Z0-9]+/g, " ")
    .trim()
    .toLowerCase();
}

export function dedupeAlbums(albums: AlbumSummary[]) {
  return preferNative(albums, albumMetadataKey);
}

export function dedupeArtists(artists: ArtistSummary[]) {
  return preferNative(artists, (artist) => normalize(artist.name));
}
