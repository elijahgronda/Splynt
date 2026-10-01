import { CheckCircle2, Clock3, Cloud, MoreHorizontal, Pause, Play } from "lucide-react";
import { useRef, useState } from "react";
import type { DragEvent as ReactDragEvent, MouseEvent as ReactMouseEvent } from "react";
import type { AlbumSummary, ArtistSummary, DownloadProgress, PlaylistSummary, SongSummary } from "../types";
import { parseExternalSource } from "../lib/externalSource";
import { MediaArtwork } from "./MediaArtwork";

export function formatDuration(value?: number) {
  if (!value || !Number.isFinite(value)) return "—";
  const total = Math.max(0, Math.round(value));
  const minutes = Math.floor(total / 60);
  const seconds = String(total % 60).padStart(2, "0");
  return `${minutes}:${seconds}`;
}

/// Collection totals read the way Spotify writes them: "45 min 39 sec", or
/// "2 hr 5 min" once a collection passes an hour.
export function formatLongDuration(value?: number) {
  if (!value || !Number.isFinite(value)) return undefined;
  const total = Math.max(0, Math.round(value));
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const seconds = total % 60;
  if (hours) return `${hours} hr ${minutes} min`;
  return seconds ? `${minutes} min ${seconds} sec` : `${minutes} min`;
}

function formatDate(value: string) {
  const date = new Date(value);
  return Number.isFinite(date.getTime()) ? new Intl.DateTimeFormat(undefined, { dateStyle: "medium" }).format(date) : undefined;
}

/// Recent additions are relative ("3 days ago"); anything older than a month
/// is a date, matching Spotify's Date added column.
export function formatAdded(value?: string, now = Date.now()) {
  if (!value) return undefined;
  const time = new Date(value).getTime();
  if (!Number.isFinite(time)) return undefined;
  const days = Math.floor((now - time) / 86_400_000);
  if (days < 0) return formatDate(value);
  if (days === 0) return "Today";
  if (days === 1) return "Yesterday";
  if (days < 7) return `${days} days ago`;
  if (days < 31) {
    const weeks = Math.floor(days / 7);
    return `${weeks} ${weeks === 1 ? "week" : "weeks"} ago`;
  }
  return formatDate(value);
}

/// Spotify's save control: an outlined plus that becomes a filled green check.
export function LikeGlyph({ liked, size = 16 }: { liked: boolean; size?: number }) {
  return liked ? (
    <svg aria-hidden="true" height={size} viewBox="0 0 16 16" width={size}>
      <circle cx="8" cy="8" fill="currentColor" r="7.25" />
      <path d="M4.9 8.3 7 10.4l4.2-4.6" fill="none" stroke="#000" strokeLinecap="round" strokeLinejoin="round" strokeWidth="1.6" />
    </svg>
  ) : (
    <svg aria-hidden="true" height={size} viewBox="0 0 16 16" width={size}>
      <circle cx="8" cy="8" fill="none" r="6.9" stroke="currentColor" strokeWidth="1.3" />
      <path d="M8 5.1v5.8M5.1 8h5.8" stroke="currentColor" strokeLinecap="round" strokeWidth="1.3" />
    </svg>
  );
}

type AlbumCardProps = {
  album: AlbumSummary;
  onMenu?: (event: ReactMouseEvent) => void;
  onOpen: (album: AlbumSummary) => void;
  onPlay: (album: AlbumSummary) => void;
};

export function AlbumCard({ album, onMenu, onOpen, onPlay }: AlbumCardProps) {
  return (
    <article className="catalog-card" onContextMenu={onMenu ? (event) => { event.preventDefault(); onMenu(event); } : undefined}>
      <button className="catalog-card__body" data-search-result onClick={() => onOpen(album)} type="button">
        <MediaArtwork alt={`${album.title} cover`} coverArt={album.coverArt} />
        <strong title={album.title}>{album.title}</strong>
        <span title={album.artist}>{[album.year, album.artist].filter(Boolean).join(" • ")}</span>
      </button>
      <button className="catalog-card__play" aria-label={`Play ${album.title}`} onClick={() => onPlay(album)} type="button">
        <Play fill="currentColor" size={21} />
      </button>
    </article>
  );
}

type ArtistCardProps = { artist: ArtistSummary; onMenu?: (event: ReactMouseEvent) => void; onOpen: (artist: ArtistSummary) => void };

export function ArtistCard({ artist, onMenu, onOpen }: ArtistCardProps) {
  return (
    <article className="catalog-card catalog-card--artist" onContextMenu={onMenu ? (event) => { event.preventDefault(); onMenu(event); } : undefined}>
      <button className="catalog-card__body" data-search-result onClick={() => onOpen(artist)} type="button">
        <MediaArtwork alt={`${artist.name} portrait`} coverArt={artist.coverArt} fallback="artist" shape="circle" />
        <strong title={artist.name}>{artist.name}</strong>
        <span>Artist</span>
      </button>
    </article>
  );
}

