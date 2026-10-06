export type AlbumSummary = {
  id: string;
  title: string;
  artist: string;
  artistId?: string;
  year?: number;
  coverArt?: string;
  duration?: number;
  songCount?: number;
  created?: string;
  starred?: string;
};

export type ArtistSummary = {
  id: string;
  name: string;
  coverArt?: string;
  albumCount?: number;
  starred?: string;
};

export type PlaylistSummary = {
  id: string;
  name: string;
  owner?: string;
  coverArt?: string;
  songCount?: number;
  duration?: number;
  changed?: string;
  public?: boolean;
};

export type SongSummary = {
  id: string;
  title: string;
  artist: string;
  artistId?: string;
  album: string;
  albumId?: string;
  coverArt?: string;
  duration?: number;
  track?: number;
  discNumber?: number;
  year?: number;
  suffix?: string;
  bitRate?: number;
  starred?: string;
  created?: string;
  explicitStatus?: string;
};

export type AlbumDetail = AlbumSummary & { songs: SongSummary[] };
export type PlaylistDetail = PlaylistSummary & { songs: SongSummary[] };
export type ArtistDetail = ArtistSummary & { albums: AlbumSummary[]; topSongs?: SongSummary[]; appearances?: AlbumSummary[] };

export type ServerInfo = {
  displayHost: string;
  username: string;
  serverType?: string;
  serverVersion?: string;
  apiVersion?: string;
};

export type ConnectedLibrary = {
  server: ServerInfo;
  albums: AlbumSummary[];
  connection?: {
    status: "online" | "offline";
    message?: string;
  };
};

export type SavedProfileSummary = {
  id: string;
  displayHost: string;
  username: string;
  allowsSelfSigned: boolean;
  isActive: boolean;
};

export type GenreShelf = {
  name: string;
  albums: AlbumSummary[];
};

export type HomeOverview = {
  newest: AlbumSummary[];
  recent: AlbumSummary[];
  frequent: AlbumSummary[];
  random: AlbumSummary[];
  genres: GenreShelf[];
};

export type JumpBackInItem = {
  kind: "album" | "artist" | "playlist";
  id: string;
  title: string;
  subtitle: string;
  coverArt?: string;
};

export type HomeShortcut = {
  kind: "album" | "playlist" | "liked";
  id: string;
  title: string;
  subtitle: string;
  coverArt?: string;
};

export type LibraryOverview = {
  albums: AlbumSummary[];
  artists: ArtistSummary[];
  playlists: PlaylistSummary[];
  starredSongs: SongSummary[];
  starredAlbums: AlbumSummary[];
  starredArtists: ArtistSummary[];
};

export type SearchResults = {
  songs: SongSummary[];
  albums: AlbumSummary[];
  artists: ArtistSummary[];
};

export type LyricsLine = {
  start: number;
  value: string;
};

export type LyricsResult = {
  synced: boolean;
  lines: LyricsLine[];
};

export type PlayQueueSnapshot = {
  songs: SongSummary[];
  currentId?: string;
  position: number;
};

export type RadioResult = {
  title: string;
  songs: SongSummary[];
};

/// A song whose lyrics hold every word searched for, with the words in context.
export type LyricsMatch = { song: SongSummary; snippet: string };

/// How far the lyrics index has got. `total` is 0 until a crawl has listed
/// the server's songs this session.
export type LyricsIndexStatus = { scanned: number; searchable: number; total: number; running: boolean };

/// The connected Navidrome user's listening record. Either half is null when
/// the server does not offer it.
export type ListeningSnapshot = {
  plays: { trackId: string; at: number }[] | null;
  playedSongs: (SongSummary & { playCount: number; genre?: string })[] | null;
};

/// What the queue says about Autoplay while the last queued song plays, as
/// on iOS. `seed` is the song the continuation was fetched for.
export type AutoplayStatus =
  | { state: "preparing"; label: string; seed: string }
  | { state: "ready"; label: string; seed: string; songs: SongSummary[] }
  | { state: "unavailable"; label: string; seed: string };

export type DownloadItem = {
  song: SongSummary;
  bytes: number;
  downloadedAt: number;
  fileName: string;
};

export type DownloadProgress = {
  id: string;
  received: number;
  total?: number;
  status: "downloading" | "paused" | "complete" | "failed";
  message?: string;
};

export type DownloadFailure = {
  song: SongSummary;
  message: string;
  paused: boolean;
};

export type ConnectRequest = {
  server: string;
  username: string;
  password: string;
  allowsSelfSigned: boolean;
  rememberMe: boolean;
};

