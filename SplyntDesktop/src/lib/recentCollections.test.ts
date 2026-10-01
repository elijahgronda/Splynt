import { beforeEach, describe, expect, it } from "vitest";
import { readRecentCollections, rememberRecentCollection } from "./recentCollections";

describe("recent collections", () => {
  beforeEach(() => localStorage.clear());

  it("keeps history scoped to the active server profile", () => {
    rememberRecentCollection("one.example|sam", {
      kind: "album",
      id: "album-1",
      title: "One",
      subtitle: "Artist",
    });

    expect(readRecentCollections("one.example|sam")).toHaveLength(1);
    expect(readRecentCollections("two.example|sam")).toEqual([]);
  });

  it("moves reopened collections to the front without duplicates", () => {
    const first = { kind: "album" as const, id: "album-1", title: "One", subtitle: "Artist" };
    const second = { kind: "playlist" as const, id: "playlist-1", title: "Two", subtitle: "Playlist" };
    rememberRecentCollection("profile", first);
    rememberRecentCollection("profile", second);
    const next = rememberRecentCollection("profile", { ...first, coverArt: "new-cover" });

    expect(next.map((item) => item.id)).toEqual(["album-1", "playlist-1"]);
    expect(next[0].coverArt).toBe("new-cover");
  });

  it("caps valid history and ignores corrupt persisted data", () => {
    for (let index = 0; index < 15; index += 1) {
      rememberRecentCollection("profile", {
        kind: "artist",
        id: `artist-${index}`,
        title: `Artist ${index}`,
        subtitle: "Artist",
      });
    }
    expect(readRecentCollections("profile")).toHaveLength(12);
    expect(readRecentCollections("profile")[0].id).toBe("artist-14");

    localStorage.setItem("splice.jump-back-in.v1:broken", "not json");
    expect(readRecentCollections("broken")).toEqual([]);
  });
});
