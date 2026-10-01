import { beforeEach, describe, expect, it } from "vitest";
import { cacheConnectedLibrary, cacheLibraryOverview, cachedDataFor, offlineCacheDegraded } from "./persistence";
import type { AlbumSummary, ConnectedLibrary, LibraryOverview } from "../types";

const library: ConnectedLibrary = {
  server: { displayHost: "music.example.test", username: "elijah" },
  albums: [],
};

function albums(count: number): AlbumSummary[] {
  return Array.from({ length: count }, (_, index) => ({ id: `album-${index}`, title: `Album ${index}`, artist: "Artist" }));
}

function overview(count: number): LibraryOverview {
  return { albums: albums(count), artists: [], playlists: [], starredSongs: [], starredAlbums: [], starredArtists: [] };
}

describe("offline cache", () => {
  beforeEach(() => localStorage.clear());

  it("bounds an oversized library instead of writing nothing", () => {
    cacheConnectedLibrary(library);
    cacheLibraryOverview(library, overview(4_000));
    const saved = cachedDataFor(library);
    expect(saved?.overview?.albums.length).toBe(3_000);
    expect(offlineCacheDegraded()).toBe(false);
  });

  it("keeps a small library whole", () => {
    cacheConnectedLibrary(library);
    cacheLibraryOverview(library, overview(12));
    expect(cachedDataFor(library)?.overview?.albums.length).toBe(12);
  });
});
