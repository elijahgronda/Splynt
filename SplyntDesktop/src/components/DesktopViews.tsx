import { ChevronRight, Coffee, Disc3, Download, HardDrive, Heart, LayoutGrid, List, LoaderCircle, MoreHorizontal, Pause, Play, Radio, RotateCcw, Shuffle, Trash2, X } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import type { CSSProperties, MouseEvent as ReactMouseEvent, ReactNode } from "react";
import type {
  LyricsIndexStatus, LyricsMatch,
  AlbumDetail, AlbumSummary, ArtistDetail, ArtistSummary, ConnectedLibrary, ContextPanelMode,
  DesktopRoute, HomeOverview, HomeShortcut, JumpBackInItem, LibraryOverview, PlaylistDetail,
  PlaylistSummary, RadioResult, SearchResults, SongSummary,
} from "../types";
import { AlbumCard, ArtistCard, formatLongDuration, LikeGlyph, PlaylistCard, TrackTable } from "./Catalog";
import { MediaArtwork } from "./MediaArtwork";
import { SmoothRange } from "./RangeSlider";
import { EqualizerPanel } from "./EqualizerPanel";
import { useArtworkColor } from "../hooks/useArtworkColor";
import type { DownloadsController } from "../hooks/useDownloads";
import {
  audioFormats, effectiveStreamQuality, homeRowLabels, homeRows, lyricsSources, lyricsTextSizes, qualityTiers, tierDetail,
  tierLabel, transcodes, type AudioFormat, type DesktopSettings, type HomeRow, type QualityTier,
} from "../lib/settings";
import { offlineCacheDegraded } from "../lib/persistence";

export const searchFilters = ["all", "songs", "lyrics", "artists", "albums", "playlists"] as const;
export type SearchFilter = (typeof searchFilters)[number];

export type CardMenu = {
  album: (album: AlbumSummary, event: ReactMouseEvent) => void;
  artist: (artist: ArtistSummary, event: ReactMouseEvent) => void;
  playlist: (playlist: PlaylistSummary, event: ReactMouseEvent) => void;
};

export type DetailState = AlbumDetail | PlaylistDetail | ArtistDetail;
export type LibraryFilter = "playlists" | "artists" | "albums";

export type SongListHandlers = {
  currentId?: string;
  isPlaying: boolean;
  isLiked: (song: SongSummary) => boolean;
  onMenu: (song: SongSummary, index: number, event: ReactMouseEvent) => void;
  onSelect: (song: SongSummary, index: number, event: ReactMouseEvent) => void;
  onToggleStar: (song: SongSummary) => void;
  selectedIds: Set<string>;
  showQuality: boolean;
};

type CollectionControls = {
  compactHeader: boolean;
  onTogglePlayback: () => void;
  onToggleShuffle: () => void;
  shuffleArmed: boolean;
};

export function PageHeading({ eyebrow, title, subtitle }: { eyebrow?: string; title: string; subtitle?: string }) {
  return <div className="desktop-content__heading">{eyebrow && <p className="eyebrow">{eyebrow}</p>}<h1>{title}</h1>{subtitle && <p>{subtitle}</p>}</div>;
}

/// The shared Spotify collection layout: a header tinted from the cover, a wash
/// that keeps fading behind the action row and track list, and a compact
/// play-and-title strip that pins to the top once the header scrolls away.
function CollectionPage({ actions, art, children, color, compact, meta, onPlay, playing, title, type }: {
  actions: ReactNode;
  art: ReactNode;
  children: ReactNode;
  color?: string;
  compact: boolean;
  meta: ReactNode;
  onPlay?: () => void;
  playing: boolean;
  title: string;
  type: string;
}) {
  const style = { "--hero-color": color ?? "rgb(83, 83, 83)" } as CSSProperties;
  return (
    <div className="collection-page" style={style}>
      <div aria-hidden={!compact} className={compact ? "collection-sticky collection-sticky--shown" : "collection-sticky"}>
        {onPlay && <button aria-label={playing ? `Pause ${title}` : `Play ${title}`} className="primary-play primary-play--small" onClick={onPlay} tabIndex={compact ? 0 : -1} type="button">{playing ? <Pause fill="currentColor" size={18} /> : <Play fill="currentColor" size={18} />}</button>}
        <strong>{title}</strong>
      </div>
      <header className="collection-header">
        {art}
        <div className="collection-header__copy">
          <p className="collection-header__type">{type}</p>
          <h1 className={title.length > 28 ? "collection-header__title collection-header__title--long" : "collection-header__title"}>{title}</h1>
          <div className="collection-header__meta">{meta}</div>
        </div>
      </header>
      <div className="collection-body">
        <div className="collection-actions">{actions}</div>
        {children}
      </div>
    </div>
  );
}

function PlayButton({ label, onClick, playing }: { label: string; onClick: () => void; playing: boolean }) {
  return <button aria-label={playing ? `Pause ${label}` : `Play ${label}`} className="primary-play" onClick={onClick} title={playing ? "Pause" : "Play"} type="button">{playing ? <Pause fill="currentColor" size={24} /> : <Play fill="currentColor" size={24} />}</button>;
}

function MoreButton({ label, onClick }: { label: string; onClick: (event: ReactMouseEvent) => void }) {
  return <button aria-label={`More options for ${label}`} className="collection-icon-action" onClick={onClick} title={`More options for ${label}`} type="button"><MoreHorizontal size={26} /></button>;
}

function playsFrom(songs: SongSummary[], currentId: string | undefined, isPlaying: boolean) {
  return isPlaying && Boolean(currentId) && songs.some((song) => song.id === currentId);
}

