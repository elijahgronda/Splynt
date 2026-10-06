import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  BarChart3, Check, ChevronLeft, ChevronRight, Clock3, History, Home, Download, Heart, Library, List, LogOut, PanelLeftClose, PanelLeftOpen, Pin, Play, Plus,
  Search, Settings, SquareLibrary, UserRound, X,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { ComponentPropsWithoutRef, CSSProperties, ReactNode, KeyboardEvent as ReactKeyboardEvent, MouseEvent as ReactMouseEvent, PointerEvent as ReactPointerEvent } from "react";
import { usePlayback } from "../hooks/usePlayback";
import { useDownloads } from "../hooks/useDownloads";
import { useDialogFocus } from "../hooks/useDialogFocus";
import { dedupeAlbums, dedupeArtists, dedupeSearchResults, parseExternalSource, trackMetadataKey } from "../lib/externalSource";
import { isExternalLiked, mergeExternalLikes, setExternalLiked } from "../lib/externalLikes";
import { cacheDetail, cachedDetail, cacheHome, cacheLibraryOverview, cachedDataFor } from "../lib/persistence";
import { driftCorrection, projectedGroupPosition } from "../lib/connectClock";
import {
  DROP_COOLDOWN_SECONDS, TRACK_GUARD_MS, decideFollow, deviceName, followPeers, hasRemoteControl, notePeerActivity,
  projectedPeerPosition, repeatFromWire, repeatToWire, trackGuardIgnores, type PeerActivity, type TrackGuard,
} from "../lib/connectFollow";
import { endGroupDiagnostics, logConnectEvent, noteGroupSample, noteGroupTrackChange } from "../lib/connectDiagnostics";
import { readRecentCollections, rememberRecentCollection } from "../lib/recentCollections";
import { readRecentlyPlayed, rememberPlayed } from "../lib/recentlyPlayed";
import { useSettings } from "../lib/settings";
import { useTooltips } from "../hooks/useTooltips";
import type {
  AlbumDetail, AlbumSummary, AutoplayStatus, ArtistDetail, ArtistSummary, ConnectedLibrary, ConnectCommand,
  ConnectGroup, ConnectPeer, ConnectQueue, ConnectSnapshot, ContextPanelMode, DesktopRoute, HomeOverview, HomeShortcut,
  JumpBackInItem, LibraryOverview, ListeningSnapshot, LyricsIndexStatus, LyricsMatch, LyricsResult, PlayQueueSnapshot, PlaylistDetail, PlaylistSummary,
  RadioResult, SearchResults, SongSummary,
} from "../types";
import { readSongDrag, SONG_DRAG_TYPE } from "./Catalog";
import { Brand } from "./Brand";
import {
  DesktopContextPanel, FullPlayer, TrackContextMenu, type FullPlayerSurface, type TrackMenuState,
} from "./DesktopPanels";
import { MediaArtwork } from "./MediaArtwork";
import { CollectionMenu, type CollectionMenuState } from "./CollectionMenu";
import { PlayerBar } from "./PlayerBar";
import * as Views from "./DesktopViews";
import { HistoryView, StatsView } from "./ListeningViews";
import { logPlay, readPlayLog } from "../lib/playLog";
import { statsInput, type StatsInput } from "../lib/listeningStats";

type AuthenticatedShellProps = {
  library: ConnectedLibrary;
  onConnectionRestored: (library: ConnectedLibrary) => void;
  onSignedOut: () => void;
};
type DetailState = AlbumDetail | PlaylistDetail | ArtistDetail;
type LibraryFilter = Views.LibraryFilter;
type LibrarySort = "recents" | "added" | "alphabetical" | "creator";
const librarySorts: Array<[LibrarySort, string]> = [["recents", "Recents"], ["added", "Recently Added"], ["alphabetical", "Alphabetical"], ["creator", "Creator"]];
const SIDEBAR_COLLAPSED = 72;
const SIDEBAR_MIN_EXPANDED = 280;
const SIDEBAR_MAX = 420;

/// Spotify's library has two states, an icon rail and a full list at least
/// 280px wide. Widths in between squeezed the chips and titles into clipping,
/// so a drag snaps to whichever state it is nearer.
function snapSidebarWidth(width: number) {
  if (width < (SIDEBAR_COLLAPSED + SIDEBAR_MIN_EXPANDED) / 2) return SIDEBAR_COLLAPSED;
  return Math.min(SIDEBAR_MAX, Math.max(SIDEBAR_MIN_EXPANDED, width));
}
const COMPACT_HEADER_AT = 240;
type LocalGroupSession = { id: string; leaderID: string; memberIDs: string[] };

/// How long a leader waits for devices to answer an invitation. Long enough
/// for a follower to resolve a cold track against the server, short enough
/// that a listener does not think the button is broken.
const GROUP_JOIN_TIMEOUT_MS = 3_000;

const emptySearch: SearchResults = { songs: [], albums: [], artists: [] };
const emptyLyrics: LyricsResult = { synced: false, lines: [] };

function routeKey(route: DesktopRoute) {
  if (route.kind === "liked" && route.artist) return `liked:${route.artist.id}`;
  return "id" in route ? `${route.kind}:${route.id}` : route.kind;
}

/// Liked Songs by one artist. Matches the artist id, and the name only for a
/// song that carries no artist id, as iOS does.
function likedBy(songs: SongSummary[], artist: { id: string; name: string }) {
  const name = artist.name.toLocaleLowerCase();
  return songs.filter((song) => song.artistId ? song.artistId === artist.id : song.artist.toLocaleLowerCase() === name);
}

function reasonMessage(reason: unknown, fallback: string) {
  const message = typeof reason === "string" ? reason : fallback;
  // Warn rather than error: these are handled failures that the user already
  // sees. The point is that the log knows about them too.
  console.warn("[splice]", message, reason instanceof Error ? reason : "");
  return message;
}

function readNumber(key: string, fallback: number) {
  const saved = localStorage.getItem(key);
  if (saved === null || saved.trim() === "") return fallback;
  const value = Number(saved);
  return Number.isFinite(value) ? value : fallback;
}

function readRecentSearches(key: string) {
  try {
    const value = JSON.parse(localStorage.getItem(key) ?? "[]");
    return Array.isArray(value) ? value.filter((item): item is string => typeof item === "string").slice(0, 8) : [];
  } catch {
    return [];
  }
}

