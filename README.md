<p align="center">
  <img src="assets/icon.png" alt="Splynt" width="120">
</p>

<h1 align="center">Splynt</h1>

<p align="center">
  A Spotify-style music player for your own
  <a href="https://www.navidrome.org/">Navidrome</a> or Subsonic-compatible server.
</p>

<p align="center">
  <a href="https://discord.gg/kkaZfRpsm"><strong>Join the iPhone beta on Discord</strong></a>
  &nbsp;·&nbsp;
  <a href="https://github.com/elijahgronda/Splynt/releases/latest"><strong>Download for Windows or Linux</strong></a>
</p>

<p align="center">
  <img src="assets/screenshots/now-playing.jpg" alt="Now playing, with the lyrics card under the controls" width="200">
  <img src="assets/screenshots/lyrics.jpg" alt="Synced lyrics, full screen" width="200">
  <img src="assets/screenshots/artist.jpg" alt="An artist page with popular songs and your play counts" width="200">
  <img src="assets/screenshots/album.jpg" alt="An album page with play counts and audio format on each track" width="200">
</p>

<p align="center">
  <img src="assets/screenshots/liked-songs.jpg" alt="Liked Songs, with genre filters" width="200">
  <img src="assets/screenshots/playlist.jpg" alt="A playlist" width="200">
  <img src="assets/screenshots/queue.jpg" alt="The queue, with mix, shuffle, repeat and a sleep timer" width="200">
  <img src="assets/screenshots/equalizer.jpg" alt="The ten-band equalizer" width="200">
</p>

---

I wanted Spotify's feel for the music on my own server, so I built it. You bring
a Navidrome or Subsonic server and your library. Splynt brings no music of its
own, and everything you see after you sign in comes from your server.

It's been my everyday player for a while, so it's in good shape. I also ship
updates often, and there are still rough edges. If you hit one, tell me in the
[Discord](https://discord.gg/kkaZfRpsm).

## What it does

- **Gapless playback and crossfade.** Albums play straight through with no
  silence between tracks, or you can fade each song into the next.
- **A ten-band equalizer.** Twelve presets and a preamp, applied to everything
  Splynt plays, streamed or downloaded.
- **Search your library, or search by lyrics.** Splynt indexes your lyrics, so
  a line you half remember is enough to find the song.
- **Synced lyrics.** They follow the song line by line, on a card under the
  player or full screen.
- **Artist pages with the whole discography.** Popular songs, every release,
  and how many times you've played each one.
- **Daily Mixes from your own listening.** Built from what you actually play.
- **Playlist suggestions.** Under each of your playlists, ten songs that fit
  it. Add one and another slides into its place.
- **Home Screen widgets.** One for what's playing, one for your listening stats.
- **A home feed you control.** Pick which rows Home shows.

There's more around the edges: offline downloads for each server you use, the
audio format on every track, play counts, a sleep timer, and one tap to save
the queue as a playlist.

## Get Splynt

| Device | How |
| --- | --- |
| iPhone | Beta on TestFlight. Invites go out in the [Discord](https://discord.gg/kkaZfRpsm). An App Store release is planned |
| Windows | `.exe` or `.msi` from the [latest release](https://github.com/elijahgronda/Splynt/releases/latest) |
| Linux | `.AppImage` or `.deb` from the [latest release](https://github.com/elijahgronda/Splynt/releases/latest) |
| macOS | [Build it from source](SplyntDesktop/README.md#building-on-a-mac) |
| Apple TV and Apple Watch | In development |

The desktop installers aren't signed yet, so Windows may warn you the first
time you run one. Choose **More info**, then **Run anyway**.

## The desktop app

The desktop app is free and open source, and its code is in this repository.
It has a library sidebar with filters and sorting, a top bar with Home and
search, and a player along the bottom that stays put while you browse. It
shares gapless playback, crossfade, the equalizer, synced lyrics and artist
discographies with the iPhone app. It adds listening stats and Splynt Connect,
which hands playback between Splynt desktops on your network.

It's built with Tauri 2, React and Rust. Build steps for Windows, Linux and
macOS are in [SplyntDesktop/README.md](SplyntDesktop/README.md).

## Docs

- [Changelog](CHANGELOG.md)
- [Desktop UX spec](docs/desktop/DESKTOP-UX-SPEC.md), the interaction rules the
  desktop app follows
- [Splynt Connect](docs/connect/README.md) and its
  [wire protocol](docs/connect/WIRE-V1.md), how Splynt apps find each other and
  hand off playback on a local network

## License

The code in this repository is licensed under the
[GNU GPL v3.0 or later](LICENSE). You can use, change and share it. If you
distribute a modified version, you have to release it under the GPL with its
source.

The iPhone, Apple TV and Apple Watch apps are separate, closed-source products.
Their code isn't in this repository, and this license doesn't cover them.
