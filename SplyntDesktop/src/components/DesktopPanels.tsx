import {
  ChevronRight, Disc3, Download, GripVertical, Heart, Laptop, ListEnd, ListMusic, Maximize, MicVocal,
  Minimize, Minimize2, MonitorSpeaker, MoreHorizontal, Pause, Play, Plus, Radio, Search, SkipBack, SkipForward,
  Smartphone, Trash2, Tv, UserRound, WifiOff, X,
} from "lucide-react";
import type { KeyboardEvent as ReactKeyboardEvent, MouseEvent as ReactMouseEvent, PointerEvent as ReactPointerEvent } from "react";
import type { CSSProperties } from "react";
import { Fragment, useEffect, useLayoutEffect, useRef, useState } from "react";
import { useAnchoredMenu } from "../hooks/useAnchoredMenu";
import { useArtworkColor } from "../hooks/useArtworkColor";
import { useMenuFocus } from "../hooks/useMenuFocus";
import type { PlaybackController } from "../hooks/usePlayback";
import { deviceName, hasRemoteControl } from "../lib/connectFollow";
import { externalProviderLabel } from "../lib/externalSource";
import type {
  ArtistSummary, AutoplayStatus, ConnectCommand, ConnectPeer, ConnectSnapshot, ContextPanelMode, LyricsResult, PlaylistSummary, SongSummary,
} from "../types";
import { LikeGlyph } from "./Catalog";
import { MediaArtwork } from "./MediaArtwork";
import { PlaybackProgress } from "./RangeSlider";
import { TrackSlide } from "./TrackSlide";

type ContextPanelProps = {
  mode: ContextPanelMode;
  onClose: () => void;
  playback: PlaybackController;
  artist?: ArtistSummary;
  artistFollowed: boolean;
  liked: boolean;
  recentlyPlayed: SongSummary[];
  onOpenAlbum: (id: string) => void;
  onOpenArtist: (id: string) => void;
  onToggleFollow: () => void;
  onToggleLike: () => void;
  onOpenQueue: () => void;
  connect: ConnectSnapshot;
  groupId?: string;
  onMoveHere: (peer: ConnectPeer) => void;
  onMoveToDevice: (peer: ConnectPeer) => void;
  /// The peer this computer is currently driving, if any. The panel could not
  /// see it, so it could not offer the one action a listener in that state
  /// wants: bring the audio back here.
  remoteDeviceId?: string;
  /// Moves playback from the followed device to this computer.
  onPlayHere: () => void;
  onResize: (event: ReactPointerEvent) => void;
  onResizeKey: (event: ReactKeyboardEvent<HTMLButtonElement>) => void;
  onSend: (peerId: string, command: ConnectCommand) => void;
  onStartGroup: () => void;
  onStopGroup: () => void;
  onSaveQueue: () => void;
  autoplay?: AutoplayStatus;
  width: number;
};

export function DesktopContextPanel(props: ContextPanelProps) {
  const title = props.mode === "nowPlaying" ? "Now Playing" : props.mode === "queue" ? "Queue" : "Devices";
  const heading = props.mode === "nowPlaying" && props.playback.current ? props.playback.contextLabel : props.mode === "connect" ? "Connect to a device" : title;
  return (
    <aside className="context-panel" aria-label={title} key={props.mode}>
      <button aria-label="Resize context panel" aria-orientation="vertical" aria-valuemax={460} aria-valuemin={280} aria-valuenow={Math.round(props.width)} className="context-panel__resizer" onKeyDown={props.onResizeKey} onPointerDown={props.onResize} role="separator" type="button" />
      <header>
        <h2>{heading}</h2>
        <button aria-label={`Close ${title}`} onClick={props.onClose} title="Close" type="button"><X size={18} /></button>
      </header>
      <div className="context-panel__body">
      {props.mode === "nowPlaying" && (
        <NowPlayingPanel
          artist={props.artist}
          artistFollowed={props.artistFollowed}
          liked={props.liked}
          onOpenAlbum={props.onOpenAlbum}
          onOpenArtist={props.onOpenArtist}
          onOpenQueue={props.onOpenQueue}
          onToggleFollow={props.onToggleFollow}
          onToggleLike={props.onToggleLike}
          playback={props.playback}
        />
      )}
      {props.mode === "queue" && <QueuePanel autoplay={props.autoplay} onSaveQueue={props.onSaveQueue} playback={props.playback} recentlyPlayed={props.recentlyPlayed} />}
      {props.mode === "connect" && (
        <ConnectPanel
          canHandoff={Boolean(props.playback.current)}
          groupId={props.groupId}
          onMoveHere={props.onMoveHere}
          onMoveToDevice={props.onMoveToDevice}
          onSend={props.onSend}
          onStartGroup={props.onStartGroup}
          onStopGroup={props.onStopGroup}
          localPlaying={props.playback.rendering}
          onPlayHere={props.onPlayHere}
          remoteDeviceId={props.remoteDeviceId}
          snapshot={props.connect}
        />
      )}
      </div>
    </aside>
  );
}