type PlaylistCardProps = {
  playlist: PlaylistSummary;
  onMenu?: (event: ReactMouseEvent) => void;
  onOpen: (playlist: PlaylistSummary) => void;
  onPlay: (playlist: PlaylistSummary) => void;
};

export function PlaylistCard({ playlist, onMenu, onOpen, onPlay }: PlaylistCardProps) {
  return (
    <article className="catalog-card" onContextMenu={onMenu ? (event) => { event.preventDefault(); onMenu(event); } : undefined}>
      <button className="catalog-card__body" data-search-result onClick={() => onOpen(playlist)} type="button">
        <MediaArtwork alt={`${playlist.name} cover`} coverArt={playlist.coverArt} fallback="playlist" />
        <strong title={playlist.name}>{playlist.name}</strong>
        <span>{playlist.owner ? `By ${playlist.owner}` : "Playlist"}</span>
      </button>
      <button className="catalog-card__play" aria-label={`Play ${playlist.name}`} onClick={() => onPlay(playlist)} type="button">
        <Play fill="currentColor" size={21} />
      </button>
    </article>
  );
}

type TrackTableProps = {
  songs: SongSummary[];
  currentId?: string;
  isPlaying?: boolean;
  onPlay: (index: number) => void;
  onOpenAlbum?: (id: string) => void;
  onOpenArtist?: (id: string) => void;
  onToggleStar?: (song: SongSummary) => void;
  onMenu?: (song: SongSummary, index: number, event: ReactMouseEvent) => void;
  onSelect?: (song: SongSummary, index: number, event: ReactMouseEvent) => void;
  selectedIds?: Set<string>;
  downloadedIds?: Set<string>;
  downloadProgress?: Record<string, DownloadProgress>;
  isLiked?: (song: SongSummary) => boolean;
  showDateAdded?: boolean;
  showAlbum?: boolean;
  showArtwork?: boolean;
  showQuality?: boolean;
  onReorder?: (from: number, to: number) => void;
};

/// The payload every track drag carries. A drop target reads it to learn which
/// songs were dragged, whether that is one row or a whole selection.
export const SONG_DRAG_TYPE = "application/x-splice-songs";

export function readSongDrag(event: ReactDragEvent): string[] {
  try {
    const raw = event.dataTransfer.getData(SONG_DRAG_TYPE);
    const parsed: unknown = raw ? JSON.parse(raw) : [];
    return Array.isArray(parsed) ? parsed.filter((id): id is string => typeof id === "string") : [];
  } catch {
    return [];
  }
}