export function HomeView({ cardMenu, data, error, hiddenRows, jumpBackIn, likedAlbums, onOpen, onOpenJumpBackIn, onOpenShortcut, onPlay, onPlayShortcut, onRetry, rowOrder, shortcuts }: {
  cardMenu: CardMenu;
  data: HomeOverview;
  error?: string;
  hiddenRows: HomeRow[];
  jumpBackIn: JumpBackInItem[];
  likedAlbums: AlbumSummary[];
  onOpen: (album: AlbumSummary) => void;
  onOpenJumpBackIn: (item: JumpBackInItem) => void;
  onOpenShortcut: (item: HomeShortcut) => void;
  onPlay: (album: AlbumSummary) => void;
  onPlayShortcut: (item: HomeShortcut) => void;
  onRetry: () => void;
  rowOrder: HomeRow[];
  shortcuts: HomeShortcut[];
}) {
  type AlbumHomeRow = Exclude<HomeRow, "jumpBackIn">;
  const named: Record<AlbumHomeRow, Array<readonly [string, AlbumSummary[]]>> = {
    recentlyPlayed: [[homeRowLabels.recentlyPlayed, data.recent]],
    recentlyAdded: [[homeRowLabels.recentlyAdded, data.newest]],
    onRepeat: [[homeRowLabels.onRepeat, data.frequent]],
    genreMixes: (data.genres ?? []).map((shelf) => [shelf.name, shelf.albums] as const),
    albumsFeaturingLiked: [[homeRowLabels.albumsFeaturingLiked, likedAlbums]],
    discover: [[homeRowLabels.discover, data.random]],
  };
  const visibleRows = rowOrder.filter((row) => !hiddenRows.includes(row));
  const shelves = visibleRows
    .filter((row): row is AlbumHomeRow => row !== "jumpBackIn")
    .flatMap((row) => named[row]);
  // Match iOS' cooldown rule: an album already visible in the leading server
  // history should not immediately repeat in Jump back in beneath it.
  const leadingRecentAlbums = new Set(data.recent.slice(0, 8).map((album) => album.id));
  const distinctJumpBackIn = jumpBackIn.filter((item) => item.kind !== "album" || !leadingRecentAlbums.has(item.id));
  const jumpBackInVisible = visibleRows.includes("jumpBackIn") && distinctJumpBackIn.length > 0;
  return <>
    {shortcuts.length > 0 && <div aria-label="Quick access" className="home-quick-grid">{shortcuts.map((item) => <article key={`${item.kind}:${item.id}`}>
      <button aria-label={`Open ${item.title}`} className="home-quick-grid__open" onClick={() => onOpenShortcut(item)} type="button">
        {item.kind === "liked" ? <span className="home-quick-grid__art liked-mini"><Heart fill="currentColor" size={18} /></span> : <MediaArtwork alt="" className="home-quick-grid__art" coverArt={item.coverArt} fallback={item.kind === "playlist" ? "playlist" : "album"} />}
        <strong>{item.title}</strong>
      </button>
      <button aria-label={`Play ${item.title}`} className="home-quick-grid__play" onClick={() => onPlayShortcut(item)} type="button"><Play fill="currentColor" size={16} /></button>
    </article>)}</div>}
    {error && <RetryState message={error} onRetry={onRetry} />}
    {visibleRows.map((row) => row === "jumpBackIn"
      ? distinctJumpBackIn.length > 0 && <section className="catalog-shelf" key={row}><h2>{homeRowLabels.jumpBackIn}</h2><div className="catalog-row">{distinctJumpBackIn.map((item) => <article className={`catalog-card${item.kind === "artist" ? " catalog-card--artist" : ""}`} key={`${item.kind}:${item.id}`}>
        <button className="catalog-card__body" onClick={() => onOpenJumpBackIn(item)} type="button">
          <MediaArtwork alt="" coverArt={item.coverArt} fallback={item.kind} shape={item.kind === "artist" ? "circle" : "square"} />
          <strong title={item.title}>{item.title}</strong><span title={item.subtitle}>{item.subtitle}</span>
        </button>
      </article>)}</div></section>
      : named[row].map(([name, albums]) => albums.length > 0 && <section className="catalog-shelf" key={`${row}:${name}`}><h2>{name}</h2><div className="catalog-row">{albums.map((album) => <AlbumCard album={album} key={`${name}-${album.id}`} onMenu={(event) => cardMenu.album(album, event)} onOpen={onOpen} onPlay={onPlay} />)}</div></section>))}
    {!error && !shortcuts.length && !jumpBackInVisible && shelves.every(([, albums]) => albums.length === 0) && <EmptyState title="Nothing to show on Home" body="Add music to the connected server, or turn a shelf back on in Settings." />}
  </>;
}

type TopResult =
  | { kind: "artist"; artist: ArtistSummary }
  | { kind: "album"; album: AlbumSummary }
  | { kind: "song"; song: SongSummary }
  | { kind: "playlist"; playlist: PlaylistSummary };

/// Exact names win, the way Spotify ranks them: an artist called what you typed
/// beats a song that merely mentions it.
export function pickTopResult(query: string, data: SearchResults, playlists: PlaylistSummary[]): TopResult | undefined {
  const needle = query.trim().toLowerCase();
  const exactArtist = data.artists.find((artist) => artist.name.toLowerCase() === needle);
  if (exactArtist) return { kind: "artist", artist: exactArtist };
  const exactAlbum = data.albums.find((album) => album.title.toLowerCase() === needle);
  if (exactAlbum) return { kind: "album", album: exactAlbum };
  const exactSong = data.songs.find((song) => song.title.toLowerCase() === needle);
  if (exactSong) return { kind: "song", song: exactSong };
  const leadingArtist = data.artists.find((artist) => artist.name.toLowerCase().startsWith(needle));
  if (leadingArtist) return { kind: "artist", artist: leadingArtist };
  if (data.songs[0]) return { kind: "song", song: data.songs[0] };
  if (data.albums[0]) return { kind: "album", album: data.albums[0] };
  if (data.artists[0]) return { kind: "artist", artist: data.artists[0] };
  return playlists[0] ? { kind: "playlist", playlist: playlists[0] } : undefined;
}

/// Songs found by their lyrics, each with the matching words in context.
function LyricsMatches({ matches, onPlay }: { matches: LyricsMatch[]; onPlay: (song: SongSummary) => void }) {
  return (
    <ol className="lyrics-matches">
      {matches.map((match) => (
        <li key={match.song.id}>
          <button data-search-result onClick={() => onPlay(match.song)} type="button">
            <MediaArtwork alt="" className="lyrics-matches__art" coverArt={match.song.coverArt} />
            <span><strong>{match.song.title}</strong><small>{match.song.artist}</small><em>{match.snippet}</em></span>
          </button>
        </li>
      ))}
    </ol>
  );
}