function NowPlayingPanel({ artist, artistFollowed, liked, onOpenAlbum, onOpenArtist, onOpenQueue, onToggleFollow, onToggleLike, playback }: {
  artist?: ArtistSummary;
  artistFollowed: boolean;
  liked: boolean;
  onOpenAlbum: (id: string) => void;
  onOpenArtist: (id: string) => void;
  onOpenQueue: () => void;
  onToggleFollow: () => void;
  onToggleLike: () => void;
  playback: PlaybackController;
}) {
  if (!playback.current) return <PanelEmpty icon={ListMusic} text="Choose something to play." />;
  const current = playback.current;
  const source = externalProviderLabel(current.id);
  const quality = [current.bitRate ? `${current.bitRate} kbps` : undefined, current.suffix?.toUpperCase()].filter(Boolean).join(" · ") || "Original quality";
  const next = playback.queue[playback.index + 1];
  return (
    <div className="now-panel">
      <MediaArtwork alt={`${current.title} cover`} className="now-panel__art" coverArt={current.coverArt} />
      <div className="now-panel__copy">
        <span>
          {current.albumId ? <button className="now-panel__title" onClick={() => onOpenAlbum(current.albumId!)} type="button">{current.title}</button> : <h3 className="now-panel__title">{current.title}</h3>}
          {current.artistId ? <button className="now-panel__artist" onClick={() => onOpenArtist(current.artistId!)} type="button">{current.artist}</button> : <p className="now-panel__artist">{current.artist}</p>}
        </span>
        <button aria-label={liked ? "Remove from Liked Songs" : "Save to Liked Songs"} className={liked ? "now-panel__like now-panel__like--active" : "now-panel__like"} onClick={onToggleLike} title={liked ? "Remove from Liked Songs" : "Save to Liked Songs"} type="button"><LikeGlyph liked={liked} size={20} /></button>
      </div>
      <article className="now-panel__card now-panel__card--artist">
        <MediaArtwork alt="" className="now-panel__artist-art" coverArt={artist?.coverArt ?? current.coverArt} fallback="artist" />
        <p className="now-panel__card-label">About the artist</p>
        <div className="now-panel__artist-row">
          <span>
            {current.artistId ? <button onClick={() => onOpenArtist(current.artistId!)} type="button"><strong>{current.artist}</strong></button> : <strong>{current.artist}</strong>}
            {artist?.albumCount ? <small>{artist.albumCount} {artist.albumCount === 1 ? "album" : "albums"} in your library</small> : null}
          </span>
          {current.artistId && <button className={artistFollowed ? "pill-button pill-button--active" : "pill-button"} onClick={onToggleFollow} type="button">{artistFollowed ? "Following" : "Follow"}</button>}
        </div>
      </article>
      <article className="now-panel__card">
        <div className="now-panel__card-heading"><p className="now-panel__card-label">Credits</p></div>
        <dl className="now-panel__credits">
          <dt>{current.artist}</dt><dd>Main artist</dd>
          <dt>{quality}{source ? ` · ${source}` : ""}</dt><dd>Audio</dd>
        </dl>
      </article>
      {next && (
        <article className="now-panel__card">
          <div className="now-panel__card-heading"><p className="now-panel__card-label">Next in queue</p><button onClick={onOpenQueue} type="button">Open queue</button></div>
          <button className="now-panel__next" onClick={() => playback.skipTo(playback.index + 1)} type="button">
            <MediaArtwork alt="" className="queue-row__art" coverArt={next.coverArt} />
            <span><strong>{next.title}</strong><small>{next.artist}</small></span>
          </button>
        </article>
      )}
    </div>
  );
}

function QueuePanel({ autoplay, onSaveQueue, playback, recentlyPlayed }: { autoplay?: AutoplayStatus; onSaveQueue: () => void; playback: PlaybackController; recentlyPlayed: SongSummary[] }) {
  const [tab, setTab] = useState<"queue" | "recent">("queue");
  return (
    <>
      <div aria-label="Queue view" className="panel-tabs" role="tablist">
        <button aria-selected={tab === "queue"} className={tab === "queue" ? "panel-tab panel-tab--active" : "panel-tab"} onClick={() => setTab("queue")} role="tab" type="button">Queue</button>
        <button aria-selected={tab === "recent"} className={tab === "recent" ? "panel-tab panel-tab--active" : "panel-tab"} onClick={() => setTab("recent")} role="tab" type="button">Recently played</button>
      </div>
      {tab === "queue" ? <QueueList autoplay={autoplay} onSaveQueue={onSaveQueue} playback={playback} /> : <RecentlyPlayedList playback={playback} songs={recentlyPlayed} />}
    </>
  );
}

function RecentlyPlayedList({ playback, songs }: { playback: PlaybackController; songs: SongSummary[] }) {
  if (!songs.length) return <PanelEmpty icon={ListMusic} text="Songs you play will show up here." />;
  return (
    <div className="queue-list">
      {songs.map((song, index) => (
        <div className="queue-row" key={`${song.id}-${index}`}>
          <button className="queue-row__main" onClick={() => playback.playQueue(songs, index, true, 0, "Recently played")} type="button">
            <MediaArtwork alt="" className="queue-row__art" coverArt={song.coverArt} />
            <span><strong>{song.title}</strong><small>{song.artist}</small></span>
          </button>
        </div>
      ))}
    </div>
  );
}

type QueueDrag = {
  from: number;
  to: number;
  pointerId: number;
  element: HTMLElement;
  startY: number;
  pointerY: number;
  startScroll: number;
  rows: { index: number; mid: number; element: HTMLElement; shift: string }[];
  scroller: HTMLElement | null;
  active: boolean;
  frame: number;
};

function scrollParent(element: HTMLElement | null) {
  for (let node = element?.parentElement; node; node = node.parentElement) {
    const { overflowY } = getComputedStyle(node);
    if ((overflowY === "auto" || overflowY === "scroll") && node.scrollHeight > node.clientHeight) return node;
  }
  return null;
}

/// Rows are keyed by song and occurrence rather than by position, so a
/// reorder moves the existing row elements instead of remounting them.
function queueKeys(songs: SongSummary[]) {
  const seen = new Map<string, number>();
  return songs.map((song) => {
    const count = seen.get(song.id) ?? 0;
    seen.set(song.id, count + 1);
    return `${song.id}#${count}`;
  });
}

