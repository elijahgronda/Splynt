import type {
  AlbumDetail, ArtistDetail, ConnectedLibrary, HomeOverview, LibraryOverview, PlaylistDetail,
} from "../types";

const LEGACY_CACHE_KEY = "splice.desktop.active-cache.v1";
const CACHE_KEY = "splice.desktop.profile-cache.v2";
const MAX_PROFILES = 8;
const MAX_DETAILS = 40;
// A whole-library overview can run to tens of thousands of rows, which does not
// fit the browser store. Offline browsing keeps a useful head of each list
// instead of failing the write and silently caching nothing.
const MAX_CACHED_ALBUMS = 3_000;
const MAX_CACHED_ARTISTS = 2_000;
const MAX_CACHED_PLAYLISTS = 500;
const MAX_CACHED_STARRED = 2_000;

let lastWriteFailed = false;

export function offlineCacheDegraded() {
  return lastWriteFailed;
}

function boundedOverview(overview: LibraryOverview): LibraryOverview {
  return {
    albums: overview.albums.slice(0, MAX_CACHED_ALBUMS),
    artists: overview.artists.slice(0, MAX_CACHED_ARTISTS),
    playlists: overview.playlists.slice(0, MAX_CACHED_PLAYLISTS),
    starredSongs: overview.starredSongs.slice(0, MAX_CACHED_STARRED),
    starredAlbums: overview.starredAlbums.slice(0, MAX_CACHED_ALBUMS),
    starredArtists: overview.starredArtists.slice(0, MAX_CACHED_ARTISTS),
  };
}

export type CachedDetail = AlbumDetail | PlaylistDetail | ArtistDetail;

type CachedProfile = {
  identity: string;
  library: ConnectedLibrary;
  home?: HomeOverview;
  overview?: LibraryOverview;
  details?: Record<string, CachedDetail>;
  detailOrder?: string[];
  savedAt: number;
};

type ProfileCacheStore = {
  version: 2;
  activeIdentity?: string;
  profiles: Record<string, CachedProfile>;
};

function identity(library: ConnectedLibrary) {
  return `${library.server.displayHost}|${library.server.username}`.trim().toLowerCase();
}

function emptyStore(): ProfileCacheStore {
  return { version: 2, profiles: {} };
}

function validProfile(value: unknown): value is CachedProfile {
  const profile = value as Partial<CachedProfile> | null;
  return Boolean(profile && typeof profile.identity === "string" && profile.library?.server && Array.isArray(profile.library.albums));
}

function readStore(): ProfileCacheStore {
  try {
    const parsed = JSON.parse(localStorage.getItem(CACHE_KEY) ?? "null") as Partial<ProfileCacheStore> | null;
    if (parsed?.version === 2 && parsed.profiles && typeof parsed.profiles === "object") {
      return { version: 2, activeIdentity: parsed.activeIdentity, profiles: Object.fromEntries(Object.entries(parsed.profiles).filter(([, profile]) => validProfile(profile))) };
    }
    const legacy = JSON.parse(localStorage.getItem(LEGACY_CACHE_KEY) ?? "null") as unknown;
    if (validProfile(legacy)) {
      const migrated = { version: 2 as const, activeIdentity: legacy.identity, profiles: { [legacy.identity]: legacy } };
      writeStore(migrated);
      localStorage.removeItem(LEGACY_CACHE_KEY);
      return migrated;
    }
  } catch {
    // Invalid cache data is discarded instead of blocking launch.
  }
  return emptyStore();
}

export function readCachedProfile(): CachedProfile | undefined {
  const store = readStore();
  return store.activeIdentity ? store.profiles[store.activeIdentity] : undefined;
}

export function cacheConnectedLibrary(library: ConnectedLibrary) {
  const store = readStore();
  const nextIdentity = identity(library);
  const current = store.profiles[nextIdentity];
  store.activeIdentity = nextIdentity;
  store.profiles[nextIdentity] = {
    identity: nextIdentity,
    library: { ...library, connection: { status: "online" } },
    home: current?.home,
    overview: current?.overview,
    details: current?.details,
    detailOrder: current?.detailOrder,
    savedAt: Date.now(),
  };
  pruneProfiles(store);
  writeStore(store);
}