export function AuthenticatedShell({ library, onConnectionRestored, onSignedOut }: AuthenticatedShellProps) {
  const cached = cachedDataFor(library);
  const profileScope = `${library.server.displayHost}|${library.server.username}`;
  const { settings, update: updateSetting, reset: resetSettings } = useSettings(profileScope);
  const queueExhausted = useRef<(last: SongSummary) => void>(() => undefined);
  const mirrorAction = useRef<(action: "play" | "pause" | "next" | "previous" | "seek", value?: number) => void>(() => undefined);
  const playback = usePlayback(profileScope, {
    shuffleMode: settings.shuffleMode,
    crossfadeSeconds: settings.crossfadeSeconds,
    equalizer: settings.equalizer,
    gapless: settings.gapless,
    autoplay: settings.autoplay,
    onQueueExhausted: (last) => queueExhausted.current(last),
    onMirrorAction: (action, value) => mirrorAction.current(action, value),
  });
  const downloads = useDownloads();
  const playbackRef = useRef(playback);
  playbackRef.current = playback;
  const workspaceRef = useRef<HTMLDivElement>(null);
  const searchHistoryKey = `splice.search.history:${encodeURIComponent(`${library.server.displayHost}|${library.server.username}`)}`;
  const scrollPositions = useRef(new Map<string, number>());
  const [history, setHistory] = useState<DesktopRoute[]>([{ kind: "home" }]);
  const [historyIndex, setHistoryIndex] = useState(0);
  const route = history[historyIndex];
  const [homeData, setHomeData] = useState<HomeOverview>(cached?.home ?? { newest: library.albums, recent: [], frequent: [], random: [], genres: [] });
  const [libraryData, setLibraryData] = useState<LibraryOverview>(() => {
    const initial = cached?.overview ?? { albums: library.albums, artists: [], playlists: [], starredSongs: [], starredAlbums: [], starredArtists: [] };
    return { ...initial, starredSongs: mergeExternalLikes(library, initial.starredSongs) };
  });
  const [connection, setConnection] = useState(library.connection ?? { status: "online" as const });
  const [libraryFilter, setLibraryFilter] = useState<LibraryFilter>();
  const [librarySearch, setLibrarySearch] = useState("");
  const [librarySearchOpen, setLibrarySearchOpen] = useState(false);
  const [sortMenuOpen, setSortMenuOpen] = useState(false);
  const [searchFocused, setSearchFocused] = useState(false);
  const [recentSearches, setRecentSearches] = useState<string[]>(() => readRecentSearches(searchHistoryKey));
  const [query, setQuery] = useState("");
  const [searchResults, setSearchResults] = useState<SearchResults>(emptySearch);
  const [searching, setSearching] = useState(false);
  const [searchFilter, setSearchFilter] = useState<Views.SearchFilter>("all");
  const [detail, setDetail] = useState<DetailState>();
  const [radioData, setRadioData] = useState<RadioResult>();
  const [pageLoading, setPageLoading] = useState(false);
  const [homeError, setHomeError] = useState<string>();
  const [libraryError, setLibraryError] = useState<string>();
  const [pageError, setPageError] = useState<string>();
  const [reconnecting, setReconnecting] = useState(false);
  const [leavingSession, setLeavingSession] = useState(false);
  const [toast, setToast] = useState<string>();
  const [panelMode, setPanelMode] = useState<ContextPanelMode>();
  const [fullPlayer, setFullPlayer] = useState(false);
  const [fullPlayerSurface, setFullPlayerSurface] = useState<FullPlayerSurface>("artwork");
  const [windowFullscreen, setWindowFullscreen] = useState(false);
  const [accountOpen, setAccountOpen] = useState(false);
  const [lyrics, setLyrics] = useState<LyricsResult>(emptyLyrics);
  const [lyricsLoading, setLyricsLoading] = useState(false);
  const [connectState, setConnectState] = useState<ConnectSnapshot>({ isAvailable: false, peers: [], commands: [] });
  const connectRef = useRef(connectState);
  connectRef.current = connectState;
  /// Measured clock offsets by peer id, in milliseconds, from the Rust side's
  /// probe. Held in a ref rather than read off `connectState` because the
  /// commands in a snapshot are applied in the same tick that delivered it,
  /// before React has published the new state.
  const clockOffsets = useRef<Record<string, number>>({});
  /// The peer this computer follows: the device playing, shown here as if it
  /// were local, with every control driving it. iOS has the same mode
  /// (`connectedPeerID` in PlayerEngine). Set by `beginFollow` and cleared by
  /// `endFollow`, which keep `remoteRef` in step at once rather than a render
  /// later, because snapshots are processed between renders.
  const [remoteDevice, setRemoteDevice] = useState<{ id: string; name: string; platform?: string }>();
  const remoteRef = useRef(remoteDevice);
  /// The latest snapshot, for work that finishes after a later poll.
  const snapshotRef = useRef<ConnectSnapshot | undefined>(undefined);
  /// When each peer started and stopped playing, for the follow rules.
  const peerActivity = useRef<PeerActivity>({});
  /// When this computer's own audio last stopped, or the listener last took
  /// playback here. A paused computer follows only a device that started
  /// after this. Undefined until something plays here.
  const localStoppedAt = useRef<number | undefined>(undefined);
  const droppedPeer = useRef<{ id: string; until: number } | undefined>(undefined);
  const trackGuard = useRef<TrackGuard | undefined>(undefined);
  /// Resolved songs by id, so a mirrored queue that changes by one row does
  /// not resolve the other nine hundred again.
  const songCache = useRef(new Map<string, SongSummary>());
  /// The followed device's queue as mirrored here: its revision, and for each
  /// local row that row's index in the device's whole queue, which `skipTo`
  /// sends. Unresolvable rows are left out, so the two can differ.
  const mirrorRows = useRef<{ peerId: string; revision: number; fullIndexes: number[] } | undefined>(undefined);
  const queueApplying = useRef<string | undefined>(undefined);
  const queueRequested = useRef<{ peerId: string; revision: number; at: number } | undefined>(undefined);
  const mirrorGeneration = useRef(0);
  /// Whether this computer has played audio since its queue was last loaded
  /// from somewhere else. Only the device rendering audio may save the play
  /// queue to the server; anything else overwrites the copy the device that
  /// actually played saved.
  const renderedHere = useRef(false);
  const [groupSession, setGroupSession] = useState<LocalGroupSession>();
  const groupRef = useRef(groupSession);
  groupRef.current = groupSession;
  const lastGroupTrack = useRef<string | undefined>(undefined);
  /// The invitation this device is waiting on answers for. A member is a
  /// device that came back and said it is rendering the session, not one this
  /// device managed to write a frame at: a stale-but-open socket accepts
  /// writes long after the peer behind it stopped listening, which is how a
  /// session used to report a member it had never reached.
  const pendingInvite = useRef<{
    sessionId: string; awaiting: Set<string>; accepted: string[]; declined: Map<string, string>;
  } | undefined>(undefined);
  /// The session revision this device has applied, as follower or leader.
  const groupRevision = useRef(0);
  /// Consecutive failed sends per member. A member that cannot be reached three
  /// times running is dropped from the session rather than addressed forever.
  const groupSendFailures = useRef(new Map<string, number>());
  const [sidebarWidth, setSidebarWidth] = useState(() => snapSidebarWidth(readNumber("splice.sidebar.width", SIDEBAR_MIN_EXPANDED)));
  const [trackMenu, setTrackMenu] = useState<TrackMenuState>();
  const expandedSidebarWidth = useRef(Math.max(SIDEBAR_MIN_EXPANDED, sidebarWidth));
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());
  const selectionAnchor = useRef(0);
  const [createPlaylistOpen, setCreatePlaylistOpen] = useState(false);
  const [pendingPlaylistSongs, setPendingPlaylistSongs] = useState<SongSummary[]>();
  const [saveQueueOpen, setSaveQueueOpen] = useState(false);
  const [clearDownloadsConfirm, setClearDownloadsConfirm] = useState(false);
  const [signOutConfirm, setSignOutConfirm] = useState(false);
  const [lightbox, setLightbox] = useState<{ coverArt?: string; alt: string }>();
  const [recentlyPlayed, setRecentlyPlayed] = useState<SongSummary[]>(() => readRecentlyPlayed(profileScope));
  const tooltip = useTooltips();
  const [playlistEdit, setPlaylistEdit] = useState<{ id: string; name: string }>();
  const [playlistDelete, setPlaylistDelete] = useState<{ id: string; name: string }>();
  const [collectionMenu, setCollectionMenu] = useState<CollectionMenuState>();
  const [dropPlaylistId, setDropPlaylistId] = useState<string>();
  const [shortcutsOpen, setShortcutsOpen] = useState(false);
  const [jumpBackIn, setJumpBackIn] = useState<JumpBackInItem[]>(() => readRecentCollections(profileScope));
  const pinnedKey = `splice.pinned:${encodeURIComponent(profileScope)}`;
  const [pinned, setPinned] = useState<Set<string>>(() => {
    try {
      const parsed: unknown = JSON.parse(localStorage.getItem(pinnedKey) ?? "[]");
      return new Set(Array.isArray(parsed) ? parsed.filter((id): id is string => typeof id === "string") : []);
    } catch {
      return new Set<string>();
    }
  });
  const [librarySort, setLibrarySort] = useState<LibrarySort>(() => {
    const saved = localStorage.getItem("splice.library.sort");
    return librarySorts.some(([value]) => value === saved) ? saved as LibrarySort : "recents";
  });
  useEffect(() => localStorage.setItem("splice.library.sort", librarySort), [librarySort]);
  const togglePinned = useCallback((id: string) => {
    setPinned((current) => {
      const next = new Set(current);
      if (next.has(id)) next.delete(id); else next.add(id);
      try { localStorage.setItem(pinnedKey, JSON.stringify([...next])); } catch { /* a full store still applies this session */ }
      return next;
    });
  }, [pinnedKey]);
  const [headerCompact, setHeaderCompact] = useState(false);
  const [contextWidth, setContextWidth] = useState(() => Math.max(280, Math.min(460, readNumber("splice.context.width", 350))));
  const [sleepAt, setSleepAt] = useState<number>();
  const [sleepRemaining, setSleepRemaining] = useState<number>();
  const restoredServerQueue = useRef(false);
  // Offline fallbacks read through a ref so a library refresh does not re-issue
  // the live search or rebuild radio behind the user.
  const offlineSourceRef = useRef({ albums: libraryData.albums, artists: libraryData.artists, starredSongs: libraryData.starredSongs, downloads: downloads.items });
  offlineSourceRef.current = { albums: libraryData.albums, artists: libraryData.artists, starredSongs: libraryData.starredSongs, downloads: downloads.items };

  useEffect(() => localStorage.setItem("splice.sidebar.width", String(sidebarWidth)), [sidebarWidth]);
  useEffect(() => localStorage.setItem("splice.context.width", String(contextWidth)), [contextWidth]);
  useEffect(() => setJumpBackIn(readRecentCollections(profileScope)), [profileScope]);

  // The now-playing view lives inside the window, under the top bar, like
  // Spotify's. Taking the whole screen is a separate, explicit choice.
  const toggleWindowFullscreen = useCallback(() => {
    const next = !windowFullscreen;
    setWindowFullscreen(next);
    if ("__TAURI_INTERNALS__" in window) {
      void getCurrentWindow().setFullscreen(next).catch(() => setWindowFullscreen(!next));
    } else if (next) {
      void document.documentElement.requestFullscreen?.().catch(() => setWindowFullscreen(false));
    } else if (document.fullscreenElement) {
      void document.exitFullscreen().catch(() => undefined);
    }
  }, [windowFullscreen]);

  useEffect(() => {
    if ("__TAURI_INTERNALS__" in window) return;
    const sync = () => setWindowFullscreen(Boolean(document.fullscreenElement));
    document.addEventListener("fullscreenchange", sync);
    return () => document.removeEventListener("fullscreenchange", sync);
  }, []);

  // Leaving the now-playing view always gives the window back.
  useEffect(() => {
    if (fullPlayer || !windowFullscreen) return;
    setWindowFullscreen(false);
    if ("__TAURI_INTERNALS__" in window) void getCurrentWindow().setFullscreen(false).catch(() => undefined);
    else if (document.fullscreenElement) void document.exitFullscreen().catch(() => undefined);
  }, [fullPlayer, windowFullscreen]);

  const navigate = useCallback((next: DesktopRoute) => {
    const current = history[historyIndex];
    // Going anywhere from the top bar closes the now-playing view, as Spotify does.
    setFullPlayer(false);
    if (routeKey(next) === routeKey(current)) {
      // Choosing the page you are on returns it to the top, like Spotify's nav.
      workspaceRef.current?.scrollTo({ top: 0 });
      return;
    }
    scrollPositions.current.set(routeKey(current), workspaceRef.current?.scrollTop ?? 0);
    // A fresh visit starts at the top; only Back and Forward restore a position.
    scrollPositions.current.delete(routeKey(next));
    if (current.kind === "search" && next.kind !== "search") setQuery("");
    setHistory((current) => [...current.slice(0, historyIndex + 1), next]);
    setHistoryIndex((value) => value + 1);
    setSelectedIds(new Set());
    setPageError(undefined);
    setAccountOpen(false);
  }, [history, historyIndex]);

  const goBack = useCallback(() => {
    scrollPositions.current.set(routeKey(route), workspaceRef.current?.scrollTop ?? 0);
    setHistoryIndex((value) => Math.max(0, value - 1));
  }, [route]);
  const goForward = useCallback(() => {
    scrollPositions.current.set(routeKey(route), workspaceRef.current?.scrollTop ?? 0);
    setHistoryIndex((value) => Math.min(history.length - 1, value + 1));
  }, [history.length, route]);

  useEffect(() => {
    const position = scrollPositions.current.get(routeKey(route)) ?? 0;
    window.requestAnimationFrame(() => {
      workspaceRef.current?.scrollTo({ top: position });
      setHeaderCompact((workspaceRef.current?.scrollTop ?? 0) > COMPACT_HEADER_AT);
    });
  }, [route]);

  useEffect(() => {
    const onMouseUp = (event: MouseEvent) => {
      if (event.button === 3) { event.preventDefault(); goBack(); }
      else if (event.button === 4) { event.preventDefault(); goForward(); }
    };
    window.addEventListener("mouseup", onMouseUp);
    return () => window.removeEventListener("mouseup", onMouseUp);
  }, [goBack, goForward]);

  useEffect(() => {
    const song = playback.current;
    if (!song) return;
    setRecentlyPlayed((current) => current[0]?.id === song.id ? current : rememberPlayed(profileScope, current, song));
  }, [playback.current?.id, profileScope]);

  const wasRendering = useRef(false);
  useEffect(() => {
    if (playback.rendering) renderedHere.current = true;
    else if (wasRendering.current) localStoppedAt.current = Date.now() / 1000;
    wasRendering.current = playback.rendering;
  }, [playback.rendering]);

  /// Songs for ids, resolved against the server in batches and cached. A song
  /// that will not resolve comes back undefined rather than failing the rest.
  const resolveSongs = useCallback(async (ids: string[]) => {
    const missing = [...new Set(ids.filter((id) => !songCache.current.has(id)))];
    for (let start = 0; start < missing.length; start += 100) {
      try {
        const songs = await invoke<SongSummary[]>("get_songs_by_ids", { ids: missing.slice(start, start + 100) });
        for (const song of songs ?? []) songCache.current.set(song.id, song);
      } catch {
        // Left out below. One bad batch must not lose the rest of a queue.
      }
    }
    if (songCache.current.size > 5_000) {
      const keep = new Set(ids);
      for (const id of songCache.current.keys()) if (!keep.has(id)) songCache.current.delete(id);
    }
    return ids.map((id) => songCache.current.get(id));
  }, []);

  const reloadHome = useCallback(() => {
    setHomeError(undefined);
    invoke<HomeOverview>("load_home").then((raw) => {
      const value: HomeOverview = {
        newest: dedupeAlbums(raw.newest),
        recent: dedupeAlbums(raw.recent),
        frequent: dedupeAlbums(raw.frequent),
        random: dedupeAlbums(raw.random),
        genres: (raw.genres ?? []).map((shelf) => ({ ...shelf, albums: dedupeAlbums(shelf.albums) })),
      };
      setHomeData(value);
      cacheHome(library, value);
      setConnection({ status: "online" });
    }).catch((reason) => {
      const message = reasonMessage(reason, "Home could not be refreshed.");
      setHomeError(message);
      setConnection((current) => ({ status: "offline", message: current.message ?? message }));
    });
  }, [library]);
  const reloadLibrary = useCallback(() => {
    setLibraryError(undefined);
    invoke<LibraryOverview>("load_library").then((value) => {
      const merged = {
        ...value,
        albums: dedupeAlbums(value.albums),
        artists: dedupeArtists(value.artists),
        starredAlbums: dedupeAlbums(value.starredAlbums),
        starredArtists: dedupeArtists(value.starredArtists),
        starredSongs: mergeExternalLikes(library, value.starredSongs),
      };
      setLibraryData(merged);
      cacheLibraryOverview(library, merged);
      setConnection({ status: "online" });
    }).catch((reason) => {
      const message = reasonMessage(reason, "Your library could not be refreshed.");
      setLibraryError(message);
      setConnection((current) => ({ status: "offline", message: current.message ?? message }));
    });
  }, [library]);

  const offline = connection.status === "offline" || settings.offlineMode;

  // Navidrome's play queue is the cross-client recovery point used by iOS as
  // well. A profile's local queue wins when it exists; otherwise desktop picks
  // up the server queue without unexpectedly starting playback.
  useEffect(() => {
    if (offline || restoredServerQueue.current) return;
    restoredServerQueue.current = true;
    if (playbackRef.current.queue.length) return;
    void invoke<PlayQueueSnapshot | null>("get_play_queue")
      .then((snapshot) => {
        // A device already playing on the account is followed instead, and
        // its queue is newer than anything the server holds.
        if (!snapshot?.songs?.length || playbackRef.current.queue.length || remoteRef.current) return;
        const match = snapshot.currentId ? snapshot.songs.findIndex((song) => song.id === snapshot.currentId) : 0;
        playbackRef.current.playQueue(snapshot.songs, Math.max(0, match), false, snapshot.position, "Your queue");
        setToast("Queue restored from your other Splynt devices");
      })
      .catch(() => undefined);
  }, [offline]);

  const saveServerQueue = useCallback(() => {
    const controller = playbackRef.current;
    if (offline || !controller.current || !controller.queue.length) return;
    // Navidrome keeps one play queue per account. Saving it from a computer
    // that is idle, or following a phone, replaced the phone's queue with
    // whatever this computer last held, every five seconds.
    if (remoteRef.current || controller.isMirroring() || !renderedHere.current) return;
    void invoke("save_play_queue", {
      ids: controller.queue.slice(0, 1_000).map((song) => song.id),
      currentId: controller.current.id,
      position: controller.position,
    }).catch(() => undefined);
  }, [offline]);

  useEffect(() => {
    if (offline || !playback.current) return;
    const timeout = window.setTimeout(saveServerQueue, 700);
    return () => window.clearTimeout(timeout);
  }, [offline, playback.current?.id, playback.index, playback.queue, saveServerQueue]);

  useEffect(() => {
    if (offline) return;
    const interval = window.setInterval(() => { if (playbackRef.current.rendering) saveServerQueue(); }, 5_000);
    const onVisibility = () => { if (document.visibilityState === "hidden") saveServerQueue(); };
    document.addEventListener("visibilitychange", onVisibility);
    return () => { window.clearInterval(interval); document.removeEventListener("visibilitychange", onVisibility); };
  }, [offline, saveServerQueue]);

  useEffect(() => {
    if (!offline) {
      reloadHome();
      reloadLibrary();
    }
  }, [offline, reloadHome, reloadLibrary]);

  useEffect(() => {
    if (sleepAt === undefined) {
      setSleepRemaining(undefined);
      return;
    }
    const tick = () => {
      const remaining = Math.max(0, sleepAt - Date.now());
      setSleepRemaining(remaining);
      if (remaining > 0) return;
      setSleepAt(undefined);
      if (transportRef.current.isPlaying) void transportRef.current.toggle();
      setToast("Sleep timer ended playback");
    };
    tick();
    const interval = window.setInterval(tick, 1_000);
    return () => window.clearInterval(interval);
  }, [sleepAt]);

  // A play counts once it finished or ran past 30%, the line iOS uses, and
  // is logged the moment it crosses it so quitting mid-song still counts.
  // Seeking back near the start of the same song arms it again.
  const [playLog, setPlayLog] = useState(() => readPlayLog(profileScope));
  const loggedPlay = useRef<string | undefined>(undefined);
  useEffect(() => {
    const song = playback.current;
    // Another device's playback shown here is not a play on this computer.
    if (!song || remoteDevice || playback.mirroring) return;
    if (loggedPlay.current === song.id && playback.position < 2) loggedPlay.current = undefined;
    if (loggedPlay.current === song.id) return;
    const total = playback.duration || song.duration || 0;
    if (!total || playback.position < total * 0.3) return;
    loggedPlay.current = song.id;
    setPlayLog(logPlay(profileScope, song));
  }, [playback.current?.id, playback.duration, playback.mirroring, playback.position, profileScope, remoteDevice]);

  // Stats read Navidrome's record when the page opens, and fall back to the
  // log above when the server keeps none.
  const [statsSource, setStatsSource] = useState<StatsInput>();
  const [statsLoading, setStatsLoading] = useState(false);
  useEffect(() => {
    if (route.kind !== "stats") return;
    let active = true;
    setStatsLoading(true);
    const load = offline ? Promise.resolve(null) : invoke<ListeningSnapshot | null>("load_listening_snapshot").catch(() => null);
    void load.then((snapshot) => {
      if (!active) return;
      setStatsSource(statsInput(snapshot, readPlayLog(profileScope)));
      setStatsLoading(false);
    });
    return () => { active = false };
  }, [offline, profileScope, route.kind]);

  // Autoplay works as it does on iOS. When the last queued song starts, fetch
  // what follows it and say so in the queue. The songs join the queue at 75%,
  // so a crossfade or a gapless join has a next track to reach for, and a
  // song queued by hand before then still wins.
  const [autoplay, setAutoplay] = useState<AutoplayStatus>();
  const autoplayRef = useRef(autoplay);
  autoplayRef.current = autoplay;
  const lastInQueue = playback.index >= 0 && playback.index === playback.queue.length - 1;
  const autoplaySeed = settings.autoplay && !offline && playback.repeat === "off" && !remoteDevice && !playback.mirroring && !groupSession && lastInQueue
    ? playback.current
    : undefined;
  useEffect(() => {
    if (!autoplaySeed) {
      setAutoplay(undefined);
      return;
    }
    const seed = autoplaySeed.id;
    const label = `${autoplaySeed.title} Radio`;
    let active = true;
    setAutoplay({ state: "preparing", label, seed });
    invoke<RadioResult>("get_radio", { seedId: seed, title: label, count: 20 })
      .then((result) => {
        if (!active) return;
        const songs = result.songs.filter((song) => song.id !== seed);
        setAutoplay(songs.length ? { state: "ready", label, seed, songs } : { state: "unavailable", label, seed });
      })
      .catch(() => { if (active) setAutoplay({ state: "unavailable", label, seed }); });
    return () => { active = false };
    // Keyed on the seed song alone: a new fetch per song, not per render.
  }, [autoplaySeed?.id]);
  useEffect(() => {
    if (autoplay?.state !== "ready" || autoplay.seed !== playback.current?.id) return;
    if (!playback.duration || playback.position < playback.duration * 0.75) return;
    playbackRef.current.appendToQueue(autoplay.songs);
  }, [autoplay, playback.current?.id, playback.duration, playback.position]);

  // Start Radio after a song failed, as on iOS: seeded from the most recent
  // library song played, since an external id may not resolve again, and
  // falling back to the failed song itself.
  const startRadioAfterFailure = () => {
    const failedSong = playbackRef.current.current;
    if (!failedSong) return;
    const seed = recentlyPlayed.find((song) => song.id !== failedSong.id && !parseExternalSource(song.id)) ?? failedSong;
    const label = `${seed.title} Radio`;
    void invoke<RadioResult>("get_radio", { seedId: seed.id, title: label, count: 25 })
      .then((result) => {
        const controller = playbackRef.current;
        if (!controller.failed || controller.current?.id !== failedSong.id) return;
        const songs = result.songs.filter((song) => song.id !== seed.id && song.id !== failedSong.id);
        if (!songs.length) {
          setToast("No playable radio tracks are available");
          return;
        }
        controller.playQueue(songs, 0, true, 0, label);
      })
      .catch(() => setToast("No playable radio tracks are available"));
  };

  // The song ended before the 75% mark was reached, by a seek to the end or
  // a short track. Use what was already fetched, or fetch it now.
  queueExhausted.current = (last: SongSummary) => {
    if (offline) return;
    const prepared = autoplayRef.current;
    if (prepared?.state === "ready" && prepared.seed === last.id) {
      playbackRef.current.appendToQueue(prepared.songs, true);
      return;
    }
    void invoke<RadioResult>("get_radio", { seedId: last.id, title: `${last.title} Radio`, count: 20 })
      .then((result) => {
        const fresh = result.songs.filter((song) => song.id !== last.id);
        if (!fresh.length) return;
        playbackRef.current.appendToQueue(fresh, true);
        setToast("Autoplay continued with similar songs");
      })
      .catch(() => undefined);
  };

  useEffect(() => {
    let active = true;
    setSearchFilter("all");
    if (route.kind !== "search" || !query.trim()) {
      setSearchResults(emptySearch);
      setSearching(false);
      return () => { active = false };
    }
    if (offline) {
      const needle = query.trim().toLowerCase();
      const source = offlineSourceRef.current;
      const songs = [...source.starredSongs, ...source.downloads.map((item) => item.song)]
        .filter((song, index, all) => all.findIndex((item) => item.id === song.id) === index)
        .filter((song) => `${song.title} ${song.artist} ${song.album}`.toLowerCase().includes(needle));
      setSearchResults({
        songs,
        albums: source.albums.filter((album) => `${album.title} ${album.artist}`.toLowerCase().includes(needle)).slice(0, 60),
        artists: source.artists.filter((artist) => artist.name.toLowerCase().includes(needle)).slice(0, 60),
      });
      setSearching(false);
      return () => { active = false };
    }
    const timeout = window.setTimeout(() => {
      setSearching(true);
      setPageError(undefined);
      invoke<SearchResults>("search_catalog", { query: query.trim() })
        .then((results) => {
          if (!active) return;
          setSearchResults(dedupeSearchResults(results));
          const saved = readRecentSearches(searchHistoryKey);
          const next = [query.trim(), ...saved.filter((item) => item !== query.trim())].slice(0, 8);
          localStorage.setItem(searchHistoryKey, JSON.stringify(next));
          setRecentSearches(next);
        })
        .catch((reason) => active && setPageError(reasonMessage(reason, "Search is unavailable.")))
        .finally(() => active && setSearching(false));
    }, 220);
    return () => { active = false; window.clearTimeout(timeout); };
  }, [offline, query, route.kind, searchHistoryKey]);

  // Lyrics are searched in the local index, so this works offline too.
  const [lyricMatches, setLyricMatches] = useState<LyricsMatch[]>([]);
  useEffect(() => {
    const trimmed = query.trim();
    if (route.kind !== "search" || trimmed.length < 3) {
      setLyricMatches([]);
      return;
    }
    let active = true;
    const timeout = window.setTimeout(() => {
      invoke<LyricsMatch[]>("search_lyrics", { query: trimmed, limit: 50 })
        .then((matches) => { if (active) setLyricMatches(Array.isArray(matches) ? matches : []); })
        .catch(() => { if (active) setLyricMatches([]); });
    }, 220);
    return () => { active = false; window.clearTimeout(timeout); };
  }, [query, route.kind]);

  const [lyricsIndex, setLyricsIndex] = useState<LyricsIndexStatus>();
  useEffect(() => {
    let dispose: (() => void) | undefined;
    let active = true;
    void listen<LyricsIndexStatus>("lyrics-index-progress", ({ payload }) => setLyricsIndex(payload))
      .then((unlisten) => { if (active) dispose = unlisten; else unlisten(); })
      .catch(() => undefined);
    return () => { active = false; dispose?.(); };
  }, []);
  useEffect(() => {
    if (route.kind !== "settings") return;
    void invoke<LyricsIndexStatus>("lyrics_index_status").then(setLyricsIndex).catch(() => undefined);
  }, [route.kind]);

  // Index the server's lyrics in the background once per sign-in, as iOS
  // does after a library sync. It resumes where the last run stopped.
  const lyricsCrawlStarted = useRef(false);
  useEffect(() => {
    if (offline || lyricsCrawlStarted.current) return;
    lyricsCrawlStarted.current = true;
    void invoke("start_lyrics_index", { rebuild: false }).catch(() => { lyricsCrawlStarted.current = false; });
  }, [offline]);

  useEffect(() => {
    if (!("id" in route) || !(route.kind === "album" || route.kind === "playlist" || route.kind === "artist")) {
      setDetail(undefined);
      return;
    }
    let active = true;
    if (offline) {
      const saved = cachedDetail(library, route.kind, route.id);
      if (saved) {
        setDetail(saved);
        setPageError(undefined);
      } else {
        setDetail(undefined);
        setPageError("This page has not been saved for offline browsing. Reconnect to open it.");
      }
      setPageLoading(false);
      return () => { active = false };
    }
    const command = route.kind === "album" ? "get_album" : route.kind === "playlist" ? "get_playlist" : "get_artist";
    setPageLoading(true);
    setPageError(undefined);
    invoke<DetailState>(command, { id: route.id })
      .then((value) => {
        if (!active) return;
        setDetail(value);
        cacheDetail(library, route.kind, route.id, value);
      })
      .catch((reason) => active && setPageError(reasonMessage(reason, "This page could not be opened.")))
      .finally(() => active && setPageLoading(false));
    return () => { active = false };
  }, [library, offline, route]);

  useEffect(() => {
    if (route.kind !== "radio") { setRadioData(undefined); return; }
    let active = true;
    if (offline) {
      const songs = offlineSourceRef.current.downloads.map((item) => item.song).filter((song) => song.id !== route.id).slice(0, 60);
      setRadioData({ title: route.title, songs });
      setPageLoading(false);
      if (!songs.length) setPageError("Radio needs a server connection or more downloaded tracks.");
      return () => { active = false };
    }
    setPageLoading(true);
    invoke<RadioResult>("get_radio", { seedId: route.id, title: route.title, count: 60 })
      .then((value) => active && setRadioData(value))
      .catch((reason) => active && setPageError(reasonMessage(reason, "Radio could not be started.")))
      .finally(() => active && setPageLoading(false));
    return () => { active = false };
  }, [offline, route]);

  useEffect(() => {
    if (!playback.current) { setLyrics(emptyLyrics); return; }
    if (offline) { setLyrics(emptyLyrics); setLyricsLoading(false); return; }
    let active = true;
    setLyricsLoading(true);
    invoke<LyricsResult>("get_lyrics", {
      id: playback.current.id,
      source: settings.lyricsSource,
      artist: playback.current.artist,
      title: playback.current.title,
      album: playback.current.album,
      duration: playback.current.duration,
      song: playback.current,
    })
      .then((value) => active && setLyrics(value?.lines ? value : emptyLyrics))
      .catch(() => active && setLyrics(emptyLyrics))
      .finally(() => active && setLyricsLoading(false));
    return () => { active = false };
  }, [offline, playback.current?.id, settings.lyricsSource]);

  useEffect(() => {
    if (!toast) return;
    const timeout = window.setTimeout(() => setToast(undefined), 2600);
    return () => window.clearTimeout(timeout);
  }, [toast]);

  /// Keyboard and native-menu actions that need this render's state. Assigned
  /// at the end of every render, so the window listener never goes stale.
  const keyActions = useRef<{
    toggleLyrics: () => void;
    likeCurrent: () => void;
    canSelectAll: () => boolean;
    selectAll: () => void;
    canRemoveSelection: () => boolean;
    removeSelection: () => void;
  }>({
    toggleLyrics: () => undefined,
    likeCurrent: () => undefined,
    canSelectAll: () => false,
    selectAll: () => undefined,
    canRemoveSelection: () => false,
    removeSelection: () => undefined,
  });

  const openPanel = useCallback((mode: ContextPanelMode) => {
    setPanelMode((current) => {
      const next = current === mode ? undefined : mode;
      if (next) {
        localStorage.setItem("splice.panel", next);
      }
      return next;
    });
  }, []);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      const target = event.target instanceof HTMLElement ? event.target : null;
      const editable = target?.matches("input, textarea, [contenteditable='true']");
      // Space activates whatever control has focus. Only an unfocused surface —
      // or a track row, which spec 7 gives to the transport — reaches playback.
      const spaceOwner = target?.closest("button, a[href], select, summary, [role='menuitem'], [role='tab']");
      const rowOwner = Boolean(target?.closest("[role='row']"));
      const modifier = event.metaKey || event.ctrlKey;
      const altShift = event.altKey && event.shiftKey && !modifier;
      const actions = keyActions.current;
      const run = (action: () => void) => { event.preventDefault(); action(); };
      if (modifier && event.key.toLowerCase() === "k") run(() => document.querySelector<HTMLInputElement>("[data-search-input]")?.focus());
      else if (modifier && event.key === "/") run(() => setShortcutsOpen((value) => !value));
      else if (!editable && event.code === "Space" && (!spaceOwner || rowOwner)) run(() => void transportRef.current.toggle());
      else if ((event.altKey && !event.shiftKey && event.key === "ArrowLeft") || (event.metaKey && event.key === "[")) run(goBack);
      else if ((event.altKey && !event.shiftKey && event.key === "ArrowRight") || (event.metaKey && event.key === "]")) run(goForward);
      else if ((modifier && event.shiftKey && event.key.toLowerCase() === "q") || (altShift && event.code === "KeyQ")) run(() => openPanel("queue"));
      else if (altShift && event.code === "KeyJ") run(actions.toggleLyrics);
      else if (altShift && event.code === "KeyR") run(() => openPanel("nowPlaying"));
      else if (altShift && event.code === "KeyH") run(() => navigate({ kind: "home" }));
      else if (altShift && event.code === "Digit0") run(() => navigate({ kind: "library" }));
      else if (altShift && event.code === "KeyP") run(() => setCreatePlaylistOpen(true));
      else if (altShift && event.code === "KeyB") run(actions.likeCurrent);
      else if (modifier && event.key === ",") run(() => navigate({ kind: "settings" }));
      else if (!editable && modifier && !event.shiftKey && event.key.toLowerCase() === "s") run(() => transportRef.current.setShuffle((value) => !value));
      else if (!editable && modifier && !event.shiftKey && event.key.toLowerCase() === "r") run(() => transportRef.current.cycleRepeat());
      else if (!editable && modifier && event.key.toLowerCase() === "a" && actions.canSelectAll()) run(actions.selectAll);
      else if (!editable && (event.key === "Delete" || event.key === "Backspace") && actions.canRemoveSelection()) run(actions.removeSelection);
      else if (!editable && modifier && event.key === "ArrowRight") run(() => transportRef.current.next());
      else if (!editable && modifier && event.key === "ArrowLeft") run(() => transportRef.current.previous());
      else if (!editable && event.shiftKey && !modifier && event.key === "ArrowRight") run(() => transportRef.current.seek(transportRef.current.positionNow() + 5));
      else if (!editable && event.shiftKey && !modifier && event.key === "ArrowLeft") run(() => transportRef.current.seek(Math.max(0, transportRef.current.positionNow() - 5)));
      else if (!editable && modifier && event.key === "ArrowUp") run(() => transportRef.current.setVolume(transportRef.current.volume + 0.05));
      else if (!editable && modifier && event.key === "ArrowDown") run(() => transportRef.current.setVolume(transportRef.current.volume - 0.05));
      else if (!editable && (event.key === "F11" || (event.ctrlKey && event.metaKey && event.key.toLowerCase() === "f"))) run(() => { if (transportRef.current.current) setFullPlayer((value) => !value); });
      else if (!editable && event.key === "?") run(() => setShortcutsOpen((value) => !value));
      else if (event.key === "Escape") {
        if (accountOpen) setAccountOpen(false);
        else if (searchFocused) (document.activeElement as HTMLElement | null)?.blur();
        else if (sortMenuOpen) setSortMenuOpen(false);
        else if (shortcutsOpen) setShortcutsOpen(false);
        else if (signOutConfirm) setSignOutConfirm(false);
        else if (clearDownloadsConfirm) setClearDownloadsConfirm(false);
        else if (collectionMenu) setCollectionMenu(undefined);
        else if (playlistDelete) setPlaylistDelete(undefined);
        else if (playlistEdit) setPlaylistEdit(undefined);
        else if (createPlaylistOpen) setCreatePlaylistOpen(false);
        else if (trackMenu) setTrackMenu(undefined);
        else if (selectedIds.size) setSelectedIds(new Set());
        else if (fullPlayer) setFullPlayer(false);
        else if (panelMode) setPanelMode(undefined);
        else if (route.kind === "search" && query) setQuery("");
        else if (route.kind === "search") goBack();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [accountOpen, clearDownloadsConfirm, collectionMenu, createPlaylistOpen, fullPlayer, goBack, goForward, navigate, openPanel, panelMode, playlistDelete, playlistEdit, query, route.kind, searchFocused, selectedIds, shortcutsOpen, signOutConfirm, sortMenuOpen, trackMenu]);

  // The native menu owns no behaviour of its own: it forwards to the same
  // handlers the keyboard and the player bar use.
  useEffect(() => {
    let dispose: (() => void) | undefined;
    void listen<string>("menu-command", ({ payload }) => {
      const controller = transportRef.current;
      if (payload === "preferences") navigate({ kind: "settings" });
      else if (payload === "search") navigate({ kind: "search" });
      else if (payload === "playpause") void controller.toggle();
      else if (payload === "next") controller.next();
      else if (payload === "previous") controller.previous();
      else if (payload === "shuffle") controller.setShuffle((value) => !value);
      else if (payload === "repeat") controller.cycleRepeat();
      else if (payload === "queue") openPanel("queue");
      else if (payload === "lyrics") keyActions.current.toggleLyrics();
      else if (payload === "devices") openPanel("connect");
      else if (payload === "fullplayer") setFullPlayer((value) => Boolean(controller.current) && !value);
      else if (payload === "shortcuts") setShortcutsOpen(true);
    }).then((unlisten) => { dispose = unlisten; }).catch(() => undefined);
    return () => dispose?.();
  }, [navigate, openPanel]);

  const applyHandoff = useCallback(async (handoff: NonNullable<ConnectCommand["handoff"]>, label = "Splynt Connect", startAt?: number) => {
    const active = await invoke<SongSummary[]>("get_songs_by_ids", { ids: [handoff.currentTrackID] });
    if (!active[0]) throw new Error("The active track was not available on this server.");
    playbackRef.current.playQueue(active, 0, handoff.isPlaying, startAt ?? handoff.position, label);
    if (handoff.trackIDs.length <= 1) return;
    const expectedTrackId = active[0].id;
    const songs = await invoke<SongSummary[]>("get_songs_by_ids", { ids: handoff.trackIDs.slice(0, 1000) });
    if (playbackRef.current.current?.id === expectedTrackId) {
      playbackRef.current.replaceQueuePreservingCurrent(songs);
    }
  }, []);

  const applyConnectCommand = useCallback(async (command: ConnectCommand) => {
    const controller = playbackRef.current;
    const leaderOffset = (leaderID: string) => clockOffsets.current[leaderID];

    if (command.name === "groupAccept" || command.name === "groupDecline") {
      const reply = command.groupReply;
      const invite = pendingInvite.current;
      if (!reply || !invite || invite.sessionId !== reply.sessionID) return;
      if (!invite.awaiting.delete(reply.deviceID)) return;
      if (command.name === "groupAccept") invite.accepted.push(reply.deviceID);
      else invite.declined.set(reply.deviceID, reply.reason ?? "declined");
      return;
    }
    // A controller's own player is not the one these commands are meant for.
    // Applying one here would act on a mirror with no audio behind it.
    if (remoteRef.current && ["play", "pause", "toggle", "previous", "next", "seek", "setShuffle", "setRepeat",
      "setVolume", "skipTo", "enqueue", "playNext"].includes(command.name)) return;
    if (command.name === "setShuffle" && command.value !== undefined) return controller.setShuffle(command.value === 1);
    if (command.name === "setRepeat" && command.value !== undefined) {
      return controller.setRepeatMode(command.value === 2 ? "one" : command.value === 1 ? "all" : "off");
    }
    if (command.name === "setVolume" && command.value !== undefined) return controller.setVolume(command.value);
    if (command.name === "skipTo" && command.queueItem) {
      const { index, trackID } = command.queueItem;
      const target = controller.queue[index]?.id === trackID ? index : controller.queue.findIndex((song) => song.id === trackID);
      if (target >= 0) controller.skipTo(target);
      return;
    }
    if ((command.name === "enqueue" || command.name === "playNext") && command.tracks) {
      const songs = (await resolveSongs(command.tracks.trackIDs)).filter((song): song is SongSummary => Boolean(song));
      const now = playbackRef.current;
      if (command.name === "enqueue") songs.forEach((song) => now.enqueue(song));
      // Each Play next goes straight after the current track, so the last one
      // goes in first to keep the order the sender gave.
      else [...songs].reverse().forEach((song) => now.playNext(song));
      return;
    }
    if (command.name === "play" && !controller.isPlaying) return void await controller.toggle();
    if (command.name === "pause" && controller.isPlaying) return void await controller.toggle();
    if (command.name === "toggle") return void await controller.toggle();
    if (command.name === "previous") return controller.previous();
    if (command.name === "next") return controller.next();
    if (command.name === "seek" && command.value !== undefined) return controller.seek(command.value);
    // An incoming transfer means this device is the one playing now, so it
    // stops being a remote for anyone else — otherwise the bar would keep
    // claiming "Playing on <device>" over its own audio.
    if (command.name === "handoff" && command.handoff) {
      if (remoteRef.current) logConnectEvent("remote_end", { peer: remoteRef.current.id, reason: "handoff received" });
      remoteRef.current = undefined;
      mirrorRows.current = undefined;
      setRemoteDevice(undefined);
      return void await applyHandoff(command.handoff);
    }

    if (command.name === "groupJoin" && command.groupJoin) {
      const { group, handoff } = command.groupJoin;
      const localId = connectRef.current.localDeviceId ?? "";
      /// A join this device will not honour has to say so. Returning silently
      /// left the leader unable to tell a refusal from a frame that never
      /// arrived, so it kept addressing a device that never joined.
      const decline = (reason: string) => {
        logConnectEvent("group_declined", { session: group.id, reason });
        void sendGroupFrame(group.leaderID, {
          name: "groupDecline",
          groupReply: { sessionID: group.id, deviceID: localId, revision: group.revision ?? 0, reason },
        });
      };
      // An output belongs to one session at a time.
      const existing = groupRef.current;
      if (existing && existing.id !== group.id) return decline("in another session");
      remoteRef.current = undefined;
      mirrorRows.current = undefined;
      setRemoteDevice(undefined);
      // Start where the leader's clock has reached by now, not where it was
      // when the frame left. iOS has always done this; the desktop used the
      // raw handoff position and so began every group session — and, because
      // the leader resends a join at each track change, every track — already
      // behind by the whole transit plus the time spent resolving the track.
      const offset = leaderOffset(group.leaderID);
      const joinAt = projectedGroupPosition(group, offset);
      groupRevision.current = group.revision ?? 0;
      logConnectEvent("group_joined", {
        session: group.id,
        startAt: Math.round(joinAt * 1000) / 1000,
        skew: Date.now() - group.sentAt,
        offset: offset === undefined ? "none" : Math.round(offset * 10) / 10,
        queued: handoff.trackIDs.length,
      });
      try {
        await applyHandoff(handoff, "Group Session", joinAt);
      } catch {
        // The track could not be resolved here. v1 let this throw out of the
        // command loop, which both lost every command behind it and told the
        // leader nothing.
        return decline("track unresolved");
      }
      setGroupSession({ id: group.id, leaderID: group.leaderID, memberIDs: [] });
      setPanelMode("connect");
      // Only now is this device rendering the session, so only now may it
      // claim membership.
      void sendGroupFrame(group.leaderID, {
        name: "groupAccept",
        groupReply: { sessionID: group.id, deviceID: localId, revision: group.revision ?? 0 },
      });
      return;
    }

    if (command.name === "groupSync" && command.group && groupRef.current?.id === command.group.id) {
      const group = command.group;
      // A revision older than the one already applied is a controller that has
      // not caught up. Honouring it would undo what the session did since.
      const revision = group.revision ?? 0;
      if (revision < groupRevision.current) {
        logConnectEvent("group_stale_frame", { session: group.id, frame: revision, applied: groupRevision.current });
        return;
      }
      groupRevision.current = revision;
      const offset = leaderOffset(group.leaderID);
      const target = projectedGroupPosition(group, offset);
      const skew = Date.now() - group.sentAt;
      if (controller.current?.id !== group.trackID) {
        const trackIndex = controller.queue.findIndex((song) => song.id === group.trackID);
        // The leader has moved to a track this device does not hold yet. Wait
        // for the groupJoin that carries it. Falling through here used to seek
        // whatever was playing locally to the leader's position in a different
        // song, which is what made a follower on the wrong track jump around.
        if (trackIndex < 0) {
          logConnectEvent("group_track_missing", { session: group.id, track: group.trackID });
          return;
        }
        noteGroupTrackChange(group.id);
        controller.endConvergence();
        controller.playQueue(controller.queue, trackIndex, group.isPlaying, target, "Group Session");
        return;
      }
      // Both playheads are read exactly and the leader's clock is measured, so
      // this is real drift rather than two stale samples plus whatever the two
      // wall clocks disagree by. That is what makes correcting by rate worth
      // doing: v1's whole 1.25 s window now converges silently instead.
      const drift = controller.positionNow() - target;
      const correction = driftCorrection(drift);
      if (correction.kind === "converge") {
        controller.convergeRate(correction.rate, correction.seconds);
      } else {
        // Inside the deadband, a running nudge has done its job and normal
        // speed resumes now rather than at its deadline.
        controller.endConvergence();
        if (correction.kind === "seek") controller.seek(target);
      }
      noteGroupSample(group.id, { drift, skew, offset, correction: correction.kind });
      if (group.isPlaying !== controller.isPlaying) {
        if (!group.isPlaying) controller.endConvergence();
        await controller.toggle();
      }
      return;
    }

    if (command.name === "groupLeave") {
      controller.endConvergence();
      groupRevision.current = 0;
      endGroupDiagnostics("leader-ended");
      logConnectEvent("group_left", { session: command.group?.id ?? "unknown" });
      setGroupSession(undefined);
    }
  }, [applyHandoff, resolveSongs]);

  const applyConnectCommandRef = useRef(applyConnectCommand);
  applyConnectCommandRef.current = applyConnectCommand;

  /// Starts mirroring `peer`, the device that is playing. Nothing plays here:
  /// the player shows that device's track, queue and clock, and every control
  /// is sent to it. See "Following the active device" in WIRE-V1.md.
  const beginFollow = useCallback((peer: ConnectPeer, trigger: string, snapshot?: ConnectSnapshot) => {
    const previous = remoteRef.current;
    if (previous?.id === peer.id) return;
    logConnectEvent("remote_begin", {
      peer: peer.id, peerName: peer.name, platform: peer.platform, trigger,
      movedFrom: previous ? "another device" : "here",
    });
    const device = { id: peer.id, name: deviceName(peer), platform: peer.platform };
    remoteRef.current = device;
    setRemoteDevice(device);
    mirrorRows.current = undefined;
    queueApplying.current = undefined;
    queueRequested.current = undefined;
    renderedHere.current = false;
    if (snapshot) mirrorPeerRef.current(peer, snapshot);
  }, []);

  /// Stops following. The mirrored queue stays loaded here, paused where the
  /// device had reached, as Spotify leaves it.
  const endFollow = useCallback((reason: string) => {
    const followed = remoteRef.current;
    if (!followed) return;
    logConnectEvent("remote_end", { peer: followed.id, reason });
    remoteRef.current = undefined;
    setRemoteDevice(undefined);
    mirrorRows.current = undefined;
    queueApplying.current = undefined;
    trackGuard.current = undefined;
    const controller = playbackRef.current;
    if (controller.isMirroring()) controller.endMirror({ play: false, position: controller.positionNow() });
  }, []);

  /// Shows the followed device's current track here, straight from its frame
  /// so the bar changes with no server round trip, then with the full song
  /// once it resolves.
  const mirrorCurrentTrack = useCallback((peer: ConnectPeer) => {
    const frame = peer.playback;
    const id = frame.trackID;
    if (!id) return;
    const cached = songCache.current.get(id);
    const song: SongSummary = cached ?? {
      id, title: frame.title ?? "", artist: frame.artist ?? "", album: frame.album ?? "",
      coverArt: frame.coverArtID, duration: frame.duration || undefined,
    };
    mirrorRows.current = undefined;
    playbackRef.current.mirrorQueue([song], 0, frame.contextLabel ?? deviceName(peer));
    if (cached) return;
    const generation = ++mirrorGeneration.current;
    void resolveSongs([id]).then(([resolved]) => {
      const controller = playbackRef.current;
      if (!resolved || generation !== mirrorGeneration.current || !controller.isMirroring() || controller.current?.id !== id) return;
      controller.mirrorQueue(controller.queue.map((item) => item.id === id ? resolved : item), controller.index);
    });
  }, [resolveSongs]);

  /// Replaces the mirrored queue with the one the device sent, current track
  /// first, then the rest, leaving the current track alone if the device
  /// has moved on while the songs resolved.
  const applyPeerQueue = useCallback(async (peer: ConnectPeer, queue: ConnectQueue) => {
    const key = `${peer.id}:${queue.revision}`;
    if (queueApplying.current === key) return;
    queueApplying.current = key;
    const generation = ++mirrorGeneration.current;
    const currentRow = queue.index - queue.offset;
    const currentId = queue.trackIDs[currentRow];
    if (currentId) await resolveSongs([currentId]);
    const resolved = await resolveSongs(queue.trackIDs);
    const controller = playbackRef.current;
    if (generation !== mirrorGeneration.current || remoteRef.current?.id !== peer.id || !controller.isMirroring()) {
      if (queueApplying.current === key) queueApplying.current = undefined;
      return;
    }
    const songs: SongSummary[] = [];
    const fullIndexes: number[] = [];
    let index = -1;
    resolved.forEach((song, row) => {
      if (!song) return;
      if (row === currentRow) index = songs.length;
      songs.push(song);
      fullIndexes.push(queue.offset + row);
    });
    const latest = snapshotRef.current?.peers.find((candidate) => candidate.id === peer.id)?.playback;
    if (index < 0 || (latest?.trackID && songs[index].id !== latest.trackID)) {
      queueApplying.current = undefined;
      return;
    }
    mirrorRows.current = { peerId: peer.id, revision: queue.revision, fullIndexes };
    controller.mirrorQueue(songs, index, queue.contextLabel ?? latest?.contextLabel ?? controller.contextLabel);
  }, [resolveSongs]);

  const requestPeerQueue = useCallback((peerId: string, revision: number) => {
    const requested = queueRequested.current;
    if (requested?.peerId === peerId && requested.revision === revision && Date.now() - requested.at < 3_000) return;
    queueRequested.current = { peerId, revision, at: Date.now() };
    void invoke("send_connect_command", { peerId, command: { name: "queueRequest" } }).catch(() => undefined);
  }, []);

  /// Brings the mirror up to date with one frame from the followed device.
  const mirrorPeer = useCallback((peer: ConnectPeer, snapshot: ConnectSnapshot) => {
    const controller = playbackRef.current;
    const frame = peer.playback;
    // A skip just sent, and the mirror already shows where it goes; this
    // frame was written before the device moved. While the mirror still shows
    // the old track, the frame changes nothing visible and is applied.
    if (trackGuardIgnores(trackGuard.current, frame.trackID) && controller.current?.id !== frame.trackID) return;
    trackGuard.current = undefined;
    if (hasRemoteControl(frame) && frame.queueRevision !== undefined) {
      const queue = snapshot.queues?.[peer.id];
      if (queue && queue.revision === frame.queueRevision) {
        const rows = mirrorRows.current;
        if (rows?.peerId !== peer.id || rows.revision !== queue.revision) void applyPeerQueue(peer, queue);
      } else {
        requestPeerQueue(peer.id, frame.queueRevision);
      }
    }
    if (frame.trackID && controller.current?.id !== frame.trackID) {
      const rows = mirrorRows.current;
      const row = rows?.peerId === peer.id && frame.queueIndex !== undefined ? rows.fullIndexes.indexOf(frame.queueIndex) : -1;
      if (row >= 0 && controller.queue[row]?.id === frame.trackID) controller.mirrorQueue(controller.queue, row);
      else mirrorCurrentTrack(peer);
    }
    if (!playbackRef.current.isMirroring()) return;
    playbackRef.current.mirrorState({
      isPlaying: frame.isPlaying,
      position: projectedPeerPosition(frame, peer.updatedAt),
      duration: frame.duration,
      shuffle: frame.shuffle,
      repeat: repeatFromWire(frame.repeatMode),
      volume: frame.volume,
    });
  }, [applyPeerQueue, mirrorCurrentTrack, requestPeerQueue]);
  const mirrorPeerRef = useRef(mirrorPeer);
  mirrorPeerRef.current = mirrorPeer;

  /// Applies the follow rules to one snapshot: start following the device
  /// that is playing, switch, or let go, then mirror whatever is followed.
  const followSnapshot = useCallback((snapshot: ConnectSnapshot) => {
    if (!snapshot.isAvailable) return;
    const now = Date.now() / 1000;
    peerActivity.current = notePeerActivity(peerActivity.current, snapshot.peers, now);
    if (droppedPeer.current && now >= droppedPeer.current.until) droppedPeer.current = undefined;
    const controller = playbackRef.current;
    const decision = decideFollow({
      deviceID: snapshot.localDeviceId,
      rendering: controller.rendering,
      hasTrack: Boolean(controller.current),
      inSession: Boolean(groupRef.current),
      stoppedAt: localStoppedAt.current,
      followingID: remoteRef.current?.id,
    }, followPeers(snapshot.peers, peerActivity.current), droppedPeer.current, now);
    if (decision.action === "detach") {
      const followed = remoteRef.current;
      if (decision.reason === "peer left the network" && followed) {
        droppedPeer.current = { id: followed.id, until: now + DROP_COOLDOWN_SECONDS };
        setToast(`${followed.name} is no longer on this network`);
      }
      endFollow(decision.reason);
    } else if (decision.action === "follow") {
      const peer = snapshot.peers.find((candidate) => candidate.id === decision.peerID);
      if (peer) beginFollow(peer, remoteRef.current ? "switched" : "adopted");
    }
    const followed = remoteRef.current && snapshot.peers.find((peer) => peer.id === remoteRef.current?.id);
    if (followed) mirrorPeer(followed, snapshot);
  }, [beginFollow, endFollow, mirrorPeer]);
  const followSnapshotRef = useRef(followSnapshot);
  followSnapshotRef.current = followSnapshot;

  useEffect(() => {
    let active = true;
    let inFlight = false;
    let missed = false;
    async function poll() {
      if (inFlight) { missed = true; return; }
      inFlight = true;
      try {
        let snapshot: ConnectSnapshot | undefined;
        try {
          snapshot = await invoke<ConnectSnapshot>("connect_snapshot");
        } catch {
          // Only a failed snapshot means Connect itself is unreachable. A
          // command that could not be applied used to land here too and put
          // "Local discovery unavailable" on a panel that was working fine.
          if (active) setConnectState((value) => ({ ...value, isAvailable: false, commands: [] }));
          return;
        }
        if (!active) return;
        // A snapshot that came back without its arrays is a tick to skip, not
        // evidence the network went away. The previous catch-all swallowed
        // this shape silently and reported Connect as unavailable instead.
        if (!snapshot || !Array.isArray(snapshot.peers)) return;
        clockOffsets.current = snapshot.clockOffsets ?? {};
        snapshotRef.current = snapshot;
        setConnectState({ ...snapshot, commands: [] });
        followSnapshotRef.current(snapshot);
        for (const command of snapshot.commands ?? []) {
          if (!active) break;
          try {
            await applyConnectCommandRef.current(command);
          } catch (reason) {
            // The Rust side has already drained the batch, so a throw that
            // escaped this loop lost every command behind it for good.
            setPageError(reasonMessage(reason, "A command from another device could not be applied."));
          }
        }
      } finally {
        inFlight = false;
        if (missed && active) { missed = false; void poll(); }
      }
    }
    void poll();
    const interval = window.setInterval(poll, 1000);
    // The reader thread knows the moment a command frame lands, or a peer's
    // frame changes what this computer shows. Waiting for the next tick cost
    // up to a second each time; the snapshot stays the delivery path so
    // nothing is lost.
    const disposers: Array<() => void> = [];
    for (const event of ["connect-commands-pending", "connect-peers-changed"]) {
      void listen(event, () => void poll())
        .then((unlisten) => { if (active) disposers.push(unlisten); else unlisten(); })
        .catch(() => undefined);
    }
    return () => { active = false; window.clearInterval(interval); disposers.forEach((dispose) => dispose()); };
  }, []);

  /// Bumped whenever the queue this computer plays from changes, as
  /// `queueRevision` on the wire, so a follower knows to fetch it again.
  const queueRevision = useRef(0);
  const lastPublished = useRef({ position: 0, at: 0, playing: false });
  const publish = useCallback(() => {
    const controller = playbackRef.current;
    const session = groupRef.current;
    const following = remoteRef.current;
    const position = controller.positionNow();
    lastPublished.current = { position, at: Date.now(), playing: controller.rendering };
    void invoke("publish_connect_playback", {
      // While following, this computer plays nothing and says so. Publishing
      // the mirror would have it look like a second device playing the
      // same song.
      playback: following ? { isPlaying: false, position: 0, duration: 0 } : {
        trackID: controller.current?.id, title: controller.current?.title, artist: controller.current?.artist,
        album: controller.current?.album, coverArtID: controller.current?.coverArt, isPlaying: controller.rendering,
        position, duration: controller.duration,
        shuffle: controller.shuffle, repeatMode: controller.repeat, volume: controller.volume,
        queueRevision: queueRevision.current,
        queueIndex: controller.current ? controller.index : undefined,
        queueLength: controller.queue.length,
        contextLabel: controller.current ? controller.contextLabel : undefined,
      },
      // Published so another device can see, before it tries to take this
      // one, that it is already rendering a session or already driving a
      // third device.
      commitment: {
        sessionID: session?.id,
        leaderID: session?.leaderID,
        revision: groupRevision.current,
        controllingPeerID: following?.id,
      },
    }).catch(() => undefined);
  }, []);

  /// A change another device should see goes out within about 120 ms, several
  /// changes in that window as one frame. The two-second interval below and
  /// the transport's three-second heartbeat cover everything else.
  const publishTimer = useRef<number | undefined>(undefined);
  const requestPublish = useCallback(() => {
    if (publishTimer.current !== undefined) return;
    publishTimer.current = window.setTimeout(() => {
      publishTimer.current = undefined;
      publish();
    }, 120);
  }, [publish]);

  useEffect(() => {
    publish();
    const interval = window.setInterval(publish, 2000);
    return () => { window.clearInterval(interval); if (publishTimer.current !== undefined) window.clearTimeout(publishTimer.current); };
  }, [publish]);

  useEffect(() => {
    requestPublish();
  }, [playback.current?.id, playback.rendering, playback.shuffle, playback.repeat, playback.volume, remoteDevice?.id, groupSession?.id, requestPublish]);

  // A seek here moves the clock in a way the last frame cannot predict.
  useEffect(() => {
    const last = lastPublished.current;
    const expected = last.playing ? last.position + (Date.now() - last.at) / 1000 : last.position;
    if (!remoteRef.current && Math.abs(playbackRef.current.positionNow() - expected) > 1.5) requestPublish();
  }, [playback.position, requestPublish]);

  useEffect(() => {
    const controller = playbackRef.current;
    if (controller.isMirroring() || !controller.current) {
      void invoke("publish_connect_queue", { queue: null }).catch(() => undefined);
      return;
    }
    queueRevision.current += 1;
    void invoke("publish_connect_queue", {
      queue: {
        revision: queueRevision.current,
        index: controller.index,
        trackIDs: controller.queue.map((song) => song.id),
        contextLabel: controller.contextLabel,
      },
    }).catch(() => undefined);
    requestPublish();
  }, [playback.queue, playback.index, playback.mirroring, playback.contextLabel, requestPublish]);

  async function sendRemote(peerId: string, command: ConnectCommand) {
    try { await invoke("send_connect_command", { peerId, command }); }
    catch (reason) { setPageError(reasonMessage(reason, "That device is no longer available.")); }
  }

  /// The same send without the error banner, for frames this device emits on a
  /// timer rather than because someone pressed something. The group leader sends
  /// once a second per member; routing that through `sendRemote` put a visible
  /// error on screen every second for as long as a member stayed unreachable.
  async function sendGroupFrame(peerId: string, command: ConnectCommand) {
    try {
      await invoke("send_connect_command", { peerId, command });
      groupSendFailures.current.delete(peerId);
      return true;
    } catch {
      groupSendFailures.current.set(peerId, (groupSendFailures.current.get(peerId) ?? 0) + 1);
      return false;
    }
  }

  useEffect(() => {
    const interval = window.setInterval(() => {
      const group = groupRef.current;
      const controller = playbackRef.current;
      const localId = connectRef.current.localDeviceId;
      if (!group || group.leaderID !== localId || !controller.current) return;
      const trackKey = `${group.id}:${controller.current.id}`;
      // A track change is an accepted state change, so it advances the
      // session. Sync frames in between carry the revision they belong to.
      if (trackKey !== lastGroupTrack.current) groupRevision.current += 1;
      const state: ConnectGroup = { id: group.id, leaderID: group.leaderID, trackID: controller.current.id, position: controller.positionNow(), isPlaying: controller.isPlaying, sentAt: Date.now(), revision: groupRevision.current };
      const frame: ConnectCommand = trackKey !== lastGroupTrack.current
        ? { name: "groupJoin", groupJoin: { group: state, handoff: { trackIDs: controller.queue.slice(0, 1000).map((song) => song.id), currentTrackID: controller.current.id, position: state.position, isPlaying: controller.isPlaying } } }
        : { name: "groupSync", group: state };
      if (frame.name === "groupJoin") lastGroupTrack.current = trackKey;
      void Promise.all(group.memberIDs.map((peerId) => sendGroupFrame(peerId, frame))).then((results) => {
        const lost = group.memberIDs.filter((peerId, index) => !results[index] && (groupSendFailures.current.get(peerId) ?? 0) >= 3);
        if (!lost.length) return;
        for (const peerId of lost) groupSendFailures.current.delete(peerId);
        logConnectEvent("group_member_dropped", { session: group.id, dropped: lost.length });
        setGroupSession((value) => value && value.id === group.id
          ? { ...value, memberIDs: value.memberIDs.filter((peerId) => !lost.includes(peerId)) }
          : value);
        setToast(lost.length === 1 ? "A device left the group session" : `${lost.length} devices left the group session`);
      });
    }, 1000);
    return () => window.clearInterval(interval);
  }, []);

  const retryConnection = useCallback(async () => {
    if (reconnecting) return;
    setReconnecting(true);
    setConnection({ status: "offline", message: "Reconnecting…" });
    try {
      const restored = await invoke<ConnectedLibrary | null>("restore_session");
      if (!restored) throw new Error("No saved login is available.");
      setConnection({ status: "online" });
      onConnectionRestored(restored);
    } catch (reason) {
      setConnection({ status: "offline", message: reasonMessage(reason, "The server is still unavailable.") });
    } finally {
      setReconnecting(false);
    }
  }, [onConnectionRestored, reconnecting]);

  async function leaveSession(forgetSavedLogin: boolean) {
    if (leavingSession) return;
    setLeavingSession(true);
    try {
      await invoke("disconnect_server", { forgetSavedLogin });
      onSignedOut();
    } catch (reason) {
      setPageError(reasonMessage(reason, forgetSavedLogin ? "Splynt could not remove the saved login." : "Splynt could not switch accounts."));
      setSignOutConfirm(false);
    } finally {
      setLeavingSession(false);
    }
  }

  useEffect(() => {
    const onOnline = () => {
      if (connection.status === "offline" && !settings.offlineMode && !reconnecting) void retryConnection();
    };
    window.addEventListener("online", onOnline);
    return () => window.removeEventListener("online", onOnline);
  }, [connection.status, retryConnection, settings.offlineMode]);

  /// Every user-initiated play routes through here so that "playing on another
  /// device" behaves the way it does on iOS: the remote device receives the new
  /// queue, and this device loads the same queue *paused*. Keeping the local
  /// queue in step is what lets the player bar, the queue view and "Play here"
  /// stay truthful about what is playing while the audio is somewhere else.
  const startPlayback = useCallback((songs: SongSummary[], index: number, label: string, position = 0) => {
    if (!songs.length) return;
    const safeIndex = Math.max(0, Math.min(index, songs.length - 1));
    const remote = remoteRef.current;
    if (!remote) {
      playbackRef.current.playQueue(songs, safeIndex, true, position, label);
      return;
    }
    // Shown here at once, as the queue the device is about to play. Frames it
    // writes before it gets there still name the old track and are ignored.
    const controller = playbackRef.current;
    trackGuard.current = { oldTrackID: controller.current?.id, until: Date.now() + TRACK_GUARD_MS };
    for (const song of songs) songCache.current.set(song.id, song);
    mirrorRows.current = undefined;
    controller.mirrorQueue(songs, safeIndex, label);
    controller.mirrorState({ isPlaying: true, position, duration: songs[safeIndex].duration ?? 0 });
    void invoke("send_connect_command", {
      peerId: remote.id,
      command: { name: "handoff", handoff: {
        trackIDs: songs.slice(0, 1000).map((song) => song.id),
        currentTrackID: songs[safeIndex].id,
        position,
        isPlaying: true,
      } },
    }).catch(() => setPageError(`Splynt could not reach ${remote.name}.`));
    setToast(`Playing on ${remote.name}`);
  }, []);

  const remoteControls = Boolean(remoteDevice && connectState.peers.some((peer) => peer.id === remoteDevice.id && hasRemoteControl(peer.playback)));
  const remoteControlsRef = useRef(remoteControls);
  remoteControlsRef.current = remoteControls;

  /// Add to queue, here or on the followed device. A v1 device cannot be
  /// told to, so it says so rather than adding the songs here instead.
  const enqueueSongs = useCallback((songs: SongSummary[], next = false) => {
    if (!songs.length) return;
    const remote = remoteRef.current;
    const count = songs.length > 1 ? `${songs.length} songs` : undefined;
    if (remote) {
      if (!remoteControlsRef.current) {
        setToast(`Update Splynt on ${remote.name} to change its queue from here`);
        return;
      }
      for (const song of songs) songCache.current.set(song.id, song);
      void invoke("send_connect_command", {
        peerId: remote.id,
        command: { name: next ? "playNext" : "enqueue", tracks: { trackIDs: songs.slice(0, 1000).map((song) => song.id) } },
      }).catch(() => setPageError(`Splynt could not reach ${remote.name}.`));
      setToast(next ? `${count ?? "Song"} playing next on ${remote.name}` : `${count ? `${count} added` : "Added"} to the queue on ${remote.name}`);
      return;
    }
    const controller = playbackRef.current;
    if (next) [...songs].reverse().forEach((song) => controller.playNext(song));
    else songs.forEach((song) => controller.enqueue(song));
    setToast(next ? (count ? `${count} playing next` : "Playing next") : (count ? `${count.replace(" songs", "")} added to queue` : "Added to queue"));
  }, []);

  const playCollection = useCallback((songs: SongSummary[], label: string) => {
    if (!songs.length) return;
    const start = playbackRef.current.shuffle ? Math.floor(Math.random() * songs.length) : 0;
    startPlayback(songs, start, label);
  }, [startPlayback]);

  /// Volume drags produce dozens of values a second. The followed device gets
  /// one every 120 ms, and always the last one.
  const volumeSend = useRef<{ timer?: number; value?: number }>({});

  /// What the player bar, panels, expanded player, keyboard and media keys
  /// drive. Locally that is the player itself. While following, the player
  /// mirrors the followed device, so everything reads the right track and
  /// state already, and this routes every control to that device instead.
  const transport = useMemo(() => {
    if (!remoteDevice) return playback;
    const device = remoteDevice;
    const send = (command: ConnectCommand) => {
      void invoke("send_connect_command", { peerId: device.id, command })
        .catch(() => setPageError(`Splynt could not reach ${device.name}.`));
    };
    const guardTrack = () => {
      trackGuard.current = { oldTrackID: playbackRef.current.current?.id, until: Date.now() + TRACK_GUARD_MS };
    };
    return {
      ...playback,
      remote: { ...device, controls: remoteControls },
      toggle: async () => {
        const playing = playbackRef.current.isPlaying;
        send({ name: playing ? "pause" : "play" });
        playbackRef.current.mirrorState({ isPlaying: !playing });
      },
      next: () => { guardTrack(); send({ name: "next" }); },
      // Previous a few seconds in restarts the same track on every Splynt
      // player, and frames for that track are then the new state.
      previous: () => { if (playbackRef.current.positionNow() <= 4) guardTrack(); send({ name: "previous" }); },
      seek: (value: number) => { send({ name: "seek", value }); playbackRef.current.mirrorState({ position: value }); },
      setShuffle: (value: boolean | ((current: boolean) => boolean)) => {
        if (!remoteControls) return;
        const next = typeof value === "function" ? value(playbackRef.current.shuffle) : value;
        send({ name: "setShuffle", value: next ? 1 : 0 });
        playbackRef.current.mirrorState({ shuffle: next });
      },
      cycleRepeat: () => {
        if (!remoteControls) return;
        const now = playbackRef.current.repeat;
        const next = now === "off" ? "all" : now === "all" ? "one" : "off";
        send({ name: "setRepeat", value: repeatToWire(next) });
        playbackRef.current.mirrorState({ repeat: next });
      },
      setVolume: (value: number) => {
        if (!remoteControls) return;
        const safe = Math.max(0, Math.min(1, value));
        playbackRef.current.mirrorState({ volume: safe });
        const pending = volumeSend.current;
        pending.value = safe;
        if (pending.timer !== undefined) return;
        send({ name: "setVolume", value: safe });
        pending.timer = window.setTimeout(() => {
          pending.timer = undefined;
          if (pending.value !== safe) send({ name: "setVolume", value: pending.value });
        }, 120);
      },
      skipTo: (row: number) => {
        const song = playbackRef.current.queue[row];
        if (!song) return;
        const rows = mirrorRows.current;
        if (!remoteControls || rows?.peerId !== device.id) {
          // A v1 device cannot jump within its queue; sending it the queue
          // from this row is the one way to get there.
          startPlayback(playbackRef.current.queue, row, playbackRef.current.contextLabel);
          return;
        }
        guardTrack();
        send({ name: "skipTo", queueItem: { index: rows.fullIndexes[row] ?? row, trackID: song.id } });
        playbackRef.current.mirrorQueue(playbackRef.current.queue, row);
        playbackRef.current.mirrorState({ position: 0 });
      },
      playQueue: (songs: SongSummary[], startIndex = 0, _autoplay = true, startPosition = 0, label = "Queue") =>
        startPlayback(songs, startIndex, label, startPosition),
      enqueue: (song: SongSummary) => enqueueSongs([song]),
      playNext: (song: SongSummary) => enqueueSongs([song], true),
      // No wire command removes or reorders another device's queue.
      removeQueueItem: () => undefined,
      moveQueueItem: () => undefined,
      clearUpcoming: () => undefined,
      clearManualQueue: () => undefined,
      undoQueueMutation: () => undefined,
    };
  }, [enqueueSongs, playback, remoteControls, remoteDevice, startPlayback]);
  const transportRef = useRef(transport);
  transportRef.current = transport;
  mirrorAction.current = (action, value) => {
    const controls = transportRef.current;
    if (action === "play" && !controls.isPlaying) void controls.toggle();
    else if (action === "pause" && controls.isPlaying) void controls.toggle();
    else if (action === "next") controls.next();
    else if (action === "previous") controls.previous();
    else if (action === "seek" && value !== undefined) controls.seek(value);
  };

  async function downloadCollection(songs: SongSummary[]) {
    setToast(`Downloading ${songs.length} ${songs.length === 1 ? "track" : "tracks"}…`);
    const { completed, failed } = await downloads.downloadMany(songs);
    if (!failed.length) setToast(`${completed} ${completed === 1 ? "track" : "tracks"} available offline`);
    else setToast(`${completed} downloaded · ${failed.length} did not finish. Retry them in Downloads.`);
  }

  async function removeCollection(songs: SongSummary[]) {
    for (const song of songs) await downloads.remove(song.id).catch(() => undefined);
    setToast("Removed from downloads");
  }

  async function clearAllDownloads() {
    setClearDownloadsConfirm(false);
    try {
      await downloads.clear();
      setToast("Downloads cleared");
    } catch (reason) {
      setPageError(reasonMessage(reason, "Downloads could not be cleared."));
    }
  }

  async function playAlbum(album: AlbumSummary, start = 0) {
    try {
      const saved = offline ? cachedDetail(library, "album", album.id) : undefined;
      const full = saved && "songs" in saved ? saved as AlbumDetail : await invoke<AlbumDetail>("get_album", { id: album.id });
      if (start === 0) playCollection(full.songs, full.title);
      else startPlayback(full.songs, start, full.title);
    } catch (reason) { setPageError(reasonMessage(reason, "That album could not be played.")); }
  }

  async function playPlaylist(playlist: PlaylistSummary, start = 0) {
    try {
      const saved = offline ? cachedDetail(library, "playlist", playlist.id) : undefined;
      const full = saved && "songs" in saved ? saved as PlaylistDetail : await invoke<PlaylistDetail>("get_playlist", { id: playlist.id });
      if (start === 0) playCollection(full.songs, full.name);
      else startPlayback(full.songs, start, full.name);
    } catch (reason) { setPageError(reasonMessage(reason, "That playlist could not be played.")); }
  }

  async function toggleSongStar(song: SongSummary) {
    const external = Boolean(parseExternalSource(song.id));
    const starred = !isSongLiked(song);
    try {
      if (external) setExternalLiked(library, song, starred);
      else await invoke("set_starred", { id: song.id, itemType: "song", starred });
      const matches = (item: SongSummary) => external ? trackMetadataKey(item) === trackMetadataKey(song) : item.id === song.id;
      const update = (songs: SongSummary[]) => songs.map((item) => matches(item) ? { ...item, starred: starred ? new Date().toISOString() : undefined } : item);
      setLibraryData((value) => ({ ...value, starredSongs: starred ? update([...value.starredSongs.filter((item) => !matches(item)), song]) : value.starredSongs.filter((item) => !matches(item)) }));
      setDetail((value) => value && "songs" in value ? { ...value, songs: update(value.songs) } : value);
      setRadioData((value) => value ? { ...value, songs: update(value.songs) } : value);
      setToast(starred ? "Saved to Liked Songs" : "Removed from Liked Songs");
    } catch (reason) { setPageError(reasonMessage(reason, "Liked Songs could not be updated.")); }
  }

  async function downloadSong(song: SongSummary) {
    try {
      await downloads.download(song);
      setToast(`${song.title} is available offline`);
    } catch (reason) {
      setPageError(reasonMessage(reason, "The song could not be downloaded."));
    }
  }

  async function removeDownloadedSong(song: SongSummary) {
    try {
      await downloads.remove(song.id);
      setToast(`Removed ${song.title} from downloads`);
    } catch (reason) {
      setPageError(reasonMessage(reason, "The download could not be removed."));
    }
  }

  async function createPlaylist(name: string) {
    try {
      const created = await invoke<PlaylistDetail>("create_playlist", { name: name.trim() });
      setCreatePlaylistOpen(false);
      const pending = pendingPlaylistSongs;
      setPendingPlaylistSongs(undefined);
      if (pending?.length) {
        await invoke("add_songs_to_playlist", { playlistId: created.id, songIds: pending.map((song) => song.id) });
        setSelectedIds(new Set());
      }
      reloadLibrary();
      navigate({ kind: "playlist", id: created.id });
    } catch (reason) { setPageError(reasonMessage(reason, "The playlist could not be created.")); }
  }

  async function renamePlaylist(playlistId: string, name: string) {
    try {
      await invoke("rename_playlist", { playlistId, name });
      if (route.kind === "playlist" && route.id === playlistId) {
        setDetail((value) => value && "name" in value ? { ...value, name: name.trim() } : value);
      }
      reloadLibrary();
      setToast("Playlist renamed");
    } catch (reason) { setPageError(reasonMessage(reason, "The playlist could not be renamed.")); }
  }

  async function deletePlaylist(playlistId: string) {
    try {
      await invoke("delete_playlist", { playlistId });
      setPlaylistDelete(undefined);
      reloadLibrary();
      setToast("Playlist deleted");
      if (route.kind === "playlist" && route.id === playlistId) navigate({ kind: "library" });
    } catch (reason) { setPageError(reasonMessage(reason, "The playlist could not be deleted.")); }
  }

  /// Songs for a collection the user acted on without opening it.
  async function collectionSongs(menu: CollectionMenuState) {
    if (menu.kind === "album") return (await invoke<AlbumDetail>("get_album", { id: menu.id })).songs;
    if (menu.kind === "playlist") return (await invoke<PlaylistDetail>("get_playlist", { id: menu.id })).songs;
    return (await invoke<ArtistDetail>("get_artist", { id: menu.id })).topSongs ?? [];
  }

  async function runCollectionAction(menu: CollectionMenuState, action: "play" | "enqueue" | "download") {
    try {
      const songs = await collectionSongs(menu);
      if (!songs.length) { setToast("That collection has no songs."); return; }
      if (action === "play") playCollection(songs, menu.name);
      else if (action === "enqueue") enqueueSongs(songs);
      else await downloadCollection(songs);
    } catch (reason) { setPageError(reasonMessage(reason, "That collection could not be opened.")); }
  }

  async function setCollectionStarred(kind: "album" | "artist", id: string, starred: boolean, summary?: AlbumSummary | ArtistSummary) {
    try {
      await invoke("set_starred", { id, itemType: kind, starred });
      const stamp = starred ? new Date().toISOString() : undefined;
      setLibraryData((value) => {
        if (kind === "album") {
          const album = (summary ?? value.albums.find((item) => item.id === id)) as AlbumSummary | undefined;
          const without = value.starredAlbums.filter((item) => item.id !== id);
          return { ...value, starredAlbums: starred && album ? [{ ...album, starred: stamp }, ...without] : without };
        }
        const artist = (summary ?? value.artists.find((item) => item.id === id)) as ArtistSummary | undefined;
        const without = value.starredArtists.filter((item) => item.id !== id);
        return { ...value, starredArtists: starred && artist ? [{ ...artist, starred: stamp }, ...without] : without };
      });
      setToast(kind === "album" ? (starred ? "Saved album to Liked" : "Removed album from Liked") : (starred ? "Following artist" : "Unfollowed artist"));
    } catch (reason) { setPageError(reasonMessage(reason, "That could not be saved to your library.")); }
  }

  /// Subsonic cannot move a row, so the whole order is rewritten. The optimistic
  /// local move is reconciled with whatever the server returns.
  async function reorderCurrentPlaylist(from: number, to: number) {
    if (route.kind !== "playlist" || !detail || !("songs" in detail)) return;
    const next = [...detail.songs];
    const [moved] = next.splice(from, 1);
    next.splice(to, 0, moved);
    setDetail((value) => value && "songs" in value ? { ...value, songs: next } : value);
    try {
      const updated = await invoke<PlaylistDetail>("set_playlist_songs", { playlistId: route.id, songIds: next.map((song) => song.id) });
      setDetail(updated);
      cacheDetail(library, "playlist", route.id, updated);
    } catch (reason) {
      setDetail((value) => value && "songs" in value ? { ...value, songs: detail.songs } : value);
      setPageError(reasonMessage(reason, "The playlist order could not be saved."));
    }
  }

  async function saveQueueAsPlaylist(name: string) {
    const songs = playbackRef.current.queue;
    if (!songs.length) return;
    setSaveQueueOpen(false);
    try {
      const created = await invoke<PlaylistDetail>("create_playlist", { name: name.trim() });
      await invoke("add_songs_to_playlist", { playlistId: created.id, songIds: songs.map((song) => song.id) });
      reloadLibrary();
      setToast(`Saved ${songs.length} ${songs.length === 1 ? "song" : "songs"} to ${created.name}`);
      navigate({ kind: "playlist", id: created.id });
    } catch (reason) { setPageError(reasonMessage(reason, "The queue could not be saved as a playlist.")); }
  }

  async function addSongIdsToPlaylist(songIds: string[], playlist: PlaylistSummary) {
    try {
      await invoke("add_songs_to_playlist", { playlistId: playlist.id, songIds });
      setToast(songIds.length === 1 ? `Added to ${playlist.name}` : `Added ${songIds.length} songs to ${playlist.name}`);
      reloadLibrary();
    } catch (reason) { setPageError(reasonMessage(reason, "Those songs could not be added to that playlist.")); }
  }

  async function removeFromCurrentPlaylist(index: number) {
    if (route.kind !== "playlist") return;
    try {
      await invoke("remove_song_from_playlist", { playlistId: route.id, songIndex: index });
      setDetail((value) => value && "songs" in value ? { ...value, songs: value.songs.filter((_, itemIndex) => itemIndex !== index) } : value);
      setToast("Removed from playlist");
    } catch (reason) { setPageError(reasonMessage(reason, "The song could not be removed from this playlist.")); }
  }

  async function movePlaybackTo(peer: ConnectPeer) {
    const controller = playbackRef.current;
    if (!controller.current || !controller.queue.length) return;
    const position = controller.positionNow();
    const wasPlaying = controller.isPlaying;
    // A transferred queue is capped at 1,000 entries (`docs/connect/WIRE-V1.md`).
    // The other transfer sites already sliced; these two sent the whole queue,
    // and a receiver that enforces the cap refuses the frame outright rather
    // than truncating it, so a long queue moved nothing at all.
    await sendRemote(peer.id, { name: "handoff", handoff: { trackIDs: controller.queue.slice(0, 1000).map((song) => song.id), currentTrackID: controller.current.id, position, isPlaying: wasPlaying } });
    // Moving between devices has to stop the first one; the handoff only ever
    // starts the new one, so without this both would be playing.
    const previous = remoteRef.current;
    if (previous && previous.id !== peer.id) await sendRemote(previous.id, { name: "pause" });
    // The same queue, now as a mirror of the device it moved to. Its frames
    // from before the handoff still name what it was playing then.
    trackGuard.current = { oldTrackID: peer.playback.trackID, until: Date.now() + TRACK_GUARD_MS };
    if (previous) endFollow("changed output");
    controller.mirrorQueue(controller.queue, controller.index, controller.contextLabel);
    controller.mirrorState({ isPlaying: wasPlaying, position, duration: controller.duration });
    beginFollow(peer, "chosen");
    setToast(`Playing on ${deviceName(peer)}`);
  }

  /// Picking this computer while following: the same queue, track and
  /// position continue here, and the device that was playing pauses. The
  /// pause goes first, so that device sees this computer start after it
  /// stopped and follows it, rather than the other way round.
  async function takePlaybackHere() {
    const followed = remoteRef.current;
    const controller = playbackRef.current;
    if (!followed) return;
    const play = controller.isPlaying;
    const position = controller.positionNow();
    // The device that was playing until now no longer takes this computer
    // back the moment its last frame arrives.
    localStoppedAt.current = Date.now() / 1000;
    await sendRemote(followed.id, { name: "pause" });
    logConnectEvent("remote_end", { peer: followed.id, reason: "played here" });
    remoteRef.current = undefined;
    setRemoteDevice(undefined);
    mirrorRows.current = undefined;
    trackGuard.current = undefined;
    if (controller.isMirroring()) controller.endMirror({ play, position });
  }

  /// The queue a peer holds, asked for and resolved, or undefined if it does
  /// not answer within a second and a half. A v1 peer never will.
  async function fetchPeerQueue(peer: ConnectPeer) {
    if (!hasRemoteControl(peer.playback) || peer.playback.queueRevision === undefined) return undefined;
    const revision = peer.playback.queueRevision;
    let queue = snapshotRef.current?.queues?.[peer.id];
    if (queue?.revision !== revision) {
      requestPeerQueue(peer.id, revision);
      const deadline = Date.now() + 1_500;
      while (Date.now() < deadline) {
        await new Promise((resolve) => window.setTimeout(resolve, 100));
        queue = snapshotRef.current?.queues?.[peer.id];
        if (queue?.revision === revision) break;
      }
      if (queue?.revision !== revision) return undefined;
    }
    const resolved = await resolveSongs(queue.trackIDs);
    const songs: SongSummary[] = [];
    let index = -1;
    resolved.forEach((song, row) => {
      if (!song) return;
      if (row === queue.index - queue.offset) index = songs.length;
      songs.push(song);
    });
    return index < 0 ? undefined : { songs, index, label: queue.contextLabel };
  }

  async function playPeerHere(peer: ConnectPeer) {
    if (remoteRef.current?.id === peer.id && playbackRef.current.isMirroring()) return void await takePlaybackHere();
    if (!peer.playback.trackID) return;
    // Taking the audio back ends remote control, whichever device it was for —
    // including a third device that was playing until now.
    const previous = remoteRef.current;
    if (previous && previous.id !== peer.id) void sendRemote(previous.id, { name: "pause" });
    if (previous) endFollow("played here");
    try {
      // The whole queue when the device can send it; a v1 device's frames
      // carry only the current track.
      const whole = await fetchPeerQueue(peer);
      const songs = whole?.songs ?? await invoke<SongSummary[]>("get_songs_by_ids", { ids: [peer.playback.trackID] });
      if (!songs?.length) throw new Error(`${peer.name} is playing something this server could not resolve.`);
      localStoppedAt.current = Date.now() / 1000;
      await sendRemote(peer.id, { name: "pause" });
      playbackRef.current.playQueue(songs, whole?.index ?? 0, peer.playback.isPlaying,
        projectedPeerPosition(peer.playback, peer.updatedAt), whole?.label ?? deviceName(peer));
    } catch (reason) { setPageError(reasonMessage(reason, "That track is not available on this server.")); }
  }

  async function startGroup() {
    const localId = connectState.localDeviceId;
    if (!localId || !playback.current || !connectState.peers.length) return;
    // A device already rendering another session, or already driving a third
    // device, is not available to take. Inviting it only produces a decline.
    const candidates = connectState.peers.filter((peer) => {
      const commitment = peer.commitment;
      return !commitment || (!commitment.sessionID && !commitment.controllingPeerID);
    });
    if (!candidates.length) {
      setPageError("Every nearby Splynt device is already in a session.");
      return;
    }
    const sessionId = crypto.randomUUID();
    const startAt = playback.positionNow();
    const handoff = { trackIDs: playback.queue.slice(0, 1000).map((song) => song.id), currentTrackID: playback.current.id, position: startAt, isPlaying: playback.isPlaying };
    // A new session id starts its own revision count.
    groupRevision.current = 1;
    const group: ConnectGroup = { id: sessionId, leaderID: localId, trackID: playback.current.id, position: startAt, isPlaying: playback.isPlaying, sentAt: Date.now(), revision: 1 };
    lastGroupTrack.current = `${sessionId}:${playback.current.id}`;
    // A member is a device that answered. `sendGroupFrame` returning true only
    // means the frame reached this device's own network stack, which a
    // stale-but-open connection reports for a long time after the peer behind
    // it stopped listening. That is what made a session report a member it had
    // never reached.
    const reachable = await Promise.all(candidates.map(async (peer) =>
      await sendGroupFrame(peer.id, { name: "groupJoin", groupJoin: { group, handoff } }) ? peer.id : undefined));
    const awaiting = new Set(reachable.filter((peerId): peerId is string => Boolean(peerId)));
    groupSendFailures.current.clear();
    if (!awaiting.size) {
      lastGroupTrack.current = undefined;
      groupRevision.current = 0;
      logConnectEvent("group_invited", { session: sessionId, invited: candidates.length, accepted: 0, declined: 0, unanswered: 0, reasons: "", queued: handoff.trackIDs.length, unreachable: candidates.length });
      setPageError("No other Splynt device could be reached.");
      return;
    }
    const invite = { sessionId, awaiting, accepted: [] as string[], declined: new Map<string, string>() };
    pendingInvite.current = invite;
    // Bounded, because a follower has real work to do first: it resolves the
    // track against the server before it can honestly claim to have joined.
    const deadline = Date.now() + GROUP_JOIN_TIMEOUT_MS;
    while (invite.awaiting.size && Date.now() < deadline) {
      await new Promise((resolve) => window.setTimeout(resolve, 50));
    }
    pendingInvite.current = undefined;
    const members = invite.accepted;
    logConnectEvent("group_invited", {
      session: sessionId,
      invited: candidates.length,
      accepted: members.length,
      declined: invite.declined.size,
      unanswered: invite.awaiting.size,
      reasons: [...invite.declined.values()].sort().join(","),
      queued: handoff.trackIDs.length,
    });
    if (!members.length) {
      lastGroupTrack.current = undefined;
      groupRevision.current = 0;
      setPageError(invite.declined.size
        ? "No other Splynt device could join right now."
        : "No other Splynt device answered.");
      return;
    }
    setGroupSession({ id: sessionId, leaderID: localId, memberIDs: members });
    // Only now has a session actually started, which is what iOS's
    // `connect_group_started` has always meant.
    logConnectEvent("group_started", { session: sessionId, members: members.length, revision: groupRevision.current });
    setToast(`Group session started on ${members.length + 1} devices`);
  }

  async function stopGroup() {
    const session = groupRef.current;
    if (!session) return;
    for (const peerId of session.memberIDs) await sendRemote(peerId, { name: "groupLeave", group: { id: session.id, leaderID: session.leaderID, trackID: playback.current?.id ?? "", position: playback.positionNow(), isPlaying: playback.isPlaying, sentAt: Date.now() } });
    endGroupDiagnostics("stopped here");
    logConnectEvent("group_stopped", { session: session.id, members: session.memberIDs.length });
    setGroupSession(undefined);
    lastGroupTrack.current = undefined;
    setToast("Group session ended");
  }

  function openTrackMenu(song: SongSummary, index: number, event: ReactMouseEvent) {
    // Right-clicking outside the selection selects that row, so the menu always
    // acts on what is highlighted.
    const occurrence = `${song.id}-${index}`;
    if (!selectedIds.has(occurrence)) {
      selectionAnchor.current = index;
      setSelectedIds(new Set([occurrence]));
    }
    const fromButton = event.type === "click";
    const rect = fromButton ? (event.currentTarget as HTMLElement).getBoundingClientRect() : undefined;
    setTrackMenu({ song, index, x: rect ? rect.right : event.clientX, y: rect ? rect.bottom : event.clientY });
  }

  /// Top songs when the server has them; otherwise the discography in order,
  /// so an artist page always has something to play.
  async function playArtist(artist: ArtistSummary | ArtistDetail) {
    try {
      const detailed = "albums" in artist ? artist : await invoke<ArtistDetail>("get_artist", { id: artist.id });
      if (detailed.topSongs?.length) { playCollection(filterSongs(detailed.topSongs), detailed.name); return; }
      const albums = await Promise.all(detailed.albums.slice(0, 20).map((album) => invoke<AlbumDetail>("get_album", { id: album.id }).catch(() => undefined)));
      const songs = filterSongs(albums.flatMap((album) => album?.songs ?? []));
      if (!songs.length) { setToast("This artist has no songs to play."); return; }
      playCollection(songs, detailed.name);
    } catch (reason) { setPageError(reasonMessage(reason, "That artist could not be played.")); }
  }

  async function removeSelectionFromPlaylist() {
    if (route.kind !== "playlist" || !detail || !("songs" in detail)) return;
    const indexes = detail.songs.map((song, index) => selectedIds.has(`${song.id}-${index}`) ? index : -1).filter((index) => index >= 0).sort((a, b) => b - a);
    if (!indexes.length) return;
    try {
      // Highest index first, so each removal still points at the right row.
      for (const index of indexes) await invoke("remove_song_from_playlist", { playlistId: route.id, songIndex: index });
      const removed = new Set(indexes);
      setDetail((value) => value && "songs" in value ? { ...value, songs: value.songs.filter((_, index) => !removed.has(index)) } : value);
      setSelectedIds(new Set());
      setToast(indexes.length === 1 ? "Removed from playlist" : `Removed ${indexes.length} songs from playlist`);
    } catch (reason) { setPageError(reasonMessage(reason, "Those songs could not be removed from this playlist.")); }
  }

  function selectTrack(song: SongSummary, index: number, event: ReactMouseEvent) {
    const songs = visibleSongs;
    setSelectedIds((current) => {
      if (event.shiftKey) {
        const start = Math.min(selectionAnchor.current, index);
        const end = Math.max(selectionAnchor.current, index);
        return new Set(songs.slice(start, end + 1).map((item, offset) => `${item.id}-${start + offset}`));
      }
      selectionAnchor.current = index;
      if (event.metaKey || event.ctrlKey) {
        const next = new Set(current);
        const occurrence = `${song.id}-${index}`;
        if (next.has(occurrence)) next.delete(occurrence); else next.add(occurrence);
        return next;
      }
      return new Set([`${song.id}-${index}`]);
    });
  }

  function beginResize(event: ReactPointerEvent) {
    event.preventDefault();
    const start = event.clientX;
    const initial = sidebarWidth;
    const move = (moveEvent: PointerEvent) => setSidebarWidth(snapSidebarWidth(initial + moveEvent.clientX - start));
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  }

  function beginContextResize(event: ReactPointerEvent) {
    event.preventDefault();
    const start = event.clientX;
    const initial = contextWidth;
    const move = (moveEvent: PointerEvent) => setContextWidth(Math.max(280, Math.min(460, initial - (moveEvent.clientX - start))));
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  }

  function resizeSidebarWithKeyboard(event: ReactKeyboardEvent<HTMLButtonElement>) {
    // Arrow keys step through the expanded range, and one step past either
    // end of it crosses to the other state instead of sticking at the minimum.
    const collapsed = sidebarWidth === SIDEBAR_COLLAPSED;
    const next = event.key === "ArrowLeft" ? (sidebarWidth <= SIDEBAR_MIN_EXPANDED ? SIDEBAR_COLLAPSED : sidebarWidth - 12)
      : event.key === "ArrowRight" ? (collapsed ? SIDEBAR_MIN_EXPANDED : sidebarWidth + 12)
        : event.key === "Home" ? SIDEBAR_COLLAPSED
          : event.key === "End" ? SIDEBAR_MAX
            : undefined;
    if (next === undefined) return;
    event.preventDefault();
    setSidebarWidth(snapSidebarWidth(next));
  }

  function resizeContextWithKeyboard(event: ReactKeyboardEvent<HTMLButtonElement>) {
    const next = event.key === "ArrowLeft" ? contextWidth + 12
      : event.key === "ArrowRight" ? contextWidth - 12
        : event.key === "Home" ? 280
          : event.key === "End" ? 460
            : undefined;
    if (next === undefined) return;
    event.preventDefault();
    setContextWidth(Math.max(280, Math.min(460, next)));
  }

  // The explicit filter is a browsing filter: it never hides a track the user
  // already downloaded on purpose.
  const hideExplicit = settings.hideExplicitContent;
  const filterSongs = useCallback((songs: SongSummary[]) => hideExplicit
    ? songs.filter((song) => !song.explicitStatus || song.explicitStatus === "clean")
    : songs, [hideExplicit]);
  const shownDetail = useMemo(() => {
    if (!detail) return detail;
    if ("songs" in detail) return { ...detail, songs: filterSongs(detail.songs) };
    return { ...detail, topSongs: detail.topSongs ? filterSongs(detail.topSongs) : detail.topSongs };
  }, [detail, filterSongs]);
  const shownSearch = useMemo(() => ({ ...searchResults, songs: filterSongs(searchResults.songs) }), [filterSongs, searchResults]);
  const shownLiked = useMemo(() => filterSongs(libraryData.starredSongs), [filterSongs, libraryData.starredSongs]);
  const likedArtist = route.kind === "liked" ? route.artist : undefined;
  const shownLikedPage = useMemo(() => likedArtist ? likedBy(shownLiked, likedArtist) : shownLiked, [likedArtist, shownLiked]);
  const artistLikedCount = useMemo(() => {
    if (route.kind !== "artist" || !shownDetail || !("albums" in shownDetail)) return 0;
    return likedBy(libraryData.starredSongs, { id: shownDetail.id, name: shownDetail.name }).length;
  }, [libraryData.starredSongs, route.kind, shownDetail]);
  const shownRadio = useMemo(() => radioData ? { ...radioData, songs: filterSongs(radioData.songs) } : radioData, [filterSongs, radioData]);

  const visibleSongs = shownDetail && "songs" in shownDetail ? shownDetail.songs : route.kind === "liked" ? shownLikedPage : route.kind === "downloads" ? downloads.items.map((item) => item.song) : route.kind === "radio" ? shownRadio?.songs ?? [] : shownSearch.songs;
  const selectedSongs = visibleSongs.filter((song, index) => selectedIds.has(`${song.id}-${index}`));

  async function addSongsToPlaylist(songs: SongSummary[], playlist: PlaylistSummary) {
    try {
      await invoke("add_songs_to_playlist", { playlistId: playlist.id, songIds: songs.map((song) => song.id) });
      setToast(`Added to ${playlist.name}`);
      setSelectedIds(new Set());
      reloadLibrary();
    } catch (reason) {
      setPageError(reasonMessage(reason, "The song could not be added to that playlist."));
    }
  }

  async function toggleSelectionLike(songs: SongSummary[]) {
    const allLiked = songs.every((song) => isSongLiked(song));
    for (const song of songs) {
      if (isSongLiked(song) === allLiked) await toggleSongStar(song);
    }
  }

  function isSongLiked(song: SongSummary) {
    return parseExternalSource(song.id)
      ? isExternalLiked(library, song)
      : Boolean(song.starred) || libraryData.starredSongs.some((item) => item.id === song.id);
  }

  const rememberJumpBackIn = useCallback((item: JumpBackInItem) => {
    setJumpBackIn(rememberRecentCollection(profileScope, item));
  }, [profileScope]);
  const openAlbum = useCallback((album: AlbumSummary) => {
    rememberJumpBackIn({ kind: "album", id: album.id, title: album.title, subtitle: `Album · ${album.artist}`, coverArt: album.coverArt });
    navigate({ kind: "album", id: album.id });
  }, [navigate, rememberJumpBackIn]);
  const openArtist = useCallback((artist: ArtistSummary) => {
    rememberJumpBackIn({ kind: "artist", id: artist.id, title: artist.name, subtitle: "Artist", coverArt: artist.coverArt });
    navigate({ kind: "artist", id: artist.id });
  }, [navigate, rememberJumpBackIn]);
  const openPlaylist = useCallback((playlist: PlaylistSummary) => {
    rememberJumpBackIn({ kind: "playlist", id: playlist.id, title: playlist.name, subtitle: playlist.owner ? `Playlist · ${playlist.owner}` : "Playlist", coverArt: playlist.coverArt });
    navigate({ kind: "playlist", id: playlist.id });
  }, [navigate, rememberJumpBackIn]);
  const openAlbumById = (id: string) => {
    const summary = libraryData.albums.find((album) => album.id === id)
      ?? (playback.current?.albumId === id ? { id, title: playback.current.album, artist: playback.current.artist, coverArt: playback.current.coverArt } : undefined);
    if (summary) openAlbum(summary);
    else navigate({ kind: "album", id });
  };
  const openArtistById = (id: string) => {
    const summary = libraryData.artists.find((artist) => artist.id === id)
      ?? (playback.current?.artistId === id ? { id, name: playback.current.artist, coverArt: playback.current.coverArt } : undefined);
    if (summary) openArtist(summary);
    else navigate({ kind: "artist", id });
  };
  const startRadio = (song: SongSummary) => navigate({ kind: "radio", id: song.id, title: `${song.title} Radio` });
  const toggleShuffle = () => transport.setShuffle((value) => !value);
  const detailTitle = detail ? ("name" in detail ? detail.name : "title" in detail ? detail.title : undefined) : undefined;
  const routeTitle = route.kind === "home" ? "Home" : route.kind === "search" ? "Search" : route.kind === "library" ? "Your Library" : route.kind === "liked" ? "Liked Songs" : route.kind === "downloads" ? "Downloads" : route.kind === "profile" ? "Profile" : route.kind === "settings" ? "Settings" : route.kind === "history" ? "Listening History" : route.kind === "stats" ? "Stats" : route.kind === "radio" ? route.title : "Splynt";
  const visiblePlaylists = settings.hideExternalPlaylists
    ? libraryData.playlists.filter((playlist) => parseExternalSource(playlist.id)?.type !== "playlist")
    : libraryData.playlists;
  const homeShortcuts = useMemo<HomeShortcut[]>(() => {
    const items: HomeShortcut[] = [];
    if (shownLiked.length) items.push({ kind: "liked", id: "liked", title: "Liked Songs", subtitle: `${shownLiked.length} songs` });
    items.push(...visiblePlaylists.slice(0, 3).map((playlist) => ({
      kind: "playlist" as const,
      id: playlist.id,
      title: playlist.name,
      subtitle: playlist.owner ? `Playlist · ${playlist.owner}` : "Playlist",
      coverArt: playlist.coverArt,
    })));
    const albums = [...homeData.recent, ...homeData.frequent]
      .filter((album, index, all) => all.findIndex((item) => item.id === album.id) === index);
    for (const album of albums) {
      if (items.length >= 8) break;
      items.push({ kind: "album", id: album.id, title: album.title, subtitle: album.artist, coverArt: album.coverArt });
    }
    return items.slice(0, 8);
  }, [homeData.frequent, homeData.recent, shownLiked.length, visiblePlaylists]);
  const openHomeShortcut = (item: HomeShortcut) => {
    if (item.kind === "liked") navigate({ kind: "liked" });
    else if (item.kind === "playlist") openPlaylist({ id: item.id, name: item.title, owner: item.subtitle.startsWith("Playlist · ") ? item.subtitle.slice(11) : undefined, coverArt: item.coverArt });
    else openAlbum({ id: item.id, title: item.title, artist: item.subtitle, coverArt: item.coverArt });
  };
  const playHomeShortcut = (item: HomeShortcut) => {
    if (item.kind === "liked") playCollection(shownLiked, "Liked Songs");
    else if (item.kind === "playlist") void playPlaylist({ id: item.id, name: item.title, coverArt: item.coverArt });
    else void playAlbum({ id: item.id, title: item.title, artist: item.subtitle, coverArt: item.coverArt });
  };
  const openJumpBackIn = (item: JumpBackInItem) => {
    if (item.kind === "album") openAlbum({ id: item.id, title: item.title, artist: item.subtitle.replace(/^Album · /, ""), coverArt: item.coverArt });
    else if (item.kind === "artist") openArtist({ id: item.id, name: item.title, coverArt: item.coverArt });
    else openPlaylist({ id: item.id, name: item.title, owner: item.subtitle.startsWith("Playlist · ") ? item.subtitle.slice(11) : undefined, coverArt: item.coverArt });
  };
  const matchingSearchPlaylists = query.trim()
    ? visiblePlaylists.filter((playlist) => playlist.name.toLowerCase().includes(query.trim().toLowerCase())).slice(0, 30)
    : [];
  const hasSearchResults = searchResults.songs.length + searchResults.albums.length + searchResults.artists.length + matchingSearchPlaylists.length + lyricMatches.length > 0;
  // With no filter the library mixes playlists, followed artists and saved
  // albums, as Spotify's does; a chip narrows it to the whole server list.
  const libraryItems = useMemo<Views.LibraryItem[]>(() => {
    const playlists = visiblePlaylists.map((playlist): Views.LibraryItem => ({ kind: "playlist", id: playlist.id, name: playlist.name, playlist }));
    const artists = (libraryFilter === "artists" ? libraryData.artists : libraryData.starredArtists).map((artist): Views.LibraryItem => ({ kind: "artist", id: artist.id, name: artist.name, artist }));
    const albums = (libraryFilter === "albums" ? libraryData.albums : libraryData.starredAlbums).map((album): Views.LibraryItem => ({ kind: "album", id: album.id, name: album.title, album }));
    const items = libraryFilter === "playlists" ? playlists : libraryFilter === "artists" ? artists : libraryFilter === "albums" ? albums : [...playlists, ...artists, ...albums];
    const needle = librarySearch.trim().toLowerCase();
    return needle ? items.filter((item) => item.name.toLowerCase().includes(needle)) : items;
  }, [libraryData.albums, libraryData.artists, libraryData.starredAlbums, libraryData.starredArtists, libraryFilter, librarySearch, visiblePlaylists]);
  const recency = useMemo(() => new Map(jumpBackIn.map((item, index) => [item.id, index])), [jumpBackIn]);
  const itemCreator = (item: Views.LibraryItem) => item.kind === "playlist" ? item.playlist.owner ?? "" : item.kind === "album" ? item.album.artist : item.name;
  const itemAdded = (item: Views.LibraryItem) => Date.parse((item.kind === "playlist" ? item.playlist.changed : item.kind === "album" ? item.album.starred ?? item.album.created : item.artist.starred) ?? "") || 0;
  // Pinned rows float to the top of whatever ordering is in effect.
  const orderedLibraryItems = [...libraryItems].sort((a, b) => {
    const pinnedDelta = Number(pinned.has(b.id)) - Number(pinned.has(a.id));
    if (pinnedDelta) return pinnedDelta;
    if (librarySort === "alphabetical") return a.name.localeCompare(b.name);
    if (librarySort === "creator") return itemCreator(a).localeCompare(itemCreator(b)) || a.name.localeCompare(b.name);
    if (librarySort === "added") return itemAdded(b) - itemAdded(a);
    return (recency.get(a.id) ?? Number.MAX_SAFE_INTEGER) - (recency.get(b.id) ?? Number.MAX_SAFE_INTEGER) || itemAdded(b) - itemAdded(a);
  });
  const sidebarLibraryItems = librarySearch ? orderedLibraryItems : orderedLibraryItems.slice(0, 600);
  const showLikedRow = !libraryFilter || libraryFilter === "playlists";
  const likedMatches = !librarySearch.trim() || "liked songs".includes(librarySearch.trim().toLowerCase());
  const downloadsMatches = !librarySearch.trim() || "downloads".includes(librarySearch.trim().toLowerCase());
  // Albums the user already has liked songs on, newest like first.
  const likedAlbums = useMemo(() => {
    const byId = new Map(libraryData.albums.map((album) => [album.id, album]));
    const seen = new Set<string>();
    const albums: AlbumSummary[] = [];
    for (const song of libraryData.starredSongs) {
      if (!song.albumId || seen.has(song.albumId)) continue;
      seen.add(song.albumId);
      const album = byId.get(song.albumId) ?? { id: song.albumId, title: song.album, artist: song.artist, artistId: song.artistId, coverArt: song.coverArt, year: song.year };
      albums.push(album);
      if (albums.length >= 24) break;
    }
    return albums;
  }, [libraryData.albums, libraryData.starredSongs]);

  const openCollectionMenu = (kind: CollectionMenuState["kind"], id: string, name: string, event: ReactMouseEvent) => {
    event.preventDefault();
    // A "…" button opens the menu below itself; a right-click opens at the pointer.
    const rect = event.type === "click" ? (event.currentTarget as HTMLElement).getBoundingClientRect() : undefined;
    setCollectionMenu({ kind, id, name, x: rect ? rect.left : event.clientX, y: rect ? rect.bottom + 4 : event.clientY });
  };
  const openCurrentCollectionMenu = (event: ReactMouseEvent) => {
    if (!("id" in route) || !(route.kind === "album" || route.kind === "artist" || route.kind === "playlist")) return;
    openCollectionMenu(route.kind, route.id, detailTitle ?? routeTitle, event);
  };
  const cardMenu = {
    album: (album: AlbumSummary, event: ReactMouseEvent) => openCollectionMenu("album", album.id, album.title, event),
    artist: (artist: ArtistSummary, event: ReactMouseEvent) => openCollectionMenu("artist", artist.id, artist.name, event),
    playlist: (playlist: PlaylistSummary, event: ReactMouseEvent) => openCollectionMenu("playlist", playlist.id, playlist.name, event),
  };
  const menuStarred = collectionMenu?.kind === "album"
    ? libraryData.starredAlbums.some((album) => album.id === collectionMenu.id)
    : collectionMenu?.kind === "artist"
      ? libraryData.starredArtists.some((artist) => artist.id === collectionMenu.id)
      : false;

  const collectionStarred = route.kind === "album"
    ? libraryData.starredAlbums.some((album) => album.id === route.id)
    : route.kind === "artist"
      ? libraryData.starredArtists.some((artist) => artist.id === route.id)
      : false;
  // Only the owner can rename, delete or reorder a server playlist.
  const ownsCurrentPlaylist = route.kind === "playlist" && detail !== undefined && "songs" in detail
    && (!(detail as PlaylistDetail).owner || (detail as PlaylistDetail).owner === library.server.username);
  const currentLiked = Boolean(playback.current && (
    isExternalLiked(library, playback.current)
    || libraryData.starredSongs.some((song) => song.id === playback.current?.id)
  ));

  const compactSidebar = sidebarWidth === SIDEBAR_COLLAPSED;
  const toggleSidebar = () => {
    if (compactSidebar) setSidebarWidth(expandedSidebarWidth.current);
    else { expandedSidebarWidth.current = sidebarWidth; setSidebarWidth(SIDEBAR_COLLAPSED); }
  };
  // Lyrics live inside the now-playing view: the mic opens it with lyrics slid
  // in beside the art, and pressing it again slides them back out.
  const toggleLyrics = () => {
    if (!playback.current) return;
    if (fullPlayer && fullPlayerSurface === "lyrics") { setFullPlayerSurface("artwork"); return; }
    setFullPlayerSurface("lyrics");
    setFullPlayer(true);
  };
  const songList: Views.SongListHandlers = {
    currentId: playback.current?.id,
    isPlaying: playback.isPlaying,
    isLiked: isSongLiked,
    onMenu: openTrackMenu,
    onSelect: selectTrack,
    onToggleStar: (song) => void toggleSongStar(song),
    selectedIds,
    showQuality: settings.showTrackQuality,
  };
  const collectionControls = {
    compactHeader: headerCompact,
    onTogglePlayback: () => void transport.toggle(),
    onToggleShuffle: toggleShuffle,
    shuffleArmed: playback.shuffle,
  };
  const currentArtist = playback.current?.artistId ? libraryData.artists.find((artist) => artist.id === playback.current?.artistId) : undefined;
  const currentArtistFollowed = Boolean(playback.current?.artistId && libraryData.starredArtists.some((artist) => artist.id === playback.current?.artistId));
  const detailArtistArt = detail && "artistId" in detail && detail.artistId ? libraryData.artists.find((artist) => artist.id === detail.artistId)?.coverArt : undefined;
  keyActions.current = {
    toggleLyrics,
    likeCurrent: () => { if (playback.current) void toggleSongStar(playback.current); },
    canSelectAll: () => visibleSongs.length > 0 && route.kind !== "home" && route.kind !== "settings",
    selectAll: () => setSelectedIds(new Set(visibleSongs.map((song, index) => `${song.id}-${index}`))),
    canRemoveSelection: () => ownsCurrentPlaylist && selectedIds.size > 0,
    removeSelection: () => void removeSelectionFromPlaylist(),
  };
  const showSearchDropdown = searchFocused && !query.trim() && recentSearches.length > 0;
  const clearSelectionOnBackground = (event: ReactMouseEvent) => {
    if (!selectedIds.size) return;
    const target = event.target as HTMLElement;
    if (target.closest("[data-track-row], .context-menu, .collection-actions")) return;
    setSelectedIds(new Set());
  };

  return (
    <div className={`desktop-shell${panelMode ? " desktop-shell--panel" : ""}${fullPlayer ? " desktop-shell--expanded" : ""}${compactSidebar ? " desktop-shell--compact-sidebar" : ""}${remoteDevice ? " desktop-shell--remote" : ""}`} style={{ "--sidebar-width": `${sidebarWidth}px`, "--context-width": `${contextWidth}px` } as CSSProperties}>
      <header className="global-bar" data-tauri-drag-region>
        <div className="global-bar__leading" data-tauri-drag-region>
          <Brand compact />
          <div className="history-controls">
            <button aria-label="Back" disabled={historyIndex === 0} onClick={goBack} title="Go back" type="button"><ChevronLeft size={20} /></button>
            <button aria-label="Forward" disabled={historyIndex === history.length - 1} onClick={goForward} title="Go forward" type="button"><ChevronRight size={20} /></button>
          </div>
        </div>
        <nav aria-label="Main navigation" className="global-bar__center">
          <button aria-current={route.kind === "home" ? "page" : undefined} aria-label="Home" className={route.kind === "home" ? "home-button home-button--active" : "home-button"} onClick={() => navigate({ kind: "home" })} title="Home" type="button"><Home fill={route.kind === "home" ? "currentColor" : "none"} size={22} /></button>
          <div className="global-search">
            <label className={route.kind === "search" ? "desktop-search desktop-search--active" : "desktop-search"}>
              <Search size={22} />
              <input
                data-search-input
                onBlur={() => window.setTimeout(() => setSearchFocused(false), 120)}
                onChange={(event) => { setQuery(event.target.value); if (route.kind !== "search" && event.target.value.trim()) navigate({ kind: "search" }); }}
                onFocus={() => setSearchFocused(true)}
                onKeyDown={(event) => {
                  if (event.key === "Enter" && route.kind !== "search") { event.preventDefault(); navigate({ kind: "search" }); }
                  else if (event.key === "ArrowDown") { event.preventDefault(); document.querySelector<HTMLElement>("[data-search-suggestion], [data-search-result]")?.focus(); }
                }}
                placeholder="What do you want to play?"
                value={query}
              />
              {query && <button aria-label="Clear search" className="desktop-search__clear" onClick={() => setQuery("")} title="Clear search" type="button"><X size={20} /></button>}
              <span aria-hidden="true" className="desktop-search__divider" />
              <button aria-label="Browse" className={route.kind === "search" && !query ? "desktop-search__browse desktop-search__browse--active" : "desktop-search__browse"} onClick={() => { setQuery(""); navigate({ kind: "search" }); }} title="Browse" type="button"><SquareLibrary size={22} /></button>
            </label>
            {showSearchDropdown && (
              <div className="search-dropdown" role="listbox" aria-label="Recent searches">
                <p>Recent searches</p>
                {recentSearches.map((item) => (
                  <div className="search-dropdown__row" key={item}>
                    <button data-search-suggestion onMouseDown={(event) => { event.preventDefault(); setQuery(item); navigate({ kind: "search" }); }} role="option" type="button"><Clock3 size={16} /><span>{item}</span></button>
                    <button aria-label={`Remove ${item}`} onMouseDown={(event) => {
                      event.preventDefault();
                      const next = recentSearches.filter((entry) => entry !== item);
                      localStorage.setItem(searchHistoryKey, JSON.stringify(next));
                      setRecentSearches(next);
                    }} title="Remove" type="button"><X size={14} /></button>
                  </div>
                ))}
                <button className="search-dropdown__clear" onMouseDown={(event) => { event.preventDefault(); localStorage.removeItem(searchHistoryKey); setRecentSearches([]); }} type="button">Clear recent searches</button>
              </div>
            )}
          </div>
        </nav>
        <div className="global-bar__trailing">
          <AccountMenu
            busy={leavingSession}
            host={library.server.displayHost}
            onOpenChange={setAccountOpen}
            onHistory={() => navigate({ kind: "history" })}
            onProfile={() => navigate({ kind: "profile" })}
            onStats={() => navigate({ kind: "stats" })}
            onSettings={() => navigate({ kind: "settings" })}
            onSignOut={() => setSignOutConfirm(true)}
            onSwitchAccount={() => void leaveSession(false)}
            open={accountOpen}
            username={library.server.username}
          />
        </div>
      </header>

      <aside className={compactSidebar ? "desktop-sidebar desktop-sidebar--compact" : "desktop-sidebar"}>
        <div className="sidebar-library-header">
          <button aria-label={compactSidebar ? "Open Your Library" : "Collapse Your Library"} className="sidebar-library-title" onClick={toggleSidebar} title={compactSidebar ? "Open Your Library" : "Collapse Your Library"} type="button">
            {compactSidebar ? <PanelLeftOpen size={22} /> : <PanelLeftClose size={20} />}
            {!compactSidebar && <span>Your Library</span>}
          </button>
          {!compactSidebar && <>
            <button aria-label="Create playlist" className={sidebarWidth >= 340 ? "sidebar-create" : "sidebar-create sidebar-create--icon"} onClick={() => setCreatePlaylistOpen(true)} title="Create a playlist" type="button"><Plus size={18} />{sidebarWidth >= 340 && <span>Create</span>}</button>
            <button aria-label="Show Your Library page" className={route.kind === "library" ? "sidebar-icon-button sidebar-icon-button--active" : "sidebar-icon-button"} onClick={() => navigate({ kind: "library" })} title="Show Your Library page" type="button"><Library size={18} /></button>
          </>}
        </div>
        {compactSidebar && <button aria-label="Create playlist" className="sidebar-create sidebar-create--compact" onClick={() => setCreatePlaylistOpen(true)} title="Create a playlist" type="button"><Plus size={18} /></button>}
        {!compactSidebar && <div className="filter-chips filter-chips--sidebar"><Views.LibraryChips compact filter={libraryFilter} onFilter={setLibraryFilter} /></div>}
        {!compactSidebar && (
          <div className="sidebar-tools">
            {librarySearchOpen || librarySearch ? (
              <label className="sidebar-search">
                <Search size={14} />
                <input aria-label="Search your library" autoFocus onBlur={() => { if (!librarySearch) setLibrarySearchOpen(false); }} onChange={(event) => setLibrarySearch(event.target.value)} onKeyDown={(event) => { if (event.key === "Escape") { event.stopPropagation(); setLibrarySearch(""); setLibrarySearchOpen(false); } }} placeholder="Search in Your Library" value={librarySearch} />
                {librarySearch && <button aria-label="Clear library search" onClick={() => setLibrarySearch("")} type="button"><X size={13} /></button>}
              </label>
            ) : (
              <button aria-label="Search your library" className="sidebar-icon-button" onClick={() => setLibrarySearchOpen(true)} title="Search in Your Library" type="button"><Search size={16} /></button>
            )}
            <div className="sidebar-sort-wrap">
              <button aria-expanded={sortMenuOpen} aria-haspopup="menu" aria-label={`Sort by ${librarySorts.find(([value]) => value === librarySort)?.[1]}`} className="sidebar-sort" onClick={() => setSortMenuOpen((value) => !value)} type="button">
                <span>{librarySorts.find(([value]) => value === librarySort)?.[1]}</span><List size={16} />
              </button>
              {sortMenuOpen && <>
                <button aria-label="Close sort menu" className="context-menu-scrim" onClick={() => setSortMenuOpen(false)} type="button" />
                <div aria-label="Sort by" className="context-menu sort-menu" role="menu">
                  <p className="context-menu__label">Sort by</p>
                  {librarySorts.map(([value, label]) => <button aria-checked={librarySort === value} className={librarySort === value ? "sort-menu__item sort-menu__item--active" : "sort-menu__item"} key={value} onClick={() => { setLibrarySort(value); setSortMenuOpen(false); }} role="menuitemradio" type="button">{label}{librarySort === value && <Check size={15} />}</button>)}
                </div>
              </>}
            </div>
          </div>
        )}
        <div className="sidebar-collections" aria-label="Your library">
          {showLikedRow && likedMatches && <SidebarCollectionRow active={route.kind === "liked"} art={<span className="liked-mini"><Heart fill="currentColor" size={compactSidebar ? 18 : 16} /></span>} onOpen={() => navigate({ kind: "liked" })} onPlay={() => playCollection(shownLiked, "Liked Songs")} pinned subtitle={`Playlist • ${libraryData.starredSongs.length} songs`} title="Liked Songs" />}
          {showLikedRow && downloadsMatches && <SidebarCollectionRow active={route.kind === "downloads"} art={<span className="downloads-mini"><Download size={compactSidebar ? 18 : 16} /></span>} onOpen={() => navigate({ kind: "downloads" })} pinned subtitle={`${downloads.items.length} downloaded`} title="Downloads" />}
          {sidebarLibraryItems.map((item) => {
            if (item.kind === "playlist") {
              const playlist = item.playlist;
              return <SidebarCollectionRow active={route.kind === "playlist" && route.id === playlist.id} className={dropPlaylistId === playlist.id ? "sidebar-collection--drop" : undefined} coverArt={playlist.coverArt} fallback="playlist" key={`playlist:${playlist.id}`} onContextMenu={(event) => cardMenu.playlist(playlist, event)} onDoubleClick={() => void playPlaylist(playlist)} onDragLeave={() => setDropPlaylistId((value) => value === playlist.id ? undefined : value)} onDragOver={(event) => { if (!event.dataTransfer.types.includes(SONG_DRAG_TYPE)) return; event.preventDefault(); event.dataTransfer.dropEffect = "copy"; setDropPlaylistId(playlist.id); }} onDrop={(event) => { event.preventDefault(); setDropPlaylistId(undefined); const ids = readSongDrag(event); if (ids.length) void addSongIdsToPlaylist(ids, playlist); }} onOpen={() => openPlaylist(playlist)} onPlay={() => void playPlaylist(playlist)} pinned={pinned.has(playlist.id)} subtitle={`Playlist${playlist.owner ? ` • ${playlist.owner}` : ""}`} title={playlist.name} />;
            }
            if (item.kind === "artist") {
              const artist = item.artist;
              return <SidebarCollectionRow active={route.kind === "artist" && route.id === artist.id} coverArt={artist.coverArt} fallback="artist" key={`artist:${artist.id}`} onContextMenu={(event) => cardMenu.artist(artist, event)} onOpen={() => openArtist(artist)} onPlay={() => void playArtist(artist)} pinned={pinned.has(artist.id)} shape="circle" subtitle="Artist" title={artist.name} />;
            }
            const album = item.album;
            return <SidebarCollectionRow active={route.kind === "album" && route.id === album.id} coverArt={album.coverArt} key={`album:${album.id}`} onContextMenu={(event) => cardMenu.album(album, event)} onDoubleClick={() => void playAlbum(album)} onOpen={() => openAlbum(album)} onPlay={() => void playAlbum(album)} pinned={pinned.has(album.id)} subtitle={`Album • ${album.artist}`} title={album.title} />;
          })}
        </div>
        <button aria-label="Resize library sidebar" aria-orientation="vertical" aria-valuemax={SIDEBAR_MAX} aria-valuemin={SIDEBAR_COLLAPSED} aria-valuenow={Math.round(sidebarWidth)} className="sidebar-resizer" onKeyDown={resizeSidebarWithKeyboard} onPointerDown={beginResize} role="separator" type="button" />
      </aside>

      <main className={`desktop-workspace desktop-workspace--${route.kind}`} onClick={clearSelectionOnBackground}>
        <div className="desktop-content" onScroll={(event) => { const compact = event.currentTarget.scrollTop > COMPACT_HEADER_AT; if (compact !== headerCompact) setHeaderCompact(compact); }} ref={workspaceRef}>
          {offline && (
            <div className="connection-banner" role="status">
              <span><strong>{settings.offlineMode ? "Offline mode" : "Server unavailable"}</strong><small>{settings.offlineMode ? "Offline mode is on in Settings. Only downloaded music plays." : connection.message ?? "Showing the last library saved on this computer."}</small></span>
              {settings.offlineMode
                ? <button onClick={() => updateSetting("offlineMode", false)} type="button">Go online</button>
                : <button disabled={reconnecting} onClick={() => void retryConnection()} type="button">{reconnecting ? "Reconnecting…" : "Reconnect"}</button>}
            </div>
          )}
          {pageError && <div className="page-error" role="alert">{pageError}<button aria-label="Dismiss error" onClick={() => setPageError(undefined)} type="button"><X size={15} /></button></div>}
          {route.kind === "home" && <Views.HomeView cardMenu={cardMenu} data={homeData} error={homeError} hiddenRows={settings.hiddenHomeRows} jumpBackIn={jumpBackIn} likedAlbums={likedAlbums} onOpen={openAlbum} onOpenJumpBackIn={openJumpBackIn} onOpenShortcut={openHomeShortcut} onPlay={playAlbum} onPlayShortcut={playHomeShortcut} onRetry={reloadHome} rowOrder={settings.homeRowOrder} shortcuts={homeShortcuts} />}
          {route.kind === "search" && <Views.SearchView cardMenu={cardMenu} data={shownSearch} filter={searchFilter} onFilter={setSearchFilter} hasResults={hasSearchResults} home={homeData} isLoading={searching} lyrics={lyricMatches} onPlayLyric={(song) => startPlayback([song], 0, `Lyrics matching "${query.trim()}"`)} onOpenAlbum={openAlbum} onOpenAlbumById={openAlbumById} onOpenArtist={openArtist} onOpenArtistById={openArtistById} onOpenPlaylist={openPlaylist} onPlayAlbum={playAlbum} onPlayArtist={(artist) => void playArtist(artist)} onPlayPlaylist={playPlaylist} onPlaySongs={(songs, index) => startPlayback(songs, index, `Search for ${query}`)} playlists={matchingSearchPlaylists} query={query} songs={songList} />}
          {route.kind === "library" && <Views.LibraryView cardMenu={cardMenu} error={libraryError} filter={libraryFilter} grid={settings.libraryGridView} items={orderedLibraryItems} onToggleGrid={() => updateSetting("libraryGridView", !settings.libraryGridView)} onFilter={setLibraryFilter} onOpenAlbum={openAlbum} onOpenArtist={openArtist} onOpenPlaylist={openPlaylist} onPlayAlbum={playAlbum} onPlayPlaylist={playPlaylist} onRetry={reloadLibrary} />}
          {route.kind === "liked" && <Views.LikedView {...collectionControls} artistName={route.artist?.name} onPlay={(songs, index) => startPlayback(songs, index, route.artist ? `Liked Songs · ${route.artist.name}` : "Liked Songs")} onPlayCollection={playCollection} songList={songList} songs={shownLikedPage} username={library.server.username} />}
          {route.kind === "downloads" && <Views.DownloadsView downloads={downloads} onClear={() => setClearDownloadsConfirm(true)} onPlay={(songs, index) => startPlayback(songs, index, "Downloads")} songList={songList} />}
          {(route.kind === "album" || route.kind === "playlist" || route.kind === "artist") && (pageError && !detail && !pageLoading ? <Views.EmptyState title="This page is unavailable" body="Reconnect to your server, then try again." /> : <Views.DetailView {...collectionControls} artistLikedCount={artistLikedCount} onOpenLikedByArtist={(artist) => navigate({ kind: "liked", artist })} artistArt={detailArtistArt} cardMenu={cardMenu} collectionStarred={collectionStarred} detail={shownDetail} downloads={downloads} isLoading={pageLoading} onDownloadCollection={(songs) => void downloadCollection(songs)} onEnlarge={(coverArt, alt) => setLightbox({ coverArt, alt })} onMore={openCurrentCollectionMenu} onOpenAlbum={openAlbumById} onOpenArtist={openArtistById} onPlay={(songs, index, label) => startPlayback(songs, index, label)} onPlayAlbum={playAlbum} onPlayArtist={(artist) => void playArtist(artist)} onPlayCollection={playCollection} onRadio={startRadio} onRemoveCollection={(songs) => void removeCollection(songs)} onReorder={(from, to) => void reorderCurrentPlaylist(from, to)} onToggleCollectionStar={() => (route.kind === "album" || route.kind === "artist") && void setCollectionStarred(route.kind, route.id, !collectionStarred, detail as AlbumSummary | ArtistSummary | undefined)} ownsPlaylist={ownsCurrentPlaylist} route={route} songList={songList} />)}
          {route.kind === "radio" && <Views.RadioView {...collectionControls} data={shownRadio} isLoading={pageLoading} onPlay={(songs, index) => startPlayback(songs, index, route.title)} onPlayCollection={playCollection} songList={songList} title={route.title} />}
          {route.kind === "profile" && <Views.ProfileView connectionStatus={connection.status} library={library} overview={libraryData} onDevices={() => openPanel("connect")} onHistory={() => navigate({ kind: "history" })} onSettings={() => navigate({ kind: "settings" })} onSignOut={() => void leaveSession(false)} onStats={() => navigate({ kind: "stats" })} />}
          {route.kind === "history" && <HistoryView onPlay={(songs, index, label) => startPlayback(songs, index, label)} plays={playLog} songList={songList} />}
          {route.kind === "stats" && <StatsView input={statsSource} loading={statsLoading} onOpenAlbum={openAlbumById} onOpenArtist={openArtistById} onPlaySong={(song) => startPlayback([song], 0, "Stats")} />}
          {route.kind === "settings" && <Views.SettingsView contextWidth={contextWidth} downloads={downloads} library={library} lyricsIndex={lyricsIndex} onIndexLyrics={() => void invoke("start_lyrics_index", { rebuild: true }).then(() => invoke<LyricsIndexStatus>("lyrics_index_status")).then(setLyricsIndex).catch(() => setToast("Lyrics indexing could not start"))} onClearDownloads={() => setClearDownloadsConfirm(true)} onOpenPanel={openPanel} onReload={() => { reloadHome(); reloadLibrary(); }} onResetLayout={() => { setSidebarWidth(280); setContextWidth(350); setToast("Desktop layout reset"); }} onSleep={(minutes) => { setSleepAt(minutes === undefined ? undefined : Date.now() + minutes * 60_000); setToast(minutes === undefined ? "Sleep timer cancelled" : `Playback stops in ${minutes} min`); }} resetSettings={resetSettings} settings={settings} sidebarWidth={sidebarWidth} sleepRemaining={sleepRemaining} updateSetting={updateSetting} />}
        </div>
      </main>

      {panelMode && <DesktopContextPanel artist={currentArtist} artistFollowed={currentArtistFollowed} autoplay={autoplay} connect={connectState} groupId={groupSession?.id} liked={currentLiked} mode={panelMode} onClose={() => setPanelMode(undefined)} onMoveHere={playPeerHere} onMoveToDevice={movePlaybackTo} onPlayHere={() => void takePlaybackHere()} onOpenAlbum={openAlbumById} onOpenArtist={openArtistById} onOpenQueue={() => openPanel("queue")} onResize={beginContextResize} onResizeKey={resizeContextWithKeyboard} onSaveQueue={() => setSaveQueueOpen(true)} onSend={sendRemote} onStartGroup={startGroup} onStopGroup={stopGroup} onToggleFollow={() => playback.current?.artistId && void setCollectionStarred("artist", playback.current.artistId, !currentArtistFollowed, currentArtist)} onToggleLike={() => playback.current && void toggleSongStar({ ...playback.current, starred: currentLiked ? new Date().toISOString() : undefined })} playback={transport} recentlyPlayed={recentlyPlayed} remoteDeviceId={remoteDevice?.id} width={contextWidth} />}
      <PlayerBar expanded={fullPlayer} liked={currentLiked} lyricsOpen={fullPlayer && fullPlayerSurface === "lyrics"} onOpenDevices={() => openPanel("connect")} remoteDevice={remoteDevice} onOpenAlbum={(id) => { setFullPlayer(false); openAlbumById(id); }} onOpenArtist={(id) => { setFullPlayer(false); openArtistById(id); }} onOpenPanel={openPanel} onToggleExpanded={() => setFullPlayer((value) => !value)} onToggleLike={() => playback.current && void toggleSongStar({ ...playback.current, starred: currentLiked ? new Date().toISOString() : undefined })} onStartRadio={startRadioAfterFailure} onToggleLyrics={toggleLyrics} panelMode={panelMode} playback={transport} />
      {fullPlayer && <FullPlayer onSurface={setFullPlayerSurface} onToggleWindowFullscreen={toggleWindowFullscreen} surface={fullPlayerSurface} windowFullscreen={windowFullscreen} liked={currentLiked} lyrics={lyrics} lyricsAutoScroll={settings.lyricsAutoScroll} lyricsLoading={lyricsLoading} lyricsTextSize={settings.lyricsTextSize} onClose={() => setFullPlayer(false)} onOpenAlbum={(id) => { setFullPlayer(false); openAlbumById(id); }} onOpenArtist={(id) => { setFullPlayer(false); openArtistById(id); }} onOpenPanel={openPanel} onToggleLike={() => playback.current && void toggleSongStar({ ...playback.current, starred: currentLiked ? new Date().toISOString() : undefined })} playback={transport} />}
      {trackMenu && (() => {
        const targets = selectedIds.has(`${trackMenu.song.id}-${trackMenu.index}`) && selectedSongs.length > 1 ? selectedSongs : [trackMenu.song];
        return <><button aria-label="Close track menu" className="context-menu-scrim" onClick={() => setTrackMenu(undefined)} type="button" /><TrackContextMenu
          downloaded={targets.every((song) => downloads.downloadedIds.has(song.id))}
          liked={targets.every((song) => isSongLiked(song))}
          menu={trackMenu}
          onAddToPlaylist={(playlist) => void addSongsToPlaylist(targets, playlist)}
          onClose={() => setTrackMenu(undefined)}
          onCreatePlaylist={() => { setPendingPlaylistSongs(targets); setCreatePlaylistOpen(true); }}
          playlists={visiblePlaylists.filter((playlist) => !playlist.owner || playlist.owner === library.server.username)}
          onDownload={() => targets.length > 1 ? void downloadCollection(targets) : void downloadSong(trackMenu.song)}
          onEnqueue={() => enqueueSongs(targets)}
          onOpenAlbum={trackMenu.song.albumId ? () => openAlbumById(trackMenu.song.albumId!) : undefined}
          onOpenArtist={trackMenu.song.artistId ? () => openArtistById(trackMenu.song.artistId!) : undefined}
          onPlayNext={() => enqueueSongs(targets, true)}
          onRadio={() => startRadio(trackMenu.song)}
          onRemoveDownload={() => targets.length > 1 ? void removeCollection(targets) : void removeDownloadedSong(trackMenu.song)}
          onRemoveFromPlaylist={ownsCurrentPlaylist ? () => targets.length === 1 ? void removeFromCurrentPlaylist(trackMenu.index) : void removeSelectionFromPlaylist() : undefined}
          onToggleLike={() => targets.length > 1 ? void toggleSelectionLike(targets) : void toggleSongStar(trackMenu.song)}
          targetCount={targets.length}
        /></>;
      })()}
      {createPlaylistOpen && <NameDialog confirmLabel="Create" eyebrow="NEW PLAYLIST" onCancel={() => { setCreatePlaylistOpen(false); setPendingPlaylistSongs(undefined); }} onConfirm={createPlaylist} title={pendingPlaylistSongs?.length ? `Create a playlist with ${pendingPlaylistSongs.length === 1 ? pendingPlaylistSongs[0].title : `${pendingPlaylistSongs.length} songs`}` : "Create playlist"} />}
      {saveQueueOpen && <NameDialog confirmLabel="Save" eyebrow="QUEUE" initialValue={playback.contextLabel} onCancel={() => setSaveQueueOpen(false)} onConfirm={(name) => void saveQueueAsPlaylist(name)} title={`Save ${playback.queue.length} songs as a playlist`} />}
      {playlistEdit && (
        <NameDialog
          confirmLabel="Rename"
          eyebrow="PLAYLIST"
          initialValue={playlistEdit.name}
          onCancel={() => setPlaylistEdit(undefined)}
          onConfirm={(name) => { const target = playlistEdit.id; setPlaylistEdit(undefined); void renamePlaylist(target, name); }}
          title="Rename playlist"
        />
      )}
      {playlistDelete && (
        <ConfirmDialog
          body="This deletes the playlist on the server for everyone who can see it. Downloaded songs stay on this computer."
          confirmLabel="Delete playlist"
          eyebrow="PLAYLIST"
          onCancel={() => setPlaylistDelete(undefined)}
          onConfirm={() => void deletePlaylist(playlistDelete.id)}
          title={`Delete ${playlistDelete.name}?`}
        />
      )}
      {clearDownloadsConfirm && (
        <ConfirmDialog
          body="Every downloaded track for this server profile will be removed from this computer. Your server library and playlists will not change."
          confirmLabel="Clear downloads"
          eyebrow="OFFLINE MUSIC"
          onCancel={() => setClearDownloadsConfirm(false)}
          onConfirm={() => void clearAllDownloads()}
          title="Clear all downloads?"
        />
      )}
      {signOutConfirm && (
        <ConfirmDialog
          body={`This removes the saved login for ${library.server.displayHost} from this computer. Downloads and cached artwork stay on disk.`}
          confirmLabel="Sign out"
          eyebrow="ACCOUNT"
          onCancel={() => setSignOutConfirm(false)}
          onConfirm={() => void leaveSession(true)}
          title={`Sign out ${library.server.username}?`}
        />
      )}
      {collectionMenu && <>
        <button aria-label="Close menu" className="context-menu-scrim" onClick={() => setCollectionMenu(undefined)} type="button" />
        <CollectionMenu
          canEdit={libraryData.playlists.some((playlist) => playlist.id === collectionMenu.id && (!playlist.owner || playlist.owner === library.server.username))}
          menu={collectionMenu}
          onClose={() => setCollectionMenu(undefined)}
          onDelete={() => setPlaylistDelete({ id: collectionMenu.id, name: collectionMenu.name })}
          onDownload={() => void runCollectionAction(collectionMenu, "download")}
          onEnqueue={() => void runCollectionAction(collectionMenu, "enqueue")}
          onOpen={"id" in route && route.id === collectionMenu.id ? undefined : () => navigate({ kind: collectionMenu.kind, id: collectionMenu.id })}
          onPin={() => togglePinned(collectionMenu.id)}
          onPlay={() => void runCollectionAction(collectionMenu, "play")}
          onRename={() => setPlaylistEdit({ id: collectionMenu.id, name: collectionMenu.name })}
          onToggleStar={() => collectionMenu.kind !== "playlist" && void setCollectionStarred(collectionMenu.kind, collectionMenu.id, !menuStarred)}
          pinned={pinned.has(collectionMenu.id)}
          starred={menuStarred}
        />
      </>}
      {lightbox && <Views.ArtworkLightbox alt={lightbox.alt} coverArt={lightbox.coverArt} onClose={() => setLightbox(undefined)} />}
      {shortcutsOpen && <ShortcutsDialog onClose={() => setShortcutsOpen(false)} />}
      {toast && <div className="desktop-toast" key={toast} role="status">{toast}</div>}
      {tooltip && <div aria-hidden="true" className={tooltip.below ? "app-tooltip app-tooltip--below" : "app-tooltip"} style={{ left: tooltip.left, top: tooltip.top }}>{tooltip.text}</div>}
    </div>
  );
}