export function SearchView({ cardMenu, data, filter, hasResults, home, isLoading, lyrics, onFilter, onOpenAlbum, onOpenAlbumById, onOpenArtist, onOpenArtistById, onOpenPlaylist, onPlayAlbum, onPlayArtist, onPlayLyric, onPlayPlaylist, onPlaySongs, playlists, query, songs }: {
  lyrics: LyricsMatch[]; onPlayLyric: (song: SongSummary) => void;
  filter: SearchFilter; onFilter: (filter: SearchFilter) => void;
  cardMenu: CardMenu; data: SearchResults; hasResults: boolean; home: HomeOverview; isLoading: boolean;
  onOpenAlbum: (album: AlbumSummary) => void; onOpenArtist: (artist: ArtistSummary) => void; onOpenPlaylist: (playlist: PlaylistSummary) => void;
  onOpenAlbumById: (id: string) => void; onOpenArtistById: (id: string) => void;
  onPlayAlbum: (album: AlbumSummary) => void; onPlayArtist: (artist: ArtistSummary) => void; onPlayPlaylist: (playlist: PlaylistSummary) => void; onPlaySongs: (songs: SongSummary[], index: number) => void;
  playlists: PlaylistSummary[]; query: string; songs: SongListHandlers;
}) {
  useEffect(() => {
    const move = (event: KeyboardEvent) => {
      if (!(event.target instanceof HTMLElement) || !event.target.matches("[data-search-result]")) return;
      if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
      const results = Array.from(document.querySelectorAll<HTMLElement>("[data-search-result]"));
      const index = results.indexOf(event.target);
      const next = event.key === "ArrowDown" ? Math.min(results.length - 1, index + 1) : Math.max(0, index - 1);
      event.preventDefault();
      results[next]?.focus();
    };
    window.addEventListener("keydown", move);
    return () => window.removeEventListener("keydown", move);
  }, []);
  const table = (list: SongSummary[], compact: boolean) => <TrackTable currentId={songs.currentId} isLiked={songs.isLiked} isPlaying={songs.isPlaying} onMenu={songs.onMenu} onOpenAlbum={onOpenAlbumById} onOpenArtist={onOpenArtistById} onPlay={(index) => onPlaySongs(data.songs, index)} onSelect={songs.onSelect} onToggleStar={songs.onToggleStar} selectedIds={songs.selectedIds} showAlbum={!compact} showArtwork showQuality={songs.showQuality && !compact} songs={list} />;

  if (!query) {
    return <>
      <section className="catalog-shelf"><h2>Browse your library</h2><div className="catalog-grid">{home.random.slice(0, 12).map((album) => <AlbumCard album={album} key={album.id} onMenu={(event) => cardMenu.album(album, event)} onOpen={onOpenAlbum} onPlay={onPlayAlbum} />)}</div></section>
    </>;
  }
  const top = pickTopResult(query, data, playlists);
  return <>
    {!isLoading && hasResults && <div className="filter-chips filter-chips--sticky">{searchFilters.map((value) => <button aria-pressed={filter === value} className={filter === value ? "filter-chip filter-chip--active" : "filter-chip"} key={value} onClick={() => onFilter(value)} type="button">{value === "all" ? "All" : value[0].toUpperCase() + value.slice(1)}</button>)}</div>}
    {isLoading && <LoadingState label="Searching your server" />}
    {!isLoading && !hasResults && <EmptyState title={`No results found for "${query}"`} body="Please make sure your words are spelled correctly, or use fewer or different keywords." />}
    {!isLoading && hasResults && filter === "all" && <div className="search-top">
      {top && <section className="top-result"><h2>Top result</h2><TopResultCard onOpenAlbum={onOpenAlbum} onOpenArtist={onOpenArtist} onOpenPlaylist={onOpenPlaylist} onPlayAlbum={onPlayAlbum} onPlayArtist={onPlayArtist} onPlayPlaylist={onPlayPlaylist} onPlaySong={() => top.kind === "song" && onPlaySongs(data.songs, Math.max(0, data.songs.indexOf(top.song)))} result={top} /></section>}
      {data.songs.length > 0 && <section className="result-section search-top__songs"><h2>Songs</h2>{table(data.songs.slice(0, 4), true)}</section>}
    </div>}
    {!isLoading && data.songs.length > 0 && filter === "songs" && <section className="result-section">{table(data.songs, false)}</section>}
    {!isLoading && filter === "all" && (() => {
      // Songs already shown above are not repeated as lyric matches.
      const shown = new Set(data.songs.slice(0, 4).map((song) => song.id));
      const rest = lyrics.filter((match) => !shown.has(match.song.id)).slice(0, 10);
      return rest.length > 0 && <section className="result-section"><h2>Lyrics matches</h2><LyricsMatches matches={rest} onPlay={onPlayLyric} /></section>;
    })()}
    {!isLoading && filter === "lyrics" && (lyrics.length > 0 ? <section className="result-section"><LyricsMatches matches={lyrics} onPlay={onPlayLyric} /></section> : <EmptyState title="No lyrics match yet" body="Lyrics become searchable as Splynt indexes your server. Settings shows how far it has got." />)}
    {!isLoading && data.artists.length > 0 && (filter === "all" || filter === "artists") && <section className="catalog-shelf"><h2>Artists</h2><div className={filter === "all" ? "catalog-row" : "catalog-grid"}>{data.artists.map((artist) => <ArtistCard artist={artist} key={artist.id} onMenu={(event) => cardMenu.artist(artist, event)} onOpen={onOpenArtist} />)}</div></section>}
    {!isLoading && data.albums.length > 0 && (filter === "all" || filter === "albums") && <section className="catalog-shelf"><h2>Albums</h2><div className={filter === "all" ? "catalog-row" : "catalog-grid"}>{data.albums.map((album) => <AlbumCard album={album} key={album.id} onMenu={(event) => cardMenu.album(album, event)} onOpen={onOpenAlbum} onPlay={onPlayAlbum} />)}</div></section>}
    {!isLoading && playlists.length > 0 && (filter === "all" || filter === "playlists") && <section className="catalog-shelf"><h2>Playlists</h2><div className={filter === "all" ? "catalog-row" : "catalog-grid"}>{playlists.map((playlist) => <PlaylistCard key={playlist.id} onMenu={(event) => cardMenu.playlist(playlist, event)} onOpen={onOpenPlaylist} onPlay={onPlayPlaylist} playlist={playlist} />)}</div></section>}
  </>;
}

function TopResultCard({ onOpenAlbum, onOpenArtist, onOpenPlaylist, onPlayAlbum, onPlayArtist, onPlayPlaylist, onPlaySong, result }: {
  onOpenAlbum: (album: AlbumSummary) => void; onOpenArtist: (artist: ArtistSummary) => void; onOpenPlaylist: (playlist: PlaylistSummary) => void;
  onPlayAlbum: (album: AlbumSummary) => void; onPlayArtist: (artist: ArtistSummary) => void; onPlayPlaylist: (playlist: PlaylistSummary) => void; onPlaySong: () => void;
  result: TopResult;
}) {
  let art: ReactNode; let name: string; let subtitle: ReactNode; let open: () => void; let play: () => void;
  if (result.kind === "artist") {
    art = <MediaArtwork alt="" className="top-result__art" coverArt={result.artist.coverArt} fallback="artist" shape="circle" />;
    name = result.artist.name; subtitle = <span className="top-result__kind">Artist</span>;
    open = () => onOpenArtist(result.artist); play = () => onPlayArtist(result.artist);
  } else if (result.kind === "album") {
    art = <MediaArtwork alt="" className="top-result__art" coverArt={result.album.coverArt} />;
    name = result.album.title; subtitle = <><span className="top-result__kind">Album</span> • {result.album.artist}</>;
    open = () => onOpenAlbum(result.album); play = () => onPlayAlbum(result.album);
  } else if (result.kind === "playlist") {
    art = <MediaArtwork alt="" className="top-result__art" coverArt={result.playlist.coverArt} fallback="playlist" />;
    name = result.playlist.name; subtitle = <><span className="top-result__kind">Playlist</span> • {result.playlist.owner ?? "Playlist"}</>;
    open = () => onOpenPlaylist(result.playlist); play = () => onPlayPlaylist(result.playlist);
  } else {
    const song = result.song;
    art = <MediaArtwork alt="" className="top-result__art" coverArt={song.coverArt} />;
    name = song.title; subtitle = <><span className="top-result__kind">Song</span> • {song.artist}</>;
    open = () => song.albumId ? onOpenAlbum({ id: song.albumId, title: song.album, artist: song.artist, coverArt: song.coverArt }) : onPlaySong();
    play = onPlaySong;
  }
  return (
    <article className="top-result__card">
      <button className="top-result__open" data-search-result onClick={open} type="button">
        {art}
        <strong>{name}</strong>
        <span className="top-result__subtitle">{subtitle}</span>
      </button>
      <button aria-label={`Play ${name}`} className="catalog-card__play top-result__play" onClick={play} type="button"><Play fill="currentColor" size={21} /></button>
    </article>
  );
}

export type LibraryItem =
  | { kind: "playlist"; id: string; name: string; playlist: PlaylistSummary }
  | { kind: "album"; id: string; name: string; album: AlbumSummary }
  | { kind: "artist"; id: string; name: string; artist: ArtistSummary };

