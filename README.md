<p align="center">
  <img src="assets/icon.png" alt="Splynt" width="140">
</p>

<h1 align="center">Splynt</h1>

<p align="center">
  A music player for your own <a href="https://www.navidrome.org/">Navidrome</a> or
  Subsonic-compatible server.
</p>

<p align="center">
  <a href="https://github.com/elijahgronda/Splynt/releases/latest"><strong>Download for Windows or Linux</strong></a>
  &nbsp;·&nbsp;
  <a href="https://discord.gg/kkaZfRpsm"><strong>Join the Discord</strong></a>
</p>

---

Splynt is a client, not a music service. You run the server and own the music,
and Splynt plays it. There is no bundled demo library. After you sign in, every
album, playlist and mix you see comes from your own server.

## Where it runs

| Device | Status | How to get it |
| --- | --- | --- |
| Windows | Released | `.exe` or `.msi` from the [latest release](https://github.com/elijahgronda/Splynt/releases/latest) |
| Linux | Released | `.AppImage` or `.deb` from the [latest release](https://github.com/elijahgronda/Splynt/releases/latest) |
| macOS | Build from source | See [building the desktop app](SplyntDesktop/README.md) |
| iPhone | TestFlight beta | Invites go out in the [Discord](https://discord.gg/kkaZfRpsm). An App Store release is planned |
| Apple TV | In development | Not available yet |
| Apple Watch | In development | Not available yet |

The desktop app is free and open source, and all of its code is in this
repository. The iPhone, Apple TV and Apple Watch apps are separate,
closed-source products. Their code is not published here.

## What it does

On every Splynt app:

- Browse your whole library live from your server: Home, Search, albums,
  artists, playlists and Liked Songs.
- Get Daily Mixes built from your library and what you actually play.
- Download music for offline listening, kept separate for each server you use.
- Shape the sound with a ten-band equalizer, twelve presets and a preamp. The
  curves match on desktop and iPhone, so Bass Boost sounds the same on both.

On the desktop:

- A library sidebar with filters and sorting, a top bar with Home and search,
  and a player along the bottom that stays put while you browse.
- Listening stats with your top songs, artists and albums, a listening clock,
  and streaks.
- Splynt Connect, which finds other Splynt desktops on your network and hands
  playback between them.
- Your credentials stay in the system keychain, and the app proxies artwork
  and audio without exposing your API token.

In the iPhone beta:

- Genre Mixes, Song Radio, and Autoplay that keeps going when your queue ends.
- Search that works offline and can find a song by a line from its lyrics.
- One saved server can hold several addresses, say one for home and one for
  away, and Splynt switches between them.

## Building the desktop app

The desktop app is Tauri 2 with a React front end and a Rust host. Build steps
for Windows, Linux and macOS are in [SplyntDesktop/README.md](SplyntDesktop/README.md).

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

The iPhone, Apple TV and Apple Watch apps are not in this repository, and this
license does not cover them.