function AccountMenu({ busy, host, onHistory, onOpenChange, onProfile, onSettings, onSignOut, onStats, onSwitchAccount, open, username }: {
  busy: boolean;
  host: string;
  onHistory: () => void;
  onOpenChange: (open: boolean) => void;
  onProfile: () => void;
  onStats: () => void;
  onSettings: () => void;
  onSignOut: () => void;
  onSwitchAccount: () => void;
  open: boolean;
  username: string;
}) {
  const buttonRef = useRef<HTMLButtonElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const frame = window.requestAnimationFrame(() => menuRef.current?.querySelector<HTMLButtonElement>("[role='menuitem']")?.focus());
    return () => window.cancelAnimationFrame(frame);
  }, [open]);
  const choose = (action: () => void) => {
    onOpenChange(false);
    action();
  };
  const onMenuKeyDown = (event: ReactKeyboardEvent<HTMLDivElement>) => {
    const items = Array.from(menuRef.current?.querySelectorAll<HTMLButtonElement>("[role='menuitem']:not(:disabled)") ?? []);
    const current = items.indexOf(document.activeElement as HTMLButtonElement);
    let next = current;
    if (event.key === "ArrowDown") next = current < 0 ? 0 : (current + 1) % items.length;
    else if (event.key === "ArrowUp") next = current < 0 ? items.length - 1 : (current - 1 + items.length) % items.length;
    else if (event.key === "Home") next = 0;
    else if (event.key === "End") next = items.length - 1;
    else if (event.key === "Escape") {
      event.preventDefault();
      onOpenChange(false);
      buttonRef.current?.focus();
      return;
    } else return;
    event.preventDefault();
    items[next]?.focus();
  };
  return (
    <div className="account-menu-wrap">
      <button aria-expanded={open} aria-haspopup="menu" aria-label="Open account menu" className="account-pill" onClick={() => onOpenChange(!open)} ref={buttonRef} title="Account" type="button">{username.slice(0, 1).toUpperCase()}</button>
      {open && <>
        <button aria-label="Close account menu" className="account-menu-scrim" onClick={() => { onOpenChange(false); buttonRef.current?.focus(); }} tabIndex={-1} type="button" />
        <div aria-label="Account" className="account-menu" onKeyDown={onMenuKeyDown} ref={menuRef} role="menu">
          <div className="account-menu__identity"><strong>{username}</strong><small>{host}</small></div>
          <button onClick={() => choose(onProfile)} role="menuitem" type="button"><UserRound size={17} /> Profile</button>
          <button onClick={() => choose(onStats)} role="menuitem" type="button"><BarChart3 size={17} /> Stats</button>
          <button onClick={() => choose(onHistory)} role="menuitem" type="button"><History size={17} /> Listening History</button>
          <button onClick={() => choose(onSettings)} role="menuitem" type="button"><Settings size={17} /> Settings</button>
          <div className="account-menu__separator" />
          <button disabled={busy} onClick={() => choose(onSwitchAccount)} role="menuitem" type="button"><UserRound size={17} /> Switch account</button>
          <button disabled={busy} onClick={() => choose(onSignOut)} role="menuitem" type="button"><LogOut size={17} /> Sign out</button>
        </div>
      </>}
    </div>
  );
}