export function LibraryView({ cardMenu, error, filter, grid, items, onFilter, onOpenAlbum, onOpenArtist, onOpenPlaylist, onPlayAlbum, onPlayPlaylist, onRetry, onToggleGrid }: { cardMenu: CardMenu; error?: string; filter?: LibraryFilter; grid: boolean; items: LibraryItem[]; onFilter: (filter?: LibraryFilter) => void; onOpenAlbum: (album: AlbumSummary) => void; onOpenArtist: (artist: ArtistSummary) => void; onOpenPlaylist: (playlist: PlaylistSummary) => void; onPlayAlbum: (album: AlbumSummary) => void; onPlayPlaylist: (playlist: PlaylistSummary) => void; onRetry: () => void; onToggleGrid: () => void }) {
  const [visibleCount, setVisibleCount] = useState(240);
  useEffect(() => setVisibleCount(240), [filter, items.length]);
  const visible = items.slice(0, visibleCount);
  const noun = filter ?? "items";
  return <>
    <PageHeading title="Your Library" />
    <div className="filter-chips">
      <LibraryChips filter={filter} onFilter={onFilter} />
      <button className="filter-chip filter-chip--sort" onClick={onToggleGrid} title={grid ? "Show as list" : "Show as grid"} type="button">{grid ? <List size={13} /> : <LayoutGrid size={13} />}{grid ? "List" : "Grid"}</button>
    </div>
    {error && <RetryState message={error} onRetry={onRetry} />}
    <div className={grid ? "catalog-grid" : "catalog-list"}>{visible.map((item) => item.kind === "playlist"
      ? <PlaylistCard key={`playlist:${item.id}`} onMenu={(event) => cardMenu.playlist(item.playlist, event)} onOpen={onOpenPlaylist} onPlay={onPlayPlaylist} playlist={item.playlist} />
      : item.kind === "album"
        ? <AlbumCard album={item.album} key={`album:${item.id}`} onMenu={(event) => cardMenu.album(item.album, event)} onOpen={onOpenAlbum} onPlay={onPlayAlbum} />
        : <ArtistCard artist={item.artist} key={`artist:${item.id}`} onMenu={(event) => cardMenu.artist(item.artist, event)} onOpen={onOpenArtist} />)}</div>
    {visibleCount < items.length && <div className="load-more"><button onClick={() => setVisibleCount((count) => Math.min(items.length, count + 240))} type="button">Show more</button><span>{visible.length.toLocaleString()} of {items.length.toLocaleString()}</span></div>}
    {!error && items.length === 0 && <EmptyState title={`No ${noun} found`} body={`Your server did not return any ${noun}.`} />}
  </>;
}

/// Spotify's library chips: none selected shows everything, a selected chip
/// narrows it, and the ✕ in front clears the filter.
export function LibraryChips({ compact, filter, onFilter }: { compact?: boolean; filter?: LibraryFilter; onFilter: (filter?: LibraryFilter) => void }) {
  const values: LibraryFilter[] = filter ? [filter] : ["playlists", "artists", "albums"];
  return <>
    {filter && <button aria-label="Clear filter" className={compact ? "filter-chip filter-chip--clear filter-chip--small" : "filter-chip filter-chip--clear"} onClick={() => onFilter(undefined)} title="Clear filter" type="button"><X size={compact ? 14 : 16} /></button>}
    {values.map((value) => <button aria-pressed={filter === value} className={`${filter === value ? "filter-chip filter-chip--active" : "filter-chip"}${compact ? " filter-chip--small" : ""}`} key={value} onClick={() => onFilter(filter === value ? undefined : value)} title={`Show ${value}`} type="button">{value[0].toUpperCase() + value.slice(1)}</button>)}
  </>;
}

export function LikedView({ artistName, compactHeader, onPlay, onPlayCollection, onTogglePlayback, onToggleShuffle, shuffleArmed, songs, songList, username }: CollectionControls & { artistName?: string; onPlay: (songs: SongSummary[], index: number) => void; onPlayCollection: (songs: SongSummary[], label: string) => void; songs: SongSummary[]; songList: SongListHandlers; username: string }) {
  const playing = playsFrom(songs, songList.currentId, songList.isPlaying);
  const label = artistName ? `Liked Songs · ${artistName}` : "Liked Songs";
  const play = () => playing ? onTogglePlayback() : onPlayCollection(songs, label);
  return (
    <CollectionPage
      actions={songs.length > 0 && <><PlayButton label={label} onClick={play} playing={playing} /><ShuffleToggle armed={shuffleArmed} onToggle={onToggleShuffle} /></>}
      art={<span className="collection-header__art liked-cover"><Heart fill="currentColor" size={64} /></span>}
      color="rgb(80, 56, 160)"
      compact={compactHeader}
      meta={<><strong>{artistName ?? username}</strong><span> • {songs.length} {songs.length === 1 ? "song" : "songs"}</span></>}
      onPlay={songs.length ? play : undefined}
      playing={playing}
      title="Liked Songs"
      type="Playlist"
    >
      {songs.length ? <TrackTable currentId={songList.currentId} isLiked={songList.isLiked} isPlaying={songList.isPlaying} onMenu={songList.onMenu} onPlay={(index) => onPlay(songs, index)} onSelect={songList.onSelect} onToggleStar={songList.onToggleStar} selectedIds={songList.selectedIds} showDateAdded showQuality={songList.showQuality} songs={songs} /> : (artistName ? <EmptyState title={`No liked songs by ${artistName}`} body="Songs you like by this artist will appear here." /> : <EmptyState title="Songs you like will appear here" body="Save songs by tapping the plus icon." />)}
    </CollectionPage>
  );
}