export function TrackTable({
  songs,
  currentId,
  isPlaying,
  onPlay,
  onOpenAlbum,
  onOpenArtist,
  onToggleStar,
  onMenu,
  onSelect,
  selectedIds,
  downloadedIds,
  downloadProgress,
  isLiked,
  showDateAdded,
  showAlbum = true,
  showArtwork = showAlbum,
  showQuality,
  onReorder,
}: TrackTableProps) {
  const activeIndex = currentId ? songs.findIndex((song) => song.id === currentId) : -1;
  const dragFrom = useRef<number | undefined>(undefined);
  const [dropTarget, setDropTarget] = useState<number>();
  const columns = ["track-table", showAlbum ? "track-table--album" : "", showDateAdded ? "track-table--added" : "", showQuality ? "track-table--quality" : ""].filter(Boolean).join(" ");
  return (
    <div aria-label="Songs" aria-rowcount={songs.length + 1} className={columns} role="grid">
      <div className="track-row track-row--header" role="row">
        <span role="columnheader">#</span>
        <span role="columnheader">Title</span>
        {showAlbum && <span role="columnheader">Album</span>}
        {showDateAdded && <span role="columnheader">Date added</span>}
        {showQuality && <span role="columnheader">Quality</span>}
        <span aria-label="Duration" className="track-row__duration-header" role="columnheader"><Clock3 size={15} /></span>
      </div>
      {songs.map((song, index) => {
        const active = index === activeIndex;
        const selected = selectedIds?.has(`${song.id}-${index}`) ?? false;
        const external = Boolean(parseExternalSource(song.id));
        const offline = downloadedIds?.has(song.id) ?? false;
        const transfer = downloadProgress?.[song.id];
        const liked = isLiked ? isLiked(song) : Boolean(song.starred);
        const quality = [song.bitRate ? `${song.bitRate} kbps` : undefined, song.suffix?.toUpperCase()].filter(Boolean).join(" · ");
        return (
          <div
            aria-selected={selected}
            className={`${active ? "track-row track-row--active" : "track-row"}${selected ? " track-row--selected" : ""}${dropTarget === index ? " track-row--drop" : ""}`}
            key={`${song.id}-${index}`}
            onClick={(event) => onSelect?.(song, index, event)}
            onContextMenu={(event) => { event.preventDefault(); onMenu?.(song, index, event); }}
            onDoubleClick={() => onPlay(index)}
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                event.preventDefault();
                onPlay(index);
                return;
              }
              if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
              const rows = Array.from(event.currentTarget.parentElement?.querySelectorAll<HTMLElement>("[role='row'][tabindex='0']") ?? []);
              const current = rows.indexOf(event.currentTarget);
              const next = event.key === "ArrowDown" ? Math.min(rows.length - 1, current + 1) : Math.max(0, current - 1);
              event.preventDefault();
              rows[next]?.focus();
            }}
            role="row"
            aria-rowindex={index + 2}
            data-search-result
            data-track-row
            draggable
            onDragEnd={() => { dragFrom.current = undefined; setDropTarget(undefined); }}
            onDragLeave={() => setDropTarget((value) => value === index ? undefined : value)}
            onDragOver={onReorder ? (event) => { if (dragFrom.current === undefined) return; event.preventDefault(); event.dataTransfer.dropEffect = "move"; setDropTarget(index); } : undefined}
            onDragStart={(event) => {
              // Dragging a row that is part of a selection drags the selection.
              const ids = selectedIds?.has(`${song.id}-${index}`)
                ? songs.filter((item, itemIndex) => selectedIds.has(`${item.id}-${itemIndex}`)).map((item) => item.id)
                : [song.id];
              event.dataTransfer.setData(SONG_DRAG_TYPE, JSON.stringify(ids));
              event.dataTransfer.setData("text/plain", song.title);
              event.dataTransfer.effectAllowed = "copyMove";
              dragFrom.current = ids.length === 1 ? index : undefined;
            }}
            onDrop={onReorder ? (event) => {
              const from = dragFrom.current;
              dragFrom.current = undefined;
              setDropTarget(undefined);
              if (from === undefined || from === index) return;
              event.preventDefault();
              onReorder(from, index);
            } : undefined}
            tabIndex={0}
          >
            <span className="track-row__index-cell" role="gridcell"><button className="track-row__index" aria-label={active && isPlaying ? `Pause ${song.title}` : `Play ${song.title}`} onClick={(event) => { event.stopPropagation(); onPlay(index); }} type="button">
              {active && isPlaying
                ? <><span className="playing-bars" aria-hidden="true"><i /><i /><i /></span><Pause className="track-row__hover-play" fill="currentColor" size={14} /></>
                : <><span>{index + 1}</span><Play className="track-row__hover-play" fill="currentColor" size={14} /></>}
            </button></span>
            <span className="track-row__title" role="gridcell">
              {showArtwork && <MediaArtwork alt="" className="track-row__art" coverArt={song.coverArt} />}
              <span className="track-row__copy">
                <strong>{song.title}</strong>
                <span className="track-row__subtitle">
                  {song.explicitStatus && song.explicitStatus !== "clean" && <span aria-label="Explicit" className="explicit-badge">E</span>}
                  {offline && <CheckCircle2 aria-label="Available offline" className="downloaded-source" size={13} />}
                  {external && <Cloud aria-label="External source" className="external-source" size={12} />}
                  {song.artistId && onOpenArtist ? (
                    <button onClick={(event) => { event.stopPropagation(); onOpenArtist(song.artistId!); }} type="button">{song.artist}</button>
                  ) : <small>{song.artist}</small>}
                </span>
              </span>
            </span>
            {showAlbum && <span className="track-row__album" role="gridcell">
              {song.albumId && onOpenAlbum ? <button onClick={(event) => { event.stopPropagation(); onOpenAlbum(song.albumId!); }} type="button">{song.album}</button> : song.album}
            </span>}
            {showDateAdded && <span className="track-row__added" role="gridcell">{formatAdded(song.created) ?? ""}</span>}
            {showQuality && <span className="track-row__quality" role="gridcell">{quality}</span>}
            <span className="track-row__duration" role="gridcell">
              {transfer?.status === "downloading" && <small className="download-progress-label">{transfer.total ? `${Math.min(100, Math.round(transfer.received / transfer.total * 100))}%` : "Downloading"}</small>}
              {onToggleStar ? (
                <button className={liked ? "track-like track-like--active" : "track-like"} aria-label={liked ? "Remove from Liked Songs" : "Save to Liked Songs"} onClick={(event) => { event.stopPropagation(); onToggleStar(song); }} title={liked ? "Remove from Liked Songs" : "Save to Liked Songs"} type="button">
                  <LikeGlyph liked={liked} />
                </button>
              ) : <span className="track-like-slot" />}
              <span className="track-row__time">{formatDuration(song.duration)}</span>
              {onMenu ? <button aria-label={`More options for ${song.title}`} className="track-menu" onClick={(event) => { event.stopPropagation(); onMenu(song, index, event); }} title={`More options for ${song.title}`} type="button"><MoreHorizontal size={17} /></button> : <span className="track-like-slot" />}
            </span>
          </div>
        );
      })}
    </div>
  );
}