type SidebarCollectionRowProps = Omit<ComponentPropsWithoutRef<"div">, "title"> & {
  active: boolean;
  art?: ReactNode;
  coverArt?: string;
  fallback?: "album" | "artist" | "playlist";
  onOpen: () => void;
  onPlay?: () => void;
  pinned: boolean;
  shape?: "square" | "circle";
  subtitle: string;
  title: string;
};

/// A library row as Spotify draws it: cover, name, and a subtitle that carries
/// a green pin when the row is pinned. Pinning lives in the right-click menu.
function SidebarCollectionRow({ active, art, className, coverArt, fallback = "album", onOpen, onPlay, pinned, shape = "square", subtitle, title, ...rowProps }: SidebarCollectionRowProps) {
  const classes = ["sidebar-collection", active ? "sidebar-collection--active" : "", className].filter(Boolean).join(" ");
  return (
    <div className={classes} {...rowProps}>
      <button aria-current={active ? "page" : undefined} aria-label={`Open ${title}`} className="sidebar-collection__main" onClick={onOpen} type="button">
        <span className={shape === "circle" ? "sidebar-collection__cover sidebar-collection__cover--circle" : "sidebar-collection__cover"}>{art ?? <MediaArtwork alt="" className="sidebar-collection__art" coverArt={coverArt} fallback={fallback} shape={shape} />}</span>
        <span className="sidebar-collection__copy"><strong>{title}</strong><small>{pinned && <Pin aria-label="Pinned" className="sidebar-collection__pinned" size={12} />}{subtitle}</small></span>
      </button>
      {onPlay && <button aria-label={`Play ${title}`} className={shape === "circle" ? "sidebar-collection__play sidebar-collection__play--circle" : "sidebar-collection__play"} onClick={onPlay} onDoubleClick={(event) => event.stopPropagation()} type="button"><Play fill="currentColor" size={16} /></button>}
    </div>
  );
}