export function DetailView({ artistArt, artistLikedCount, onOpenLikedByArtist, cardMenu, collectionStarred, compactHeader, detail, downloads, isLoading, onDownloadCollection, onEnlarge, onMore, onOpenAlbum, onOpenArtist, onPlay, onPlayAlbum, onPlayArtist, onPlayCollection, onRadio, onRemoveCollection, onReorder, onToggleCollectionStar, onTogglePlayback, onToggleShuffle, ownsPlaylist, route, shuffleArmed, songList }: CollectionControls & {
  artistArt?: string;
  artistLikedCount: number;
  onOpenLikedByArtist: (artist: { id: string; name: string }) => void;
  cardMenu: CardMenu;
  collectionStarred: boolean;
  detail?: DetailState;
  downloads: DownloadsController;
  isLoading: boolean;
  onDownloadCollection: (songs: SongSummary[]) => void;
  onEnlarge: (coverArt: string | undefined, alt: string) => void;
  onMore: (event: ReactMouseEvent) => void;
  onOpenAlbum: (id: string) => void;
  onOpenArtist: (id: string) => void;
  onPlay: (songs: SongSummary[], index: number, label: string) => void;
  onPlayAlbum: (album: AlbumSummary) => void;
  onPlayArtist: (artist: ArtistDetail) => void;
  onPlayCollection: (songs: SongSummary[], label: string) => void;
  onRadio: (song: SongSummary) => void;
  onRemoveCollection: (songs: SongSummary[]) => void;
  onReorder: (from: number, to: number) => void;
  onToggleCollectionStar: () => void;
  ownsPlaylist: boolean;
  route: DesktopRoute;
  songList: SongListHandlers;
}) {
  const color = useArtworkColor(detail?.coverArt);
  // The previous page's detail is still in state for the first render after a
  // navigation; rendering it under the new route's layout reads the wrong shape.
  const stale = !detail || !("id" in route) || detail.id !== route.id;
  if (isLoading || stale) return <LoadingState label="Loading from your server" />;
  if (route.kind === "artist" && "albums" in detail) {
    const topSongs = detail.topSongs ?? [];
    const canPlay = topSongs.length > 0 || detail.albums.length > 0;
    const playing = playsFrom(topSongs, songList.currentId, songList.isPlaying);
    const play = () => playing ? onTogglePlayback() : onPlayArtist(detail);
    return (
      <CollectionPage
        actions={<>
          {canPlay && <PlayButton label={detail.name} onClick={play} playing={playing} />}
          {canPlay && <ShuffleToggle armed={shuffleArmed} onToggle={onToggleShuffle} />}
          <button aria-pressed={collectionStarred} className={collectionStarred ? "pill-button pill-button--active" : "pill-button"} onClick={onToggleCollectionStar} type="button">{collectionStarred ? "Following" : "Follow"}</button>
          <MoreButton label={detail.name} onClick={onMore} />
        </>}
        art={<button aria-label={`Enlarge ${detail.name} portrait`} className="hero-art-button" onClick={() => onEnlarge(detail.coverArt, `${detail.name} portrait`)} type="button"><MediaArtwork alt={`${detail.name} portrait`} className="collection-header__art collection-header__art--circle" coverArt={detail.coverArt} fallback="artist" shape="circle" /></button>}
        color={color}
        compact={compactHeader}
        meta={<span>{detail.albumCount ?? detail.albums.length} {(detail.albumCount ?? detail.albums.length) === 1 ? "album" : "albums"} in your library</span>}
        onPlay={canPlay ? play : undefined}
        playing={playing}
        title={detail.name}
        type="Artist"
      >
        {artistLikedCount > 0 && (
          <button className="liked-by-artist" onClick={() => onOpenLikedByArtist({ id: detail.id, name: detail.name })} type="button">
            <span className="liked-by-artist__art">
              <MediaArtwork alt="" coverArt={detail.coverArt} fallback="artist" shape="circle" />
              <span aria-hidden="true"><Heart fill="currentColor" size={10} /></span>
            </span>
            <span className="liked-by-artist__text"><strong>You liked</strong><small>{artistLikedCount} {artistLikedCount === 1 ? "song" : "songs"} by {detail.name}</small></span>
            <ChevronRight aria-hidden="true" size={16} />
          </button>
        )}
        {topSongs.length > 0 && <section className="result-section"><h2>Popular</h2><TrackTable currentId={songList.currentId} isLiked={songList.isLiked} isPlaying={songList.isPlaying} onMenu={songList.onMenu} onOpenAlbum={onOpenAlbum} onOpenArtist={onOpenArtist} onPlay={(index) => onPlay(topSongs, index, detail.name)} onSelect={songList.onSelect} onToggleStar={songList.onToggleStar} selectedIds={songList.selectedIds} showAlbum={false} showArtwork songs={topSongs.slice(0, 10)} /></section>}
        {topSongs[0] && <div className="collection-inline-actions"><button className="pill-button" onClick={() => onRadio(topSongs[0])} type="button"><Radio size={15} /> Artist radio</button></div>}
        <section className="catalog-shelf"><h2>Discography</h2><div className="catalog-grid">{detail.albums.map((album) => <AlbumCard album={album} key={album.id} onMenu={(event) => cardMenu.album(album, event)} onOpen={() => onOpenAlbum(album.id)} onPlay={onPlayAlbum} />)}</div></section>
        {detail.appearances?.length ? <section className="catalog-shelf"><h2>Appears on</h2><div className="catalog-grid">{detail.appearances.map((album) => <AlbumCard album={album} key={album.id} onMenu={(event) => cardMenu.album(album, event)} onOpen={() => onOpenAlbum(album.id)} onPlay={onPlayAlbum} />)}</div></section> : null}
      </CollectionPage>
    );
  }
  if (!("songs" in detail)) return null;
  const isAlbum = route.kind === "album";
  const album = isAlbum ? detail as AlbumDetail : undefined;
  const playlist = isAlbum ? undefined : detail as PlaylistDetail;
  const title = album ? album.title : playlist!.name;
  const allDownloaded = detail.songs.length > 0 && detail.songs.every((song) => downloads.downloadedIds.has(song.id));
  const total = detail.duration ?? detail.songs.reduce((sum, song) => sum + (song.duration ?? 0), 0);
  const length = formatLongDuration(total);
  const playing = playsFrom(detail.songs, songList.currentId, songList.isPlaying);
  const play = () => playing ? onTogglePlayback() : onPlayCollection(detail.songs, title);
  const meta = album ? <>
    {album.artistId ? <button className="collection-header__byline" onClick={() => onOpenArtist(album.artistId!)} type="button"><MediaArtwork alt="" className="collection-header__avatar" coverArt={artistArt ?? album.coverArt} fallback="artist" shape="circle" /><strong>{album.artist}</strong></button> : <strong>{album.artist}</strong>}
    {album.year ? <span> • {album.year}</span> : null}
    <span> • {detail.songs.length} {detail.songs.length === 1 ? "song" : "songs"}{length ? `, ${length}` : ""}</span>
  </> : <>
    <strong>{playlist!.owner ?? "Playlist"}</strong>
    <span> • {detail.songs.length} {detail.songs.length === 1 ? "song" : "songs"}{length ? `, ${length}` : ""}</span>
  </>;
  return (
    <CollectionPage
      actions={<>
        {detail.songs.length > 0 && <PlayButton label={title} onClick={play} playing={playing} />}
        {detail.songs[0] && <ShuffleToggle armed={shuffleArmed} onToggle={onToggleShuffle} />}
        {isAlbum && <button aria-label={collectionStarred ? "Remove from Your Library" : "Save to Your Library"} aria-pressed={collectionStarred} className={collectionStarred ? "collection-icon-action collection-icon-action--active" : "collection-icon-action"} onClick={onToggleCollectionStar} title={collectionStarred ? "Remove from Your Library" : "Save to Your Library"} type="button"><LikeGlyph liked={collectionStarred} size={30} /></button>}
        {detail.songs.length > 0 && <button aria-label={allDownloaded ? "Remove download" : "Download"} className={allDownloaded ? "collection-icon-action collection-icon-action--active" : "collection-icon-action"} onClick={() => allDownloaded ? onRemoveCollection(detail.songs) : onDownloadCollection(detail.songs)} title={allDownloaded ? "Remove download" : "Download"} type="button"><Download size={26} /></button>}
        <MoreButton label={title} onClick={onMore} />
      </>}
      art={<button aria-label={`Enlarge ${title} cover`} className="hero-art-button" onClick={() => onEnlarge(detail.coverArt, `${title} cover`)} type="button"><MediaArtwork alt={`${title} cover`} className="collection-header__art" coverArt={detail.coverArt} fallback={isAlbum ? "album" : "playlist"} /></button>}
      color={color}
      compact={compactHeader}
      meta={meta}
      onPlay={detail.songs.length ? play : undefined}
      playing={playing}
      title={title}
      type={isAlbum ? "Album" : playlist?.public === false ? "Private Playlist" : "Public Playlist"}
    >
      {detail.songs.length ? <TrackTable currentId={songList.currentId} downloadProgress={downloads.progress} downloadedIds={downloads.downloadedIds} isLiked={songList.isLiked} isPlaying={songList.isPlaying} onMenu={songList.onMenu} onOpenAlbum={onOpenAlbum} onOpenArtist={onOpenArtist} onPlay={(index) => onPlay(detail.songs, index, title)} onReorder={!isAlbum && ownsPlaylist ? onReorder : undefined} onSelect={songList.onSelect} onToggleStar={songList.onToggleStar} selectedIds={songList.selectedIds} showAlbum={!isAlbum} showDateAdded={!isAlbum} showQuality={songList.showQuality} songs={detail.songs} /> : <EmptyState title={isAlbum ? "This album is empty" : "Let's find something for your playlist"} body={isAlbum ? "The connected server returned no songs." : "Right-click any song and choose Add to playlist."} />}
      {album?.year && detail.songs.length > 0 && <p className="collection-footnote">{album.year}</p>}
    </CollectionPage>
  );
}

