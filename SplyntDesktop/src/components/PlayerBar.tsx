import {
  Cloud, ListMusic, Maximize2, MicVocal, MonitorSpeaker, Pause, Play,
  PanelRightOpen, Repeat, Repeat1, Shuffle, SkipBack, SkipForward, Volume1, Volume2, VolumeX,
} from "lucide-react";
import type { PlaybackController } from "../hooks/usePlayback";
import type { ContextPanelMode } from "../types";
import { parseExternalSource } from "../lib/externalSource";
import { LikeGlyph } from "./Catalog";
import { MediaArtwork } from "./MediaArtwork";
import { PlaybackProgress, SmoothRange } from "./RangeSlider";
import { TrackSlide } from "./TrackSlide";

type PlayerBarProps = {
  playback: PlaybackController;
  panelMode?: ContextPanelMode;
  expanded: boolean;
  lyricsOpen: boolean;
  onOpenPanel: (mode: ContextPanelMode) => void;
  onOpenAlbum: (id: string) => void;
  onOpenArtist: (id: string) => void;
  onToggleExpanded: () => void;
  onToggleLike: () => void;
  onToggleLyrics: () => void;
  liked: boolean;
  /// Set while this window is acting as a remote for another Splynt device.
  /// The transport it is handed already routes to that device; this is only
  /// how the bar says so.
  remoteDevice?: { id: string; name: string };
  onOpenDevices: () => void;
};

function iconClass(active: boolean) {
  return active ? "player-icon player-icon--active" : "player-icon";
}

export function PlayerBar({ expanded, playback, panelMode, lyricsOpen, onOpenAlbum, onOpenArtist, onOpenDevices, onOpenPanel, onToggleExpanded, onToggleLike, onToggleLyrics, liked, remoteDevice }: PlayerBarProps) {
  const VolumeIcon = playback.volume === 0 ? VolumeX : playback.volume < 0.55 ? Volume1 : Volume2;
  const external = Boolean(parseExternalSource(playback.current?.id));
  const repeatLabel = playback.repeat === "off" ? "Enable repeat" : playback.repeat === "all" ? "Enable repeat one" : "Disable repeat";
  return (
    <footer className="desktop-player" aria-label="Player">
      <div className="player-identity">
        <TrackSlide className="player-identity__slide" direction={playback.trackDirection} slideKey={playback.current?.id ?? "empty"}>
          <button aria-label="Now playing view" aria-pressed={panelMode === "nowPlaying"} className="player-art-button" disabled={!playback.current} onClick={() => onOpenPanel("nowPlaying")} title="Now playing view" type="button">
            <MediaArtwork className="desktop-player__art" alt={playback.current ? `${playback.current.title} cover` : "No track selected"} coverArt={playback.current?.coverArt} />
          </button>
          <span className="player-identity__copy">
            {playback.current?.albumId ? <button className="player-identity__link player-identity__title" onClick={() => onOpenAlbum(playback.current!.albumId!)} type="button">{playback.current.title}{external && <Cloud aria-label="External source" size={11} />}</button> : <strong>{playback.current?.title ?? "No track selected"}{external && <Cloud aria-label="External source" size={11} />}</strong>}
            {playback.current?.artistId ? <button className="player-identity__link player-identity__artist" onClick={() => onOpenArtist(playback.current!.artistId!)} type="button">{playback.current.artist}</button> : <small>{playback.current?.artist ?? "Choose music from your server"}</small>}
          </span>
        </TrackSlide>
        {playback.current && (
          <button className={liked ? "player-like player-like--active" : "player-like"} aria-label={liked ? "Remove from Liked Songs" : "Save to Liked Songs"} onClick={onToggleLike} title={liked ? "Remove from Liked Songs" : "Save to Liked Songs"} type="button">
            <LikeGlyph liked={liked} />
          </button>
        )}
      </div>

      <div className="player-transport">
        <div className="player-transport__buttons">
          <button className={iconClass(playback.shuffle)} aria-label="Shuffle" title={playback.shuffle ? "Disable shuffle" : "Enable shuffle"} aria-pressed={playback.shuffle} onClick={() => playback.setShuffle((value) => !value)} type="button"><Shuffle size={16} /></button>
          <button className="player-icon" aria-label="Previous" title="Previous" disabled={!playback.current} onClick={playback.previous} type="button"><SkipBack fill="currentColor" size={16} /></button>
          <button className="player-play" aria-label={playback.isPlaying ? "Pause" : "Play"} disabled={!playback.current} onClick={playback.toggle} title={playback.isPlaying ? "Pause" : "Play"} type="button">
            {playback.isPlaying ? <Pause fill="currentColor" size={16} /> : <Play fill="currentColor" size={16} />}
          </button>
          <button className="player-icon" aria-label="Next" title="Next" disabled={!playback.current} onClick={playback.next} type="button"><SkipForward fill="currentColor" size={16} /></button>
          <button className={iconClass(playback.repeat !== "off")} aria-label={`Repeat ${playback.repeat}`} title={repeatLabel} onClick={playback.cycleRepeat} type="button">
            {playback.repeat === "one" ? <Repeat1 size={16} /> : <Repeat size={16} />}
          </button>
        </div>
        <PlaybackProgress disabled={!playback.current} duration={playback.duration} isPlaying={playback.isPlaying} onSeek={playback.seek} position={playback.position} />
        {playback.error && <span className="player-error" role="status">{playback.error}</span>}
      </div>

      <div className="player-actions">
        {remoteDevice && (
          <button aria-label={`Playing on ${remoteDevice.name}`} className="player-remote" onClick={onOpenDevices} title={`Playing on ${remoteDevice.name}. Open devices`} type="button">
            <span>{remoteDevice.name}</span>
          </button>
        )}
        <button className={iconClass(panelMode === "nowPlaying")} aria-label="Now playing view" title="Now playing view" aria-pressed={panelMode === "nowPlaying"} onClick={() => onOpenPanel("nowPlaying")} type="button"><PanelRightOpen size={16} /></button>
        <button className={iconClass(lyricsOpen)} aria-label="Lyrics" title="Lyrics" aria-pressed={lyricsOpen} disabled={!playback.current} onClick={onToggleLyrics} type="button"><MicVocal size={16} /></button>
        <button className={iconClass(panelMode === "queue")} aria-label="Queue" title="Queue" aria-pressed={panelMode === "queue"} onClick={() => onOpenPanel("queue")} type="button"><ListMusic size={16} /></button>
        <button className={iconClass(panelMode === "connect" || Boolean(remoteDevice))} aria-label="Splynt Connect devices" title="Connect to a device" aria-pressed={panelMode === "connect"} onClick={() => onOpenPanel("connect")} type="button"><MonitorSpeaker size={16} /></button>
        <button className="player-icon" aria-label={playback.volume === 0 ? "Unmute" : "Mute"} title={playback.volume === 0 ? "Unmute" : "Mute"} onClick={() => playback.setVolume(playback.volume === 0 ? 0.8 : 0)} type="button"><VolumeIcon size={16} /></button>
        <SmoothRange aria-label="Volume" max={1} min={0} onChange={playback.setVolume} step={0.005} value={playback.volume} />
        <button aria-label={expanded ? "Exit full player" : "Open full player"} aria-pressed={expanded} className="player-icon" data-full-player-toggle disabled={!playback.current} onClick={onToggleExpanded} title={expanded ? "Exit full screen" : "Full screen"} type="button"><Maximize2 size={16} /></button>
      </div>
    </footer>
  );
}