function NameDialog({ confirmLabel, eyebrow, initialValue = "", onCancel, onConfirm, title }: { confirmLabel: string; eyebrow: string; initialValue?: string; onCancel: () => void; onConfirm: (name: string) => void; title: string }) {
  const [name, setName] = useState(initialValue);
  const dialogRef = useDialogFocus<HTMLFormElement>(onCancel);
  return <div className="modal-scrim"><form aria-labelledby="name-dialog-title" aria-modal="true" className="desktop-modal" onSubmit={(event) => { event.preventDefault(); if (name.trim()) onConfirm(name); }} ref={dialogRef} role="dialog"><p className="eyebrow">{eyebrow}</p><h2 id="name-dialog-title">{title}</h2><label>Name<input autoFocus maxLength={120} onChange={(event) => setName(event.target.value)} placeholder="My playlist" value={name} /></label><div><button onClick={onCancel} type="button">Cancel</button><button className="modal-primary" disabled={!name.trim()} type="submit">{confirmLabel}</button></div></form></div>;
}

function ShortcutsDialog({ onClose }: { onClose: () => void }) {
  const dialogRef = useDialogFocus<HTMLDivElement>(onClose);
  return (
    <div className="modal-scrim">
      <div aria-labelledby="shortcuts-title" aria-modal="true" className="desktop-modal desktop-modal--list" ref={dialogRef} role="dialog">
        <h2 id="shortcuts-title">Keyboard shortcuts</h2>
        <dl className="shortcut-list">{Views.shortcutRows.map(([action, keys]) => <div key={action}><dt>{action}</dt><dd>{keys}</dd></div>)}</dl>
        <div><button className="modal-primary" onClick={onClose} type="button">Done</button></div>
      </div>
    </div>
  );
}

function ConfirmDialog({ body, confirmLabel, eyebrow, onCancel, onConfirm, title }: { body: string; confirmLabel: string; eyebrow: string; onCancel: () => void; onConfirm: () => void; title: string }) {
  const dialogRef = useDialogFocus<HTMLDivElement>(onCancel);
  return <div className="modal-scrim"><div aria-labelledby="confirm-dialog-title" aria-modal="true" className="desktop-modal" ref={dialogRef} role="dialog"><p className="eyebrow">{eyebrow}</p><h2 id="confirm-dialog-title">{title}</h2><p className="modal-copy">{body}</p><div><button onClick={onCancel} type="button">Cancel</button><button className="modal-danger" onClick={onConfirm} type="button">{confirmLabel}</button></div></div></div>;
}