export function RadioView({ compactHeader, data, isLoading, onPlay, onPlayCollection, onTogglePlayback, onToggleShuffle, shuffleArmed, songList, title }: CollectionControls & { data?: RadioResult; isLoading: boolean; onPlay: (songs: SongSummary[], index: number) => void; onPlayCollection: (songs: SongSummary[], label: string) => void; songList: SongListHandlers; title: string }) {
  if (isLoading || !data) return <LoadingState label="Building your radio" />;
  const playing = playsFrom(data.songs, songList.currentId, songList.isPlaying);
  const play = () => playing ? onTogglePlayback() : onPlayCollection(data.songs, title);
  return (
    <CollectionPage
      actions={<><PlayButton label={title} onClick={play} playing={playing} /><ShuffleToggle armed={shuffleArmed} onToggle={onToggleShuffle} /></>}
      art={<span className="collection-header__art radio-cover"><Radio size={70} /></span>}
      color="rgb(30, 96, 60)"
      compact={compactHeader}
      meta={<span>Made for you from your own server • {data.songs.length} songs</span>}
      onPlay={play}
      playing={playing}
      title={title}
      type="Playlist"
    >
      <TrackTable currentId={songList.currentId} isLiked={songList.isLiked} isPlaying={songList.isPlaying} onMenu={songList.onMenu} onPlay={(index) => onPlay(data.songs, index)} onSelect={songList.onSelect} onToggleStar={songList.onToggleStar} selectedIds={songList.selectedIds} showQuality={songList.showQuality} songs={data.songs} />
    </CollectionPage>
  );
}

function formatBytes(bytes: number) {
  if (bytes < 1_000_000) return `${Math.max(0, bytes / 1_000).toFixed(1)} KB`;
  if (bytes < 1_000_000_000) return `${(bytes / 1_000_000).toFixed(1)} MB`;
  return `${(bytes / 1_000_000_000).toFixed(2)} GB`;
}

export function DownloadsView({ downloads, onClear, onPlay, songList }: { downloads: DownloadsController; onClear: () => void; onPlay: (songs: SongSummary[], index: number) => void; songList: SongListHandlers }) {
  const songs = downloads.items.map((item) => item.song);
  if (downloads.loading) return <LoadingState label="Loading offline music" />;
  return <><PageHeading title="Downloads" subtitle="Music stored on this computer for this server profile." /><div className="download-summary"><span><HardDrive size={19} /><strong>{songs.length} {songs.length === 1 ? "track" : "tracks"}</strong><small>{formatBytes(downloads.totalBytes)} used</small></span>{songs.length > 0 && <button onClick={onClear} type="button"><Trash2 size={16} /> Clear downloads</button>}</div>{songs.length > 0 ? <TrackTable currentId={songList.currentId} downloadProgress={downloads.progress} downloadedIds={downloads.downloadedIds} isLiked={songList.isLiked} isPlaying={songList.isPlaying} onMenu={songList.onMenu} onPlay={(index) => onPlay(songs, index)} onSelect={songList.onSelect} onToggleStar={songList.onToggleStar} selectedIds={songList.selectedIds} showDateAdded showQuality={songList.showQuality} songs={songs} /> : <EmptyState title="No downloads yet" body="Use a song or collection menu to make music available offline." />}{downloads.failures.length > 0 && <section className="download-failures"><h2>{downloads.failures.length} {downloads.failures.length === 1 ? "track" : "tracks"} did not finish</h2><p>The rest of the batch continued. Retry the ones that stalled.</p><ul>{downloads.failures.slice(0, 30).map((failure) => <li key={failure.song.id}><span><strong>{failure.song.title}</strong><small>{failure.message}</small></span><button onClick={() => void downloads.download(failure.song).catch(() => undefined)} type="button">Retry</button></li>)}</ul><button className="pill-button" onClick={() => void downloads.retryFailed()} type="button"><RotateCcw size={16} /> Retry all</button></section>}{Object.values(downloads.progress).some((item) => item.status === "downloading" || item.status === "paused") && <section className="download-transfers"><h2>Transfers</h2>{Object.values(downloads.progress).filter((item) => item.status !== "complete").map((item) => <div key={item.id}><Download size={17} /><span><strong>{item.status === "paused" ? "Paused" : item.status === "failed" ? "Needs attention" : "Downloading"}</strong><small>{item.message ?? (item.total ? `${formatBytes(item.received)} of ${formatBytes(item.total)}` : formatBytes(item.received))}</small></span>{item.status === "downloading" && <button onClick={() => void downloads.pause(item.id)} type="button">Pause</button>}</div>)}</section>}</>;
}

export function ProfileView({ connectionStatus, library, onDevices, onHistory, onSettings, onSignOut, onStats, overview }: { connectionStatus: "online" | "offline"; library: ConnectedLibrary; onDevices: () => void; onHistory: () => void; onSettings: () => void; onSignOut: () => void; onStats: () => void; overview: LibraryOverview }) {
  return <><div className="profile-hero"><span>{library.server.username.slice(0, 1).toUpperCase()}</span><div><p className="collection-header__type">Profile</p><h1>{library.server.username}</h1><p><i className={connectionStatus === "online" ? "profile-status profile-status--online" : "profile-status"} />{connectionStatus === "online" ? "Connected" : "Offline"} • {library.server.displayHost}</p></div></div><div className="profile-stats"><span><strong>{overview.albums.length.toLocaleString()}</strong><small>Albums</small></span><span><strong>{overview.artists.length.toLocaleString()}</strong><small>Artists</small></span><span><strong>{overview.playlists.length.toLocaleString()}</strong><small>Playlists</small></span><span><strong>{overview.starredSongs.length.toLocaleString()}</strong><small>Liked songs</small></span></div><div className="profile-actions"><button className="modal-primary" onClick={onStats} type="button">Stats</button><button onClick={onHistory} type="button">Listening History</button><button onClick={onSettings} type="button">Settings</button><button onClick={onDevices} type="button">Splynt Connect</button><button onClick={onSignOut} type="button">Switch account</button></div></>;
}