function QueueList({ autoplay, onSaveQueue, playback }: { autoplay?: AutoplayStatus; onSaveQueue: () => void; playback: PlaybackController }) {
  const listRef = useRef<HTMLDivElement>(null);
  const drag = useRef<QueueDrag | undefined>(undefined);
  const settling = useRef(false);
  const suppressClick = useRef(false);

  const clearOffsets = () => {
    const list = listRef.current;
    if (!list) return;
    list.classList.remove("queue-list--dragging");
    for (const row of list.querySelectorAll<HTMLElement>("[data-queue-row]")) {
      row.classList.remove("queue-row--lifted");
      row.style.transition = "none";
      row.style.transform = "";
    }
    void list.offsetHeight;
    for (const row of list.querySelectorAll<HTMLElement>("[data-queue-row]")) row.style.transition = "";
  };

  // A drop leaves every row where the drag put it until React has rendered
  // the new order, then clears the offsets before paint. Clearing them any
  // earlier shows the old order for a frame.
  useLayoutEffect(() => {
    if (!settling.current) return;
    settling.current = false;
    clearOffsets();
  }, [playback.queue]);

  useEffect(() => () => {
    if (drag.current) window.cancelAnimationFrame(drag.current.frame);
  }, []);

  if (!playback.queue.length) return <PanelEmpty icon={ListMusic} text="Your queue is empty." />;
  // Another device's queue can be played from and added to, but no wire
  // command removes or reorders its rows, so those tools are not offered.
  const remote = playback.remote;
  const manualStart = playback.index + 1;
  const contextStart = manualStart + playback.manualQueueCount;
  // Up Next is one list, as on iOS: any upcoming row can go anywhere in it,
  // and where it lands decides whether it counts as added by hand.
  const canReorder = (index: number) => !remote && index >= manualStart && index < playback.queue.length;
  const keys = queueKeys(playback.queue);

  const layout = () => {
    const state = drag.current;
    if (!state?.active) return;
    const self = state.rows.find((row) => row.index === state.from);
    if (!self) return;
    const offset = state.pointerY - state.startY + (state.scroller?.scrollTop ?? 0) - state.startScroll;
    const first = state.rows[0].mid;
    const last = state.rows[state.rows.length - 1].mid;
    const centre = Math.min(Math.max(self.mid + offset, first), last);
    let to = state.from;
    for (const row of state.rows) {
      if (row.index > state.from && centre > row.mid) to = Math.max(to, row.index);
      if (row.index < state.from && centre < row.mid) to = Math.min(to, row.index);
    }
    state.to = to;
    // A displaced row takes its neighbour's place, which also carries it
    // across a section label without overlapping it.
    state.rows.forEach((row, position) => {
      const shift = row.index === state.from ? `translateY(${centre - self.mid}px)`
        : row.index > state.from && row.index <= to ? `translateY(${state.rows[position - 1].mid - row.mid}px)`
        : row.index < state.from && row.index >= to ? `translateY(${state.rows[position + 1].mid - row.mid}px)`
        : "";
      if (shift === row.shift) return;
      row.shift = shift;
      row.element.style.transform = shift;
    });
  };

  // Holding a row near the top or bottom edge scrolls the panel, faster the
  // closer the pointer gets.
  const autoScroll = () => {
    const state = drag.current;
    if (!state?.active) return;
    if (state.scroller) {
      const bounds = state.scroller.getBoundingClientRect();
      const edge = 48;
      const speed = state.pointerY < bounds.top + edge ? -(bounds.top + edge - state.pointerY) / 4
        : state.pointerY > bounds.bottom - edge ? (state.pointerY - bounds.bottom + edge) / 4 : 0;
      if (speed) {
        state.scroller.scrollTop += speed;
        layout();
      }
    }
    state.frame = window.requestAnimationFrame(autoScroll);
  };

  const beginDrag = (event: ReactPointerEvent<HTMLDivElement>, index: number) => {
    suppressClick.current = false;
    if (event.button !== 0 || !canReorder(index) || drag.current) return;
    if ((event.target as Element).closest("button:not(.queue-row__grip):not(.queue-row__main)")) return;
    const list = listRef.current;
    if (!list) return;
    const scroller = scrollParent(list);
    const scroll = scroller?.scrollTop ?? 0;
    const rows = [...list.querySelectorAll<HTMLElement>("[data-queue-row]")]
      .map((element) => ({ element, index: Number(element.dataset.queueRow) }))
      .filter((row) => canReorder(row.index))
      .map((row) => {
        const rect = row.element.getBoundingClientRect();
        return { ...row, mid: rect.top + rect.height / 2 + scroll, shift: "" };
      });
    const self = rows.find((row) => row.index === index);
    if (!self || rows.length < 2) return;
    drag.current = {
      from: index,
      to: index,
      pointerId: event.pointerId,
      element: event.currentTarget,
      startY: event.clientY,
      pointerY: event.clientY,
      startScroll: scroll,
      rows,
      scroller,
      active: false,
      frame: 0,
    };
  };

  const moveDrag = (event: ReactPointerEvent<HTMLDivElement>) => {
    const state = drag.current;
    if (!state || event.pointerId !== state.pointerId) return;
    state.pointerY = event.clientY;
    if (!state.active) {
      if (Math.abs(event.clientY - state.startY) < 5) return;
      state.active = true;
      suppressClick.current = true;
      state.element.setPointerCapture(event.pointerId);
      state.element.classList.add("queue-row--lifted");
      listRef.current?.classList.add("queue-list--dragging");
      autoScroll();
    }
    layout();
  };

  const endDrag = (event: ReactPointerEvent<HTMLDivElement>, commit: boolean) => {
    const state = drag.current;
    if (!state || event.pointerId !== state.pointerId) return;
    drag.current = undefined;
    window.cancelAnimationFrame(state.frame);
    if (!state.active) return;
    // The click that follows a drop is not a request to play the row.
    window.setTimeout(() => { suppressClick.current = false; }, 0);
    if (commit && state.to !== state.from) {
      settling.current = true;
      playback.moveQueueItem(state.from, state.to);
    } else {
      clearOffsets();
    }
  };

  return (
    <div className="queue-list" ref={listRef}>
      {remote && <p className="queue-remote-note">{remote.controls ? `The queue on ${remote.name}` : `Update Splynt on ${remote.name} to change its queue from here`}</p>}
      {!remote && playback.undoQueueLabel && <div className="queue-undo" role="status"><span>{playback.undoQueueLabel}</span><button onClick={playback.undoQueueMutation} type="button">Undo</button></div>}
      {playback.queue.map((song, index) => index < playback.index ? null : (
        <Fragment key={keys[index]}>
          {index === playback.index && <p className="queue-section-label">Now playing</p>}
          {index === manualStart && playback.manualQueueCount > 0 && <div className="queue-section-label"><span>Next in queue</span>{!remote && <button onClick={playback.clearManualQueue} type="button">Clear queue</button>}</div>}
          {index === contextStart && index > playback.index && <div className="queue-section-label"><span>Next from: {playback.contextLabel}</span>{!remote && <button onClick={playback.clearUpcoming} type="button">Clear</button>}</div>}
        <div
          className={index === playback.index ? "queue-row queue-row--active" : "queue-row"}
          data-queue-row={index}
          onLostPointerCapture={(event) => endDrag(event, false)}
          onPointerCancel={(event) => endDrag(event, false)}
          onPointerDown={(event) => beginDrag(event, index)}
          onPointerMove={moveDrag}
          onPointerUp={(event) => endDrag(event, true)}
        >
          <button className="queue-row__main" onClick={() => { if (!suppressClick.current) playback.skipTo(index); }} type="button">
            <MediaArtwork alt="" className="queue-row__art" coverArt={song.coverArt} />
            <span><strong>{song.title}</strong><small>{song.artist}</small></span>
            {index === playback.index && playback.isPlaying && <span className="playing-bars" aria-label="Playing"><i /><i /><i /></span>}
          </button>
          {index !== playback.index && !remote && (
            <span className="queue-row__tools">
              {canReorder(index) && <button aria-label={`Reorder ${song.title}`} className="queue-row__grip" data-queue-reorder={index} onKeyDown={(event) => {
                if (event.key !== "ArrowUp" && event.key !== "ArrowDown") return;
                const target = event.key === "ArrowUp" ? index - 1 : index + 1;
                if (!canReorder(target)) return;
                event.preventDefault();
                playback.moveQueueItem(index, target);
                window.requestAnimationFrame(() => document.querySelector<HTMLButtonElement>(`[data-queue-reorder='${target}']`)?.focus());
              }} title="Drag, or use the arrow keys, to reorder" type="button"><GripVertical size={15} /></button>}
              <button aria-label={`Remove ${song.title}`} onClick={() => playback.removeQueueItem(index)} type="button"><X size={14} /></button>
            </span>
          )}
        </div>
        </Fragment>
      ))}
      {!remote && playback.index === playback.queue.length - 1 && <AutoplayNote status={autoplay} />}
      <div className="queue-list__footer"><button className="pill-button" onClick={onSaveQueue} type="button">Save queue as playlist</button></div>
    </div>
  );
}