export type DesktopRoute =
  | { kind: "home" }
  | { kind: "search" }
  | { kind: "library" }
  /// With `artist`, Liked Songs narrowed to that artist, opened from the
  /// artist page as on iOS.
  | { kind: "liked"; artist?: { id: string; name: string } }
  | { kind: "downloads" }
  | { kind: "profile" }
  | { kind: "settings" }
  | { kind: "history" }
  | { kind: "stats" }
  | { kind: "radio"; id: string; title: string }
  | { kind: "album"; id: string }
  | { kind: "playlist"; id: string }
  | { kind: "artist"; id: string };

export type RepeatMode = "off" | "all" | "one";

/// Wire field names follow `SplyntConnect.swift`, which `docs/connect/WIRE-V1.md`
/// names as the contract these have to match byte-for-byte. Swift synthesises
/// its coding keys from property names, so the id keys are `trackID`, not the
/// `trackId` a camelCase convention would produce. The desktop spent its whole
/// life sending the latter, which decoded on another desktop and on nothing
/// else.
export type ConnectPlayback = {
  trackID?: string;
  title?: string;
  artist?: string;
  album?: string;
  coverArtID?: string;
  isPlaying: boolean;
  position: number;
  duration: number;
  /// The remote control extension in `docs/connect/WIRE-V1.md`. A peer that
  /// sends none of these is a v1 peer, and its shuffle, repeat, volume and
  /// queue controls are disabled rather than faked.
  shuffle?: boolean;
  repeatMode?: string;
  volume?: number;
  queueRevision?: number;
  queueIndex?: number;
  queueLength?: number;
  contextLabel?: string;
};

/// A device's queue, or the 1,000-ID window of it one frame carries. `index`
/// counts from the start of the whole queue, so the current track is
/// `trackIDs[index - offset]`.
export type ConnectQueue = {
  revision: number;
  offset: number;
  index: number;
  trackIDs: string[];
  contextLabel?: string;
};

export type ConnectPeer = {
  id: string;
  name: string;
  platform: string;
  playback: ConnectPlayback;
  updatedAt: number;
  commitment?: ConnectCommitment;
};

export type ConnectHandoff = {
  trackIDs: string[];
  currentTrackID: string;
  position: number;
  isPlaying: boolean;
};

export type ConnectGroup = {
  id: string;
  leaderID: string;
  trackID: string;
  position: number;
  isPlaying: boolean;
  sentAt: number;
  /// Monotonic within a session, advanced by the leader on every accepted
  /// change. Absent from a v1 peer's frames, read as 0.
  revision?: number;
};

/// What a device is committed to, published on every state frame. This is the
/// field that makes "an output belongs to one session at a time" checkable.
export type ConnectCommitment = {
  sessionID?: string;
  leaderID?: string;
  revision: number;
  /// Set when the device is driving another device's audio instead of its own.
  controllingPeerID?: string;
};

/// A follower's answer to a group join. v1 had none, which is why a leader
/// could report a member it had never reached.
export type ConnectGroupReply = {
  sessionID: string;
  deviceID: string;
  revision: number;
  reason?: string;
};

export type ConnectGroupJoin = {
  group: ConnectGroup;
  handoff: ConnectHandoff;
};

/// One round of the four-timestamp exchange that measures a peer's clock.
/// Answered entirely in Rust, so this type exists for completeness rather than
/// because the webview ever sees one: `timePing`/`timePong` never reach the
/// command queue.
export type ConnectTimeProbe = {
  id: string;
  t1: number;
  t2?: number;
  t3?: number;
};

export type ConnectCommand = {
  name: "play" | "pause" | "toggle" | "previous" | "next" | "seek" | "handoff" | "groupJoin" | "groupSync" | "groupLeave" | "groupAccept" | "groupDecline" | "timePing" | "timePong"
    | "setShuffle" | "setRepeat" | "setVolume" | "skipTo" | "enqueue" | "playNext" | "queueRequest" | "queueState";
  value?: number;
  handoff?: ConnectHandoff;
  group?: ConnectGroup;
  groupJoin?: ConnectGroupJoin;
  groupReply?: ConnectGroupReply;
  time?: ConnectTimeProbe;
  queue?: ConnectQueue;
  queueItem?: { index: number; trackID: string };
  tracks?: { trackIDs: string[] };
  /// Set by the transport on a received command: the peer whose connection
  /// carried it. Never on the wire.
  from?: string;
};

export type ConnectSnapshot = {
  isAvailable: boolean;
  localDeviceId?: string;
  peers: ConnectPeer[];
  commands: ConnectCommand[];
  /// Milliseconds to add to this device's clock to read each peer's, keyed by
  /// peer id. A peer that has not answered a probe yet is absent.
  clockOffsets?: Record<string, number>;
  /// The latest queue each live peer answered a `queueRequest` with.
  queues?: Record<string, ConnectQueue>;
};

export type ContextPanelMode = "nowPlaying" | "queue" | "connect";