export function cacheHome(library: ConnectedLibrary, home: HomeOverview) {
  updateProfile(library, (current) => ({ ...current, home, savedAt: Date.now() }));
}

export function cacheLibraryOverview(library: ConnectedLibrary, overview: LibraryOverview) {
  updateProfile(library, (current) => ({ ...current, overview: boundedOverview(overview), savedAt: Date.now() }));
}

/// Best effort, and never throws. A failure here used to propagate into the
/// caller's catch and be reported as "this page could not be opened", which was
/// wrong twice over: the page had already loaded, and the real fault was in
/// writing the cache.
export function cacheDetail(library: ConnectedLibrary, kind: "album" | "playlist" | "artist", id: string, detail: CachedDetail) {
  try {
    cacheDetailUnsafe(library, kind, id, detail);
  } catch {
    // Offline browsing loses this page; the live session is unaffected.
    lastWriteFailed = true;
  }
}

function cacheDetailUnsafe(library: ConnectedLibrary, kind: "album" | "playlist" | "artist", id: string, detail: CachedDetail) {
  updateProfile(library, (current) => {
    const key = `${kind}:${id}`;
    const order = [key, ...(current.detailOrder ?? []).filter((item) => item !== key)].slice(0, MAX_DETAILS);
    const details = { ...(current.details ?? {}), [key]: boundedDetail(detail) };
    for (const stale of Object.keys(details)) if (!order.includes(stale)) delete details[stale];
    return { ...current, details, detailOrder: order, savedAt: Date.now() };
  });
}

export function cachedDetail(library: ConnectedLibrary, kind: "album" | "playlist" | "artist", id: string) {
  return cachedDataFor(library)?.details?.[`${kind}:${id}`];
}

export function cachedDataFor(library: ConnectedLibrary) {
  return readStore().profiles[identity(library)];
}

/// Tolerant of a payload missing the list it is discriminated on: the host once
/// serialized these under Subsonic's singular key names, so `songs` and `albums`
/// both arrived undefined and this threw.
function boundedDetail(detail: CachedDetail): CachedDetail {
  if ("songs" in detail) return { ...detail, songs: (detail.songs ?? []).slice(0, 1_000) };
  return { ...detail, albums: (detail.albums ?? []).slice(0, 2_000), topSongs: detail.topSongs?.slice(0, 100) };
}

function updateProfile(library: ConnectedLibrary, update: (current: CachedProfile) => CachedProfile) {
  const store = readStore();
  const key = identity(library);
  const current = store.profiles[key];
  if (!current) return;
  store.profiles[key] = update(current);
  writeStore(store);
}

function pruneProfiles(store: ProfileCacheStore) {
  const profiles = Object.values(store.profiles).sort((a, b) => b.savedAt - a.savedAt);
  for (const stale of profiles.slice(MAX_PROFILES)) delete store.profiles[stale.identity];
}

function writeStore(store: ProfileCacheStore) {
  // Shed the least useful data first rather than losing the whole cache to one
  // over-quota write: other profiles' pages, then other profiles, then this
  // profile's pages.
  const attempts: Array<(value: ProfileCacheStore) => ProfileCacheStore> = [
    (value) => value,
    (value) => mapProfiles(value, (profile) => profile.identity === value.activeIdentity ? profile : { ...profile, details: undefined, detailOrder: undefined }),
    (value) => ({ ...value, profiles: value.activeIdentity && value.profiles[value.activeIdentity] ? { [value.activeIdentity]: value.profiles[value.activeIdentity] } : {} }),
    (value) => mapProfiles(value, (profile) => ({ ...profile, details: undefined, detailOrder: undefined })),
  ];
  for (const shed of attempts) {
    try {
      localStorage.setItem(CACHE_KEY, JSON.stringify(shed(store)));
      lastWriteFailed = false;
      return;
    } catch {
      // Try the next, smaller shape.
    }
  }
  // A full or unavailable browser store must not interrupt the live session.
  lastWriteFailed = true;
}

function mapProfiles(store: ProfileCacheStore, update: (profile: CachedProfile) => CachedProfile): ProfileCacheStore {
  return { ...store, profiles: Object.fromEntries(Object.entries(store.profiles).map(([key, profile]) => [key, update(profile)])) };
}
