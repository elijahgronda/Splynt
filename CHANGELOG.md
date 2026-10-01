# Changelog

What changed in each Splynt release, newest first. Desktop installers are on
the [releases page](https://github.com/elijahgronda/Splynt/releases). iPhone
builds go to TestFlight testers. Invites are in the
[Discord](https://discord.gg/kkaZfRpsm).

## Desktop

### 1.1.0, 2026-10-01

The first desktop release from this repository, and the first under the Splynt
name. Earlier releases were called Splice and are still on the old
[Splynt-Desktop](https://github.com/elijahgronda/Splynt-Desktop/releases)
repository.

- A new layout: a full-width top bar with Home and search, a collapsible
  library sidebar with filter chips and sorting, and album and playlist pages
  tinted by their cover art, with a play header that stays visible as you
  scroll.
- Select songs from the right-click menu. The floating selection bar is gone.
- Lyrics slide in beside the artwork in the now playing view.
- Fixed: the track menu could open behind the player bar.
- Fixed: an open side panel covered the full player's controls.
- Fixed: a green tint over album headers.
- Fixed: unrelated albums showed up under "Appears on".

### 1.0.1, 2026-08-31

- A ten-band equalizer with twelve presets and a preamp. It uses the same band
  frequencies, filter shapes and preset curves as the iPhone app.
- Splynt Connect: fixed group membership, playback transfer and position
  reporting. A device that falls behind now catches up by adjusting its
  playback speed, not by jumping.

### 1.0.0, 2026-08-27

The first public release, for Windows and Linux.

## iPhone (TestFlight)

### 1.3.0 (6), 2026-09-30

- Pin to Home and Pin to Library are now two separate pins, and pinned albums
  stay put after they drop out of your recent plays.
- One saved server can hold several addresses, and Splynt switches between
  them.
- Shuffle belongs to the list it plays, not to the whole app.
- A new swipe on track rows.
- The full player opens scrolled to the top, touching the progress bar no
  longer starts the dismiss drag, and seeking lands exactly where you let go.
- Gapless playback no longer stalls after a seek near the end of a song.
- Daily Mixes recover from a refresh that was cancelled partway through.
- Smoother scrolling and animation on 120 Hz iPhones.

### 1.1.0 (5), 2026-09-21

- Search leads with a mixed group of the best matches.
- Search finds songs by their lyrics.
- Song Radio plays offline.
- Splynt recognises which kind of server it is talking to.

### 1.0.2 (3), 2026-09-17

The first TestFlight build recorded here.