export function SettingsView({ contextWidth, downloads, library, lyricsIndex, onClearDownloads, onIndexLyrics, onOpenPanel, onReload, onResetLayout, onSleep, settings, sidebarWidth, sleepRemaining, updateSetting, resetSettings }: { contextWidth: number; downloads: DownloadsController; library: ConnectedLibrary; lyricsIndex?: LyricsIndexStatus; onIndexLyrics: () => void; onClearDownloads: () => void; onOpenPanel: (mode: ContextPanelMode) => void; onReload: () => void; onResetLayout: () => void; onSleep: (minutes: number | undefined) => void; settings: DesktopSettings; sidebarWidth: number; sleepRemaining?: number; updateSetting: <K extends keyof DesktopSettings>(key: K, value: DesktopSettings[K]) => void; resetSettings: () => void }) {
  const [cache, setCache] = useState({ files: 0, bytes: 0 });
  const [diagnosticsPath, setDiagnosticsPath] = useState<string>();
  useEffect(() => { void invoke<string | null>("diagnostics_path").then((path) => setDiagnosticsPath(path ?? undefined)).catch(() => undefined); }, []);
  const refreshCache = () => void invoke<{ files: number; bytes: number }>("cache_stats").then((value) => setCache(value && Number.isFinite(value.files) && Number.isFinite(value.bytes) ? value : { files: 0, bytes: 0 })).catch(() => setCache({ files: 0, bytes: 0 }));
  useEffect(refreshCache, []);
  const streamTier = effectiveStreamQuality(settings);
  return <><PageHeading title="Settings" /><div className="settings-grid">
    <section><h2>Account</h2><dl><dt>Server</dt><dd>{library.server.displayHost}</dd><dt>Username</dt><dd>{library.server.username}</dd><dt>Server software</dt><dd>{library.server.serverType ?? "Subsonic compatible"} {library.server.serverVersion ?? ""}</dd><dt>API</dt><dd>{library.server.apiVersion ?? "Unknown"}</dd></dl><button disabled={settings.offlineMode} onClick={onReload} type="button">Refresh library</button></section>

    <section><h2>Playback</h2>
      <SettingToggle checked={settings.gapless} label="Gapless playback" hint="Joins a natural transition with no silence between tracks." onChange={(value) => updateSetting("gapless", value)} />
      <SettingToggle checked={settings.autoplay} label="Autoplay" hint="Keeps going with similar songs when the queue runs out." onChange={(value) => updateSetting("autoplay", value)} />
      <div className="setting-row setting-row--pickers"><span><strong>Shuffle order</strong><small>{settings.shuffleMode === "fewerRepeats" ? "Plays what you have not heard lately first." : "Pure random order."}</small></span><span className="setting-row__controls"><select aria-label="Shuffle order" onChange={(event) => updateSetting("shuffleMode", event.target.value as DesktopSettings["shuffleMode"])} value={settings.shuffleMode}><option value="fewerRepeats">Fewer repeats</option><option value="random">Random</option></select></span></div>
      <label className="setting-row setting-row--slider"><span><strong>Crossfade</strong><small>{settings.crossfadeSeconds ? `${settings.crossfadeSeconds}s on natural transitions only` : "Off"}</small></span><SmoothRange aria-label="Crossfade seconds" max={12} min={0} onChange={(value) => updateSetting("crossfadeSeconds", value)} step={1} value={settings.crossfadeSeconds} /></label>
      <SettingToggle checked={settings.lyricsAutoScroll} label="Lyrics follow playback" hint="Keeps the active line centred while a synced lyric plays." onChange={(value) => updateSetting("lyricsAutoScroll", value)} />
      <div className="setting-row setting-row--pickers"><span><strong>Lyrics source</strong><small>Auto checks your server first, then uses LRCLIB when the server has no lyrics.</small></span><span className="setting-row__controls"><select aria-label="Lyrics source" onChange={(event) => updateSetting("lyricsSource", event.target.value as DesktopSettings["lyricsSource"])} value={settings.lyricsSource}>{lyricsSources.map((source) => <option key={source} value={source}>{source === "auto" ? "Auto" : source === "server" ? "Music server" : "LRCLIB (public)"}</option>)}</select></span></div>
      <div className="setting-row"><span><strong>Lyrics search</strong><small>{lyricsIndex ? `${lyricsIndex.searchable.toLocaleString()} songs searchable by their lyrics${lyricsIndex.running && lyricsIndex.total ? `. Checked ${Math.min(lyricsIndex.scanned, lyricsIndex.total).toLocaleString()} of ${lyricsIndex.total.toLocaleString()} songs on your server.` : lyricsIndex.running ? ". Listing your server's songs." : "."}` : "Search finds songs by a line of their lyrics."}</small></span><span className="setting-row__controls"><button className="pill-button" disabled={lyricsIndex?.running} onClick={onIndexLyrics} type="button">{lyricsIndex?.running ? "Indexing…" : "Rebuild index"}</button></span></div>
      <div className="setting-row setting-row--pickers"><span><strong>Lyrics text size</strong><small>Applies to the lyrics view.</small></span><span className="setting-row__controls"><select aria-label="Lyrics text size" onChange={(event) => updateSetting("lyricsTextSize", event.target.value as DesktopSettings["lyricsTextSize"])} value={settings.lyricsTextSize}>{lyricsTextSizes.map((size) => <option key={size} value={size}>{size[0].toUpperCase() + size.slice(1)}</option>)}</select></span></div>
      <SettingToggle checked={settings.hideExternalPlaylists} label="Hide external playlists" hint="Leaves out playlists that come from a connected provider rather than your server." onChange={(value) => updateSetting("hideExternalPlaylists", value)} />
      <SettingToggle checked={settings.hideExplicitContent} label="Hide explicit content" hint="Filters explicit tracks out of browsing. Downloads are never hidden." onChange={(value) => updateSetting("hideExplicitContent", value)} />
      <SettingToggle checked={settings.keepPlayingInBackground} label="Keep playing when the window closes" hint="Closing hides Splynt to the tray instead of quitting. Quit always stops playback." onChange={(value) => updateSetting("keepPlayingInBackground", value)} />
    </section>

    <EqualizerPanel onChange={(value) => updateSetting("equalizer", value)} settings={settings.equalizer} />

    <section><h2>Audio quality</h2>
      <SettingToggle checked={settings.offlineMode} label="Offline mode" hint="Makes no network requests. Only downloaded music plays." onChange={(value) => updateSetting("offlineMode", value)} />
      <SettingToggle checked={settings.dataSaver} label="Data saver" hint="Overrides streaming quality with Low until you turn it off." onChange={(value) => updateSetting("dataSaver", value)} />
      <QualityPicker disabled={settings.dataSaver} format={settings.streamFormat} label="Streaming" onFormat={(value) => updateSetting("streamFormat", value)} onTier={(value) => updateSetting("streamQuality", value)} tier={streamTier} />
      <QualityPicker format={settings.downloadFormat} label="Downloads" onFormat={(value) => updateSetting("downloadFormat", value)} onTier={(value) => updateSetting("downloadQuality", value)} tier={settings.downloadQuality} />
      <SettingToggle checked={settings.showTrackQuality} label="Show audio quality in track lists" hint="Adds a column with each song's bitrate and format." onChange={(value) => updateSetting("showTrackQuality", value)} />
    </section>

    <section><h2>Home shelves</h2>
      <p className="setting-note">Choose the server and listening-history shelves that appear on Home.</p>
      <ul className="row-order">{settings.homeRowOrder.map((row, index) => {
        const hidden = settings.hiddenHomeRows.includes(row);
        return <li key={row}>
          <label><input checked={!hidden} onChange={() => updateSetting("hiddenHomeRows", hidden ? settings.hiddenHomeRows.filter((item) => item !== row) : [...settings.hiddenHomeRows, row])} type="checkbox" /><span>{homeRowLabels[row]}</span></label>
          <span>
            <button aria-label={`Move ${homeRowLabels[row]} up`} disabled={index === 0} onClick={() => updateSetting("homeRowOrder", swapRows(settings.homeRowOrder, index, index - 1))} type="button">↑</button>
            <button aria-label={`Move ${homeRowLabels[row]} down`} disabled={index === settings.homeRowOrder.length - 1} onClick={() => updateSetting("homeRowOrder", swapRows(settings.homeRowOrder, index, index + 1))} type="button">↓</button>
          </span>
        </li>;
      })}</ul>
      <div className="settings-actions"><button onClick={() => { updateSetting("homeRowOrder", [...homeRows]); updateSetting("hiddenHomeRows", []); }} type="button">Reset shelves</button></div>
    </section>

    <section><h2>Sleep timer</h2>
      <p>{sleepRemaining === undefined ? "No timer running." : `Playback stops in ${Math.ceil(sleepRemaining / 60_000)} min.`}</p>
      <div className="settings-actions">{[15, 30, 45, 60].map((minutes) => <button key={minutes} onClick={() => onSleep(minutes)} type="button">{minutes} min</button>)}{sleepRemaining !== undefined && <button className="modal-danger" onClick={() => onSleep(undefined)} type="button">Cancel</button>}</div>
    </section>

    <section><h2>Desktop layout</h2><dl><dt>Library width</dt><dd>{Math.round(sidebarWidth)} px</dd><dt>Context width</dt><dd>{Math.round(contextWidth)} px</dd><dt>Queue recovery</dt><dd>On</dd><dt>Media keys</dt><dd>{mediaKeySupport()}</dd></dl><div className="settings-actions"><button onClick={() => onOpenPanel("nowPlaying")} type="button">Open Now Playing</button><button onClick={onResetLayout} type="button">Reset layout</button><button onClick={resetSettings} type="button">Reset preferences</button></div></section>

    <section><h2>Offline storage</h2><dl><dt>Downloaded tracks</dt><dd>{downloads.items.length.toLocaleString()}</dd><dt>Audio usage</dt><dd>{formatBytes(downloads.totalBytes)}</dd><dt>Artwork cache</dt><dd>{cache.files.toLocaleString()} files · {formatBytes(cache.bytes)}</dd><dt>Profile scope</dt><dd>{library.server.username}@{library.server.displayHost}</dd><dt>Offline browsing</dt><dd>{offlineCacheDegraded() ? "Limited, this library is too large for the local store" : "Saved"}</dd></dl><p>Partial downloads are retained so interrupted transfers can resume. Downloaded audio always plays before the network copy.</p><div className="settings-actions">{downloads.items.length > 0 && <button onClick={onClearDownloads} type="button">Clear downloads</button>}{cache.files > 0 && <button onClick={() => void invoke("clear_artwork_cache").then(() => { setCache({ files: 0, bytes: 0 }); })} type="button">Clear artwork cache</button>}</div></section>

    <section><h2>Diagnostics</h2>
      <p>Splynt records what it is doing to a log file: launches, failures, anything the interface reports, and any crash of the previous session. Nothing leaves this computer unless you send it.</p>
      <dl><dt>Log file</dt><dd className="settings-path">{diagnosticsPath ?? "Not created yet"}</dd></dl>
      <div className="settings-actions"><button disabled={!diagnosticsPath} onClick={() => void invoke("reveal_diagnostics").catch(() => undefined)} type="button">Show log</button></div>
    </section>

    <section><h2>Splynt Connect</h2><p>Players signed in to this server account can hand off playback, act as remotes, or join a synchronized group.</p><button onClick={() => onOpenPanel("connect")} type="button">Open devices</button></section>

    <section><h2>Keyboard</h2><dl>{shortcutRows.map(([action, keys]) => <div className="settings-shortcut" key={action}><dt>{action}</dt><dd>{keys}</dd></div>)}</dl></section>
  </div>
  {/* Outside the grid on purpose: as another <section> it would read as one
      more settings card competing with Playback and Offline storage. Sitting
      under the grid it closes the page instead of interrupting it. */}
  <footer className="settings-support">
    {/* No supporting copy. Anything framing this as "Splynt is free" would be
        a pricing claim, and the iOS app is not committed to being free once it
        reaches the App Store. The link stands on its own. */}
    <button onClick={() => void invoke("open_support_page").catch(() => undefined)} type="button">
      <Coffee size={15} /> Buy me a coffee
    </button>
  </footer></>;
}