function activeLyricIndex(lyrics: LyricsResult, position: number) {
  return lyrics.synced
    ? lyrics.lines.reduce((active, line, index) => line.start <= position + 0.12 ? index : active, 0)
    : -1;
}


/// One glyph for every device made the picker read as a list of identical
/// phones. The platform string is already on the wire; nothing was using it.
export function DeviceIcon({ platform, size = 21 }: { platform: string; size?: number }) {
  if (platform === "tvOS" || platform === "Apple TV") return <Tv size={size} />;
  if (platform === "iPhone" || platform === "iOS") return <Smartphone size={size} />;
  if (platform === "macOS" || platform === "Windows" || platform === "Linux") return <Laptop size={size} />;
  return <MonitorSpeaker size={size} />;
}

/// What this computer is to that device, in three words or fewer. During a
/// session every row looked identical, and which device leads is the thing a
/// listener most needs to read off this panel.
function roleBadge(peer: ConnectPeer, remoteDeviceId?: string, groupId?: string) {
  if (peer.id === remoteDeviceId) return peer.playback.isPlaying ? "Playing" : "Paused";
  if (groupId && peer.commitment?.sessionID === groupId) return "In this session";
  if (peer.commitment?.controllingPeerID) return "Acting as a remote";
  if (peer.commitment?.sessionID) return "In another session";
  return undefined;
}

