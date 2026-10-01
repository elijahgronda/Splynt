# Changelog

Everything that's changed in each release, newest first. Desktop installers are
on the [releases page](https://github.com/elijahgronda/Splynt/releases), and
iPhone builds go out on [TestFlight](https://testflight.apple.com/join/UWvEDqQc).

## Desktop

### 1.1.0, 2026-10-01

The first desktop release from this repo, and the first one actually called
Splynt! The older ones were still named Splice, and they're on the old
[Splynt-Desktop](https://github.com/elijahgronda/Splynt-Desktop/releases) repo.

- New layout that works a lot more like Spotify desktop. There's a top bar with
  Home and search, a library sidebar you can collapse with filters and sorting,
  and album and playlist pages tinted from the cover art with a play header
  that sticks while you scroll.
- Selecting songs is in the right-click menu now, and the floating selection
  bar is gone.
- Lyrics slide in next to the artwork on the now playing screen.
- Fixed the track menu opening behind the player bar.
- Fixed an open side panel covering the full player's controls.
- Fixed a green tint over album headers.
- Fixed random albums showing up under "Appears on".

### 1.0.1, 2026-08-31

- Added a ten-band equalizer with twelve presets and a preamp. It uses the same
  bands and presets as the iPhone app, so Bass Boost sounds the same on both.
- Fixed Splynt Connect groups, playback transfer and position reporting. If a
  device falls out of sync, it catches up by adjusting its speed a little
  instead of jumping.

### 1.0.0, 2026-08-27

First public release, for Windows and Linux!

## iPhone (TestFlight)

### 1.3.0 (6), 2026-09-30

- New swipe actions on song rows.
- Pinned albums stay on Home and in Your Library now, even after you haven't
  played them in a while.
- The full player opens scrolled to the top, touching the progress bar doesn't
  start closing the player anymore, and seeking lands exactly where you let go.
- Fixed gapless playback getting stuck after seeking near the end of a song.
- Worked on the UPnP volume slider only working sometimes and the seek bar
  jumping back to 0:00. Thanks for the reports! Let me know if it still
  happens.
- Opening an album from an artist's discography keeps you in the discography.
- Daily Mixes recover if a refresh gets interrupted.
- Smoother scrolling on 120 Hz iPhones.
- Started on CarPlay! It still needs Apple's approval before it shows up in
  your car.

### 1.2.0 (6), 2026-09-26

- Play to UPnP speakers and receivers on your network. Right now that means
  ones with OpenHome support.
- Playlist suggestions, plus editing playlists, custom playlist covers and a
  faster way to fill a playlist with songs.
- Pin to Home and Pin to Library are two separate pins now.
- A listening stats widget.
- One server can have more than one address, say one for home and one for
  away, and Splynt switches between them. You can limit an address to certain
  Wi-Fi networks too.
- Shuffle belongs to the playlist or album you're playing instead of the whole
  app.
- The full player shows the audio format and the signal path.

### 1.1.0 (5), 2026-09-21

- Search leads with the best matches from everything.
- Search finds songs by their lyrics.
- Song Radio works offline.
- Splynt figures out what kind of server it's talking to.

### 1.0.2 (3), 2026-09-17

The first TestFlight build!