export const shortcutRows: Array<[string, string]> = [
  ["Play or pause", "Space"],
  ["Search", "Ctrl K"],
  ["Back / Forward", "Alt ← / Alt →"],
  ["Previous / Next track", "Ctrl ← / Ctrl →"],
  ["Seek backward / forward", "Shift ← / Shift →"],
  ["Volume up / down", "Ctrl ↑ / Ctrl ↓"],
  ["Shuffle", "Ctrl S"],
  ["Repeat", "Ctrl R"],
  ["Save current song to Liked Songs", "Alt Shift B"],
  ["Select all songs", "Ctrl A"],
  ["Remove selected songs from playlist", "Delete"],
  ["Queue", "Alt Shift Q"],
  ["Lyrics", "Alt Shift J"],
  ["Now playing view", "Alt Shift R"],
  ["Home", "Alt Shift H"],
  ["Your Library", "Alt Shift 0"],
  ["Create playlist", "Alt Shift P"],
  ["Preferences", "Ctrl ,"],
  ["Full screen", "F11"],
  ["Keyboard shortcuts", "Ctrl /"],
];

/// WebKitGTK does not implement Media Session, so the Linux build cannot claim
/// media-key support the way macOS and Windows can.
function mediaKeySupport() {
  if (!("mediaSession" in navigator)) return "Unavailable in this webview";
  return document.documentElement.dataset.platform === "linux" ? "Limited on Linux (no MPRIS yet)" : "On";
}

/// Full-bleed cover, opened from a hero or the full player. Escape and a click
/// anywhere dismiss it.
export function ArtworkLightbox({ alt, coverArt, onClose }: { alt: string; coverArt?: string; onClose: () => void }) {
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => { if (event.key === "Escape") { event.preventDefault(); onClose(); } };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [onClose]);
  return (
    <button aria-label={`Close ${alt}`} className="artwork-lightbox" onClick={onClose} type="button">
      <MediaArtwork alt={alt} className="artwork-lightbox__art" coverArt={coverArt} />
    </button>
  );
}

function swapRows(order: HomeRow[], from: number, to: number) {
  const next = [...order];
  [next[from], next[to]] = [next[to], next[from]];
  return next;
}

function SettingToggle({ checked, hint, label, onChange }: { checked: boolean; hint: string; label: string; onChange: (value: boolean) => void }) {
  return <label className="setting-row"><span><strong>{label}</strong><small>{hint}</small></span><input checked={checked} onChange={(event) => onChange(event.target.checked)} type="checkbox" /></label>;
}

function QualityPicker({ disabled, format, label, onFormat, onTier, tier }: { disabled?: boolean; format: AudioFormat; label: string; onFormat: (value: AudioFormat) => void; onTier: (value: QualityTier) => void; tier: QualityTier }) {
  return (
    <div className="setting-row setting-row--pickers">
      <span><strong>{label}</strong><small>{tierDetail(tier)}{transcodes(tier) ? ` · ${format.toUpperCase()}` : " · no transcode"}</small></span>
      <span className="setting-row__controls">
        <select aria-label={`${label} quality`} disabled={disabled} onChange={(event) => onTier(Number(event.target.value) as QualityTier)} value={tier}>
          {qualityTiers.map((value) => <option key={value} value={value}>{tierLabel(value)}</option>)}
        </select>
        <select aria-label={`${label} format`} disabled={disabled || !transcodes(tier)} onChange={(event) => onFormat(event.target.value as AudioFormat)} value={format}>
          {audioFormats.map((value) => <option key={value} value={value}>{value.toUpperCase()}</option>)}
        </select>
      </span>
    </div>
  );
}

/// Arm-only, matching the iOS engine: this flips the one shared shuffle flag
/// and starts nothing. Play consumes the flag.
export function ShuffleToggle({ armed, onToggle }: { armed: boolean; onToggle: () => void }) {
  return <button aria-label={armed ? "Disable shuffle" : "Enable shuffle"} aria-pressed={armed} className={armed ? "collection-icon-action collection-icon-action--active" : "collection-icon-action"} onClick={onToggle} title={armed ? "Disable shuffle" : "Enable shuffle"} type="button"><Shuffle size={26} /></button>;
}

export function LoadingState({ label }: { label: string }) { return <div aria-busy="true" className="loading-state" role="status"><div className="loading-state__signal"><LoaderCircle className="loading-spinner" size={24} /><p>{label}</p></div><div aria-hidden="true" className="loading-skeleton"><i /><i /><i /></div></div>; }
export function RetryState({ message, onRetry }: { message: string; onRetry: () => void }) { return <div className="retry-state" role="status"><p>{message}</p><button onClick={onRetry} type="button"><RotateCcw size={14} /> Retry</button></div>; }
export function EmptyState({ body, title }: { body: string; title: string }) { return <section className="empty-library"><Disc3 size={46} /><h2>{title}</h2><p>{body}</p></section>; }