function ConnectPanel({ canHandoff, groupId, localPlaying, onMoveHere, onMoveToDevice, onPlayHere, onSend, onStartGroup, onStopGroup, remoteDeviceId, snapshot }: {
  canHandoff: boolean;
  groupId?: string;
  /// This computer's own audio is playing.
  localPlaying: boolean;
  onMoveHere: (peer: ConnectPeer) => void;
  onMoveToDevice: (peer: ConnectPeer) => void;
  onPlayHere: () => void;
  onSend: (peerId: string, command: ConnectCommand) => void;
  onStartGroup: () => void;
  onStopGroup: () => void;
  remoteDeviceId?: string;
  snapshot: ConnectSnapshot;
}) {
  const followed = snapshot.peers.find((peer) => peer.id === remoteDeviceId);
  const localActive = !followed;
  return (
    <div className="connect-panel">
      {/* This computer is a device in the list like any other, as Spotify's
          picker has it. Choosing it while another device plays brings the
          playback here; while this computer is the one playing, it is
          already chosen. */}
      <button
        aria-label="This computer"
        aria-pressed={localActive}
        className={localActive ? "connect-local connect-local--active" : "connect-local"}
        disabled={localActive}
        onClick={onPlayHere}
        title={localActive ? "Playing on this computer" : `Move playback from ${followed ? deviceName(followed) : "that device"} to this computer`}
        type="button"
      >
        <MonitorSpeaker size={21} />
        <span>
          <strong>This computer</strong>
          <small>{!snapshot.isAvailable ? "Local discovery unavailable" : localActive ? (localPlaying ? "Playing here" : "Visible on your local network") : "Select to play here"}</small>
        </span>
        <i className={snapshot.isAvailable ? "connect-dot connect-dot--online" : "connect-dot"} />
      </button>
      {canHandoff && !remoteDeviceId && snapshot.peers.length > 0 && (
        <button className={groupId ? "group-session group-session--active" : "group-session"} onClick={groupId ? onStopGroup : onStartGroup} type="button">
          <MonitorSpeaker size={18} /><span><strong>{groupId ? "End group session" : `Play on all ${snapshot.peers.length + 1} devices`}</strong><small>{groupId ? "This computer is keeping the group in sync." : "Start a synchronized session with the players below."}</small></span>
        </button>
      )}
      {snapshot.peers.length ? snapshot.peers.map((peer) => (
        <section className={peer.id === remoteDeviceId ? "connect-device connect-device--active" : "connect-device"} key={peer.id}>
          <div className="connect-device__identity">
            <DeviceIcon platform={peer.platform} />
            <span>
              <strong>{deviceName(peer)}</strong>
              <small>{peer.platform}{peer.playback.title ? ` · ${peer.playback.title}` : " · Not playing"}</small>
            </span>
            {roleBadge(peer, remoteDeviceId, groupId) && (
              <em className="connect-device__role">{roleBadge(peer, remoteDeviceId, groupId)}</em>
            )}
          </div>
          {peer.playback.trackID && (
            <><div className="connect-device__track">
              <MediaArtwork alt="" className="queue-row__art" coverArt={peer.playback.coverArtID} />
              <span><strong>{peer.playback.title}</strong><small>{peer.playback.artist}</small></span>
            </div><PlaybackProgress className="connect-device__progress" duration={peer.playback.duration} isPlaying={peer.playback.isPlaying} onSeek={(value) => onSend(peer.id, { name: "seek", value })} position={peer.playback.position} /></>
          )}
          {peer.id === remoteDeviceId && !hasRemoteControl(peer.playback) && (
            <p className="connect-device__note">Shuffle, repeat, volume and the queue need a Splynt update on {deviceName(peer)}.</p>
          )}
          <div className="connect-device__controls">
            <button aria-label={`Previous on ${deviceName(peer)}`} onClick={() => onSend(peer.id, { name: "previous" })} type="button"><SkipBack fill="currentColor" size={16} /></button>
            <button aria-label={`${peer.playback.isPlaying ? "Pause" : "Resume"} on ${deviceName(peer)}`} onClick={() => onSend(peer.id, { name: peer.playback.isPlaying ? "pause" : "play" })} type="button">{peer.playback.isPlaying ? <Pause fill="currentColor" size={17} /> : <Play fill="currentColor" size={17} />}</button>
            <button aria-label={`Next on ${deviceName(peer)}`} onClick={() => onSend(peer.id, { name: "next" })} type="button"><SkipForward fill="currentColor" size={16} /></button>
          </div>
          {/* One primary per row, and while this computer is driving a device
              the primary is the way home. "Play here" and "Play on <device>"
              used to sit side by side at equal weight, with "here" meaning a
              different machine than the card it was printed on. */}
          <div className="connect-device__actions">
            {peer.playback.trackID && (
              <button className={peer.id === remoteDeviceId ? "connect-primary" : undefined} onClick={() => onMoveHere(peer)} type="button">
                Play on this computer
              </button>
            )}
            {canHandoff && peer.id !== remoteDeviceId && (
              <button className="connect-primary" onClick={() => onMoveToDevice(peer)} type="button">Play on {deviceName(peer)}</button>
            )}
          </div>
        </section>
      )) : <PanelEmpty icon={MonitorSpeaker} text="No other Splynt players found on this network." />}
    </div>
  );
}

/// The line under the last queued song, worded as iOS words it.
function AutoplayNote({ status }: { status?: AutoplayStatus }) {
  if (!status) return <p className="queue-autoplay queue-autoplay--quiet">Nothing else is queued.</p>;
  const title = status.state === "preparing" ? "Preparing Autoplay" : status.state === "ready" ? "Autoplay is ready" : "Autoplay isn't ready";
  const detail = status.state === "unavailable" ? "Playback will stop unless another song is added" : `Will continue with ${status.label}`;
  return (
    <div className={`queue-autoplay queue-autoplay--${status.state}`} role="status">
      {status.state === "unavailable" ? <WifiOff aria-hidden="true" size={17} /> : <Radio aria-hidden="true" size={17} />}
      <span><strong>{title}</strong><small>{detail}</small></span>
    </div>
  );
}

function PanelEmpty({ icon: Icon, text }: { icon: typeof ListMusic; text: string }) {
  return <div className="panel-empty"><Icon size={36} /><p>{text}</p></div>;
}

