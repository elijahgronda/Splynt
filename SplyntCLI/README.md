# Splynt CLI

Splynt in the terminal. It's a full music client for Navidrome and other
Subsonic servers that you run from a terminal window, and it's set up to
feel like a real app: there's a sidebar, cover art, a player bar, and the
mouse works everywhere.

It's new, so there are still things to iron out. If something breaks, let me
know in the [Discord](https://discord.gg/kkaZfRpsm).

## Install

On a Mac or Linux, paste this in:

```sh
curl -fsSL https://raw.githubusercontent.com/elijahgronda/Splynt/main/install.sh | sh
```

On Windows, paste this into PowerShell:

```powershell
irm https://raw.githubusercontent.com/elijahgronda/Splynt/main/install.ps1 | iex
```

Then just type `splynt`. Run the same line again to update. If you'd rather
grab the files yourself, they're in the newest
[Splynt CLI release](https://github.com/elijahgronda/Splynt/releases).

If you'd rather build it yourself, you need [Rust](https://rustup.rs):

```sh
cargo install --git https://github.com/elijahgronda/Splynt splynt
```

The first launch asks for your server, username and password. Your password
never gets saved. The config file (`~/.config/splynt/config.toml`) only
keeps the Subsonic salt and token, the same way the iPhone app does it.

## What it does

- **Search as you type.** One page with the top result, songs, artists,
  albums and your playlists. Your own library always comes before anything
  octo-fiesta finds, and it remembers your last ten searches.
- **Gapless and crossfade.** Albums play straight through, or fade songs
  into each other for up to 12 seconds.
- **The same equalizer as the iPhone app.** Ten bands, twelve presets and a
  preamp, with the music's live level behind each band.
- **A visualizer.** Bars, Mirror or Ribbon in the full screen player, plus a
  little one in the player bar.
- **Downloads and Offline Mode.** Press D on a song, album or playlist.
  Offline Mode only plays what's downloaded or cached.
- **Caching.** Songs you've played and the next couple in the queue get
  cached, so they start instantly and can always seek.
- **A real queue.** Drag songs around, shuffle or clear what's up next, and
  save the queue as a playlist.
- **Synced lyrics** from your server, or LRCLIB if your server doesn't have
  them.
- **ReplayGain, Autoplay and Fewer Repeats shuffle**, all under Settings.
- **Your queue follows you.** Quitting saves it to the server, so the
  iPhone app (or the next time you open the CLI) picks up right where you
  left off.

## Getting around

Everything works with the mouse. Click a card to open it, double click a
song to play it, right click for the menu, and drag the progress bar,
volume, sliders or queue rows.

From the keyboard:

| Key | What it does |
| --- | --- |
| arrows | move around the page |
| enter | open or play |
| esc | go back |
| 1 to 9 | jump to a sidebar page |
| space | play or pause |
| n, p | next or previous song |
| m | more options (same as right click) |
| a, A | add to queue, play next |
| D | download |
| / | search |
| f | full screen player |
| , | settings |
| ? | every key |

## Cover art

Splynt checks which images your terminal can show and uses the best one:

| Terminal | Images |
| --- | --- |
| Kitty, Ghostty | Kitty graphics |
| iTerm2, WezTerm, VS Code | iTerm2 images |
| foot, Windows Terminal, xterm with sixel | Sixel |
| anything else, including Terminal.app | pixel blocks |

VS Code needs `terminal.integrated.enableImages` turned on. You can switch
between real images, pixel blocks and no art under Settings, Appearance, or
press `i` anywhere.

Terminal.app doesn't do full color, so Splynt drops to the closest 256
colors there. It works, it just looks grainier. Ghostty or iTerm2 look way
better. On Windows, use Windows Terminal. It does full color, and newer versions
support Sixel, so they should show real cover art.

## Formats

MP3, FLAC, AAC/M4A, ALAC, Ogg Vorbis, WAV and AIFF play as they are.
Anything else, Opus included, gets asked for as an MP3 from your server.
Settings, Audio quality can cap streaming at 128, 256 or 320 kbps.

## If something goes wrong

There's a log at `~/Library/Logs/splynt/splynt.log` on a Mac and
`%LOCALAPPDATA%\splynt\splynt.log` on Windows (`splynt --log` prints where
it is). It has failed requests, dropped
streams, songs that wouldn't play and crashes. It never has your token in
it. Settings, About shows the last few lines too.

## Code

| File | What it does |
| --- | --- |
| `src/api.rs` | the Subsonic client, plus LRCLIB lyrics |
| `src/player.rs` | audio: streaming, decoding, equalizer, crossfade, visualizer |
| `src/store.rs` | the cache and downloads |
| `src/search.rs` | search ranking |
| `src/settings.rs` | the settings and their defaults |
| `src/prefs.rs` | the Settings pages |
| `src/art.rs` | cover art |
| `src/app.rs` | app state, the queue, keys and mouse |
| `src/ui.rs` | drawing everything |
| `src/theme.rs` | colors |
| `src/log.rs` | the log file |