type FullPlayerStyle = CSSProperties & { "--player-color": string };
export type FullPlayerSurface = "artwork" | "lyrics";

/// Spotify paints its now-playing view in a saturated version of the cover.
/// The averaged sample is muddy, so saturation is pushed up and lightness held
/// in a band where white type stays readable. Near-greys are left grey.
export function vividColor(rgb?: string) {
  const match = rgb?.match(/\d+(\.\d+)?/g);
  if (!match || match.length < 3) return undefined;
  const [red, green, blue] = match.slice(0, 3).map((value) => Number(value) / 255);
  const max = Math.max(red, green, blue);
  const min = Math.min(red, green, blue);
  const lightness = (max + min) / 2;
  const delta = max - min;
  const saturation = delta === 0 ? 0 : delta / (1 - Math.abs(2 * lightness - 1));
  let hue = 0;
  if (delta) {
    if (max === red) hue = ((green - blue) / delta) % 6;
    else if (max === green) hue = (blue - red) / delta + 2;
    else hue = (red - green) / delta + 4;
  }
  hue = (hue * 60 + 360) % 360;
  const vividSaturation = saturation < 0.08 ? saturation : Math.min(0.85, saturation * 1.6 + 0.15);
  const vividLightness = Math.min(0.4, Math.max(0.26, lightness));
  return `hsl(${hue.toFixed(0)} ${(vividSaturation * 100).toFixed(0)}% ${(vividLightness * 100).toFixed(0)}%)`;
}

export function FullPlayer({ liked, lyrics, lyricsAutoScroll, lyricsLoading, lyricsTextSize, onClose, onOpenAlbum, onOpenArtist, onOpenPanel, onSurface, onToggleLike, onToggleWindowFullscreen, playback, surface, windowFullscreen }: {
  liked: boolean;
  lyrics: LyricsResult;
  lyricsAutoScroll: boolean;
  lyricsLoading: boolean;
  lyricsTextSize: "small" | "standard" | "large";
  onClose: () => void;
  onOpenAlbum: (id: string) => void;
  onOpenArtist: (id: string) => void;
  onOpenPanel: (mode: ContextPanelMode) => void;
  onSurface: (surface: FullPlayerSurface) => void;
  onToggleLike: () => void;
  onToggleWindowFullscreen: () => void;
  playback: PlaybackController;
  surface: FullPlayerSurface;
  windowFullscreen: boolean;
}) {
  const [menuOpen, setMenuOpen] = useState(false);
  const exitRef = useRef<HTMLButtonElement>(null);
  const scrollRef = useRef<HTMLDivElement>(null);
  const artworkColor = useArtworkColor(playback.current?.coverArt);
  useEffect(() => {
    exitRef.current?.focus();
    return () => document.querySelector<HTMLButtonElement>("[data-full-player-toggle]")?.focus();
  }, []);
  // Lyrics slide in beside the art, so bring that row back into view if the
  // cards below were scrolled to.
  useEffect(() => {
    if (surface !== "lyrics") return;
    scrollRef.current?.scrollTo?.({ top: 0, behavior: prefersReducedMotion() ? "auto" : "smooth" });
  }, [surface]);
  if (!playback.current) return null;
  const current = playback.current;
  const quality = [current.bitRate ? `${current.bitRate} kbps` : "Original", current.suffix?.toUpperCase()].filter(Boolean).join(" · ");
  const style = { "--player-color": vividColor(artworkColor) ?? "hsl(150 30% 26%)" } as FullPlayerStyle;
  const showPanel = (mode: ContextPanelMode) => { setMenuOpen(false); onOpenPanel(mode); };
  const openAlbum = () => { if (current.albumId) { onClose(); onOpenAlbum(current.albumId); } };
  const openArtist = () => { if (current.artistId) { onClose(); onOpenArtist(current.artistId); } };
  const action = (active: boolean) => active ? "full-player__header-action full-player__header-action--active" : "full-player__header-action";

  return (
    <section
      aria-label="Expanded player"
      className={`full-player full-player--${surface}`}
      onKeyDown={(event) => {
        if (event.key === "Escape" && menuOpen) {
          event.stopPropagation();
          setMenuOpen(false);
        }
      }}
      style={style}
    >
      <header className="full-player__header">
        <strong className="full-player__context">{playback.contextLabel}</strong>
        <div className="full-player__header-actions">
          <div aria-label="Expanded player view" className="full-player__view-options" role="radiogroup">
            <button aria-checked={surface === "artwork"} aria-label="Show artwork" className={action(surface === "artwork")} onClick={() => onSurface("artwork")} role="radio" title="Show artwork" type="button"><Disc3 size={18} /></button>
            <button aria-checked={surface === "lyrics"} aria-label="Show lyrics" className={action(surface === "lyrics")} onClick={() => onSurface(surface === "lyrics" ? "artwork" : "lyrics")} role="radio" title={surface === "lyrics" ? "Hide lyrics" : "Show lyrics"} type="button"><MicVocal size={18} /></button>
          </div>
          <div className="full-player__menu-wrap">
            <button aria-expanded={menuOpen} aria-haspopup="menu" aria-label="More options" className="full-player__header-action" onClick={() => setMenuOpen((value) => !value)} title="More options" type="button"><MoreHorizontal size={20} /></button>
            {menuOpen && <div aria-label="Current track" className="full-player__menu" role="menu">
              {current.artistId && <button onClick={openArtist} role="menuitem" type="button"><UserRound size={16} />Go to artist</button>}
              {current.albumId && <button onClick={openAlbum} role="menuitem" type="button"><Disc3 size={16} />Go to album</button>}
              <button onClick={() => { onToggleLike(); setMenuOpen(false); }} role="menuitem" type="button"><Heart fill={liked ? "currentColor" : "none"} size={16} />{liked ? "Remove from Liked Songs" : "Save to Liked Songs"}</button>
              <button onClick={() => showPanel("queue")} role="menuitem" type="button"><ListMusic size={16} />Open queue</button>
              <button onClick={() => showPanel("connect")} role="menuitem" type="button"><MonitorSpeaker size={16} />Connect to a device</button>
            </div>}
          </div>
          <button aria-label={windowFullscreen ? "Exit full screen" : "Enter full screen"} aria-pressed={windowFullscreen} className="full-player__header-action" onClick={onToggleWindowFullscreen} title={windowFullscreen ? "Exit full screen" : "Enter full screen"} type="button">{windowFullscreen ? <Minimize size={18} /> : <Maximize size={18} />}</button>
          <button aria-label="Exit full player" className="full-player__close" onClick={onClose} ref={exitRef} title="Collapse" type="button"><Minimize2 size={18} /></button>
        </div>
      </header>

      <div className="full-player__scroll" ref={scrollRef}>
        <TrackSlide className="full-player__stage" direction={playback.trackDirection} slideKey={current.id} variant="cover">
          <div className="full-player__canvas">
            <div className="full-player__split">
              <section aria-label="Artwork" className="full-player__hero">
                <MediaArtwork alt={`${current.title} cover`} className="full-player__art" coverArt={current.coverArt} />
              </section>
              <section aria-label="Lyrics" className="full-player__lyrics-surface" inert={surface !== "lyrics"}>
                {lyricsLoading ? <p className="full-player__lyrics-empty">Loading lyrics…</p> : lyrics.lines.length ? <FullLyrics active={surface === "lyrics"} autoScroll={lyricsAutoScroll} lyrics={lyrics} playback={playback} textSize={lyricsTextSize} /> : <p className="full-player__lyrics-empty">Looks like we don't have the lyrics for this song.</p>}
              </section>
            </div>
          </div>
        </TrackSlide>
        <section aria-label="About current track" className="full-player__details" key={current.id}>
          <article className="full-player__detail-card full-player__detail-card--artist">
            <MediaArtwork alt="" className="full-player__detail-art" coverArt={current.coverArt} fallback="artist" />
            <h2>About the artist</h2>
            <div className="full-player__detail-row">
              <strong>{current.artist}</strong>
              {current.artistId && <button className="pill-button" onClick={openArtist} type="button">View artist</button>}
            </div>
          </article>
          <article className="full-player__detail-card">
            <h2>Credits</h2>
            <dl>
              <dt>{current.artist}</dt><dd>Main artist</dd>
              {current.album && <><dt>{current.album}</dt><dd>{current.albumId ? <button onClick={openAlbum} type="button">Album</button> : "Album"}{current.year ? ` • ${current.year}` : ""}</dd></>}
              <dt>{quality}</dt><dd>Audio</dd>
            </dl>
          </article>
        </section>
      </div>
    </section>
  );
}

/// `active` is false while the pane is parked off to the side of the artwork.
/// The lines stay mounted so the split can animate both ways, but following
/// playback in a pane nobody can see is just a smooth-scroll running forever.
function FullLyrics({ active, autoScroll, lyrics, playback, textSize }: { active: boolean; autoScroll: boolean; lyrics: LyricsResult; playback: PlaybackController; textSize: "small" | "standard" | "large" }) {
  const activeIndex = activeLyricIndex(lyrics, playback.position);
  const activeRef = useRef<HTMLButtonElement>(null);
  const columnRef = useRef<HTMLDivElement>(null);
  const positioned = useRef(false);
  useEffect(() => {
    if (!active) { positioned.current = false; return; }
    if (!autoScroll || !lyrics.synced) return;
    const column = columnRef.current;
    const line = activeRef.current;
    if (!column || !line || typeof column.scrollTo !== "function") return;
    // Scroll only the lyric column; scrollIntoView would also drag the whole
    // view down to the line. The first move is instant so opening lyrics lands
    // on the current line, then playback glides it.
    const top = line.offsetTop - column.clientHeight / 2 + line.offsetHeight / 2;
    column.scrollTo({ top, behavior: positioned.current && !prefersReducedMotion() ? "smooth" : "auto" });
    positioned.current = true;
  }, [active, activeIndex, autoScroll, lyrics.synced]);
  return (
    <div className={`full-lyrics full-lyrics--${textSize}`} ref={columnRef}>
      {lyrics.lines.map((line, index) => lyrics.synced ? (
        <button
          aria-current={index === activeIndex ? "true" : undefined}
          className={index === activeIndex ? "full-lyrics__line full-lyrics__line--active" : index < activeIndex ? "full-lyrics__line full-lyrics__line--past" : "full-lyrics__line"}
          key={`${line.start}-${index}`}
          onClick={() => playback.seek(line.start)}
          ref={index === activeIndex ? activeRef : undefined}
          type="button"
        >{line.value || "♪"}</button>
      ) : <p className="full-lyrics__line full-lyrics__line--plain" key={`${line.start}-${index}`}>{line.value || "♪"}</p>)}
    </div>
  );
}

function prefersReducedMotion() {
  return typeof window.matchMedia === "function" && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

export type TrackMenuState = { song: SongSummary; index: number; x: number; y: number };

export function TrackContextMenu({ downloaded, liked, menu, onAddToPlaylist, onClose, onCreatePlaylist, onDownload, onEnqueue, onOpenAlbum, onOpenArtist, onPlayNext, onRadio, onRemoveDownload, onRemoveFromPlaylist, onToggleLike, playlists, targetCount }: {
  downloaded: boolean;
  liked: boolean;
  menu: TrackMenuState;
  onAddToPlaylist: (playlist: PlaylistSummary) => void;
  onClose: () => void;
  onCreatePlaylist: () => void;
  playlists: PlaylistSummary[];
  onDownload: () => void;
  onEnqueue: () => void;
  onOpenAlbum?: () => void;
  onOpenArtist?: () => void;
  onPlayNext: () => void;
  onRadio: () => void;
  onRemoveDownload: () => void;
  onRemoveFromPlaylist?: () => void;
  onToggleLike: () => void;
  targetCount: number;
}) {
  const menuRef = useMenuFocus(onClose);
  const style = useAnchoredMenu(menuRef, menu.x, menu.y);
  const [flyoutOpen, setFlyoutOpen] = useState(false);
  const stop = (action: () => void) => (event: ReactMouseEvent) => { event.stopPropagation(); action(); onClose(); };
  const many = targetCount > 1;
  return (
    <div aria-label={many ? `${targetCount} songs` : menu.song.title} className="context-menu" ref={menuRef} role="menu" style={style}>
      <div className="context-menu__submenu" onMouseEnter={() => setFlyoutOpen(true)} onMouseLeave={() => setFlyoutOpen(false)}>
        <button aria-expanded={flyoutOpen} aria-haspopup="menu" onClick={(event) => { event.stopPropagation(); setFlyoutOpen((value) => !value); }} onKeyDown={(event) => { if (event.key === "ArrowRight") { event.preventDefault(); setFlyoutOpen(true); } }} role="menuitem" type="button"><Plus size={16} />Add to playlist<ChevronRight className="context-menu__chevron" size={15} /></button>
        {flyoutOpen && <PlaylistFlyout onClose={() => setFlyoutOpen(false)} onCreate={() => { onCreatePlaylist(); onClose(); }} onPick={(playlist) => { onAddToPlaylist(playlist); onClose(); }} playlists={playlists} />}
      </div>
      <button onClick={stop(onToggleLike)} role="menuitem" type="button"><span className={liked ? "context-menu__like context-menu__like--active" : "context-menu__like"}><LikeGlyph liked={liked} /></span>{liked ? "Remove from your Liked Songs" : "Save to your Liked Songs"}</button>
      <button onClick={stop(onEnqueue)} role="menuitem" type="button"><ListMusic size={16} />Add to queue</button>
      <button onClick={stop(onPlayNext)} role="menuitem" type="button"><ListEnd size={16} />Play next</button>
      <span className="context-menu__separator" />
      {!many && <button onClick={stop(onRadio)} role="menuitem" type="button"><Radio size={16} />Go to song radio</button>}
      {!many && onOpenArtist && <button onClick={stop(onOpenArtist)} role="menuitem" type="button"><UserRound size={16} />Go to artist</button>}
      {!many && onOpenAlbum && <button onClick={stop(onOpenAlbum)} role="menuitem" type="button"><Disc3 size={16} />Go to album</button>}
      <button onClick={stop(downloaded ? onRemoveDownload : onDownload)} role="menuitem" type="button">{downloaded ? <Trash2 size={16} /> : <Download size={16} />}{downloaded ? "Remove download" : "Download"}</button>
      {onRemoveFromPlaylist && <><span className="context-menu__separator" /><button onClick={stop(onRemoveFromPlaylist)} role="menuitem" type="button"><Trash2 size={16} />Remove from this playlist</button></>}
    </div>
  );
}

/// Spotify's "Add to playlist" submenu: a filter field, New playlist, and the
/// user's playlists. It opens to the right, or to the left near the edge.
function PlaylistFlyout({ onClose, onCreate, onPick, playlists }: { onClose: () => void; onCreate: () => void; onPick: (playlist: PlaylistSummary) => void; playlists: PlaylistSummary[] }) {
  const [query, setQuery] = useState("");
  const ref = useRef<HTMLDivElement>(null);
  const [side, setSide] = useState<"right" | "left">("right");
  const [lift, setLift] = useState(0);
  useLayoutEffect(() => {
    const rect = ref.current?.getBoundingClientRect();
    if (!rect) return;
    if (rect.right > window.innerWidth - 8) setSide("left");
    if (rect.bottom > window.innerHeight - 8) setLift(rect.bottom - window.innerHeight + 8);
  }, []);
  const needle = query.trim().toLowerCase();
  const matches = needle ? playlists.filter((playlist) => playlist.name.toLowerCase().includes(needle)) : playlists;
  return (
    <div className={`context-menu context-menu--flyout context-menu--flyout-${side}`} onKeyDown={(event) => { if (event.key === "ArrowLeft" || event.key === "Escape") { event.stopPropagation(); onClose(); } }} ref={ref} role="menu" style={{ marginTop: -lift }}>
      <label className="context-menu__search"><Search aria-hidden="true" size={14} /><input aria-label="Find a playlist" autoFocus onChange={(event) => setQuery(event.target.value)} onClick={(event) => event.stopPropagation()} placeholder="Find a playlist" value={query} /></label>
      <button onClick={(event) => { event.stopPropagation(); onCreate(); }} role="menuitem" type="button"><Plus size={16} />New playlist</button>
      <span className="context-menu__separator" />
      <div className="context-menu__scroll">
        {matches.map((playlist) => <button key={playlist.id} onClick={(event) => { event.stopPropagation(); onPick(playlist); }} role="menuitem" type="button">{playlist.name}</button>)}
        {!matches.length && <p className="context-menu__empty">{playlists.length ? "No playlist matches that name." : "You have no playlists yet."}</p>}
      </div>
    </div>
  );
}
