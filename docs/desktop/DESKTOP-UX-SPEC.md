# Splynt Desktop UX Specification

This document owns desktop interaction and presentation.

Status: authoritative foundation for the macOS, Windows, and Linux desktop clients.

Splynt Desktop is a music-only Navidrome/Subsonic client whose interaction model follows Spotify's desktop application while using Splynt's own identity, data, recommendation engine, and Splynt Connect protocol.

## 1. Product contract

The desktop client must feel immediately familiar to a Spotify desktop user:

- persistent library navigation on the left;
- a scrollable primary workspace in the center;
- an optional contextual panel on the right;
- a persistent player across the bottom;
- mouse, keyboard, media-key, drag-and-drop, and context-menu support throughout;
- navigation and playback never reset one another;
- the same account, library mutations, queue handoff, and playback state are available across Splynt clients.

This is behavioral parity, not a branded clone. Do not use Spotify names, logos, artwork, proprietary copy, or pixel-for-pixel assets. Splynt keeps its Figtree typography, `#121212` base, `#2BE06B` accent, and existing iconography.

Music is the complete content scope. Podcasts, audiobooks, advertisements, Spotify editorial programming, AI prompting, and social feeds are not placeholders and must not appear disabled.

## 2. Desktop shell

The app window has four persistent regions:

1. **Library rail** — left, 280 px default, user-resizable from 72–420 px.
2. **Workspace** — center, minimum useful width 520 px, owns page scrolling.
3. **Context panel** — right, 340 px default, user-resizable from 280–460 px and dismissible.
4. **Player bar** — bottom, 72 px fixed height, spans the complete window.

The title bar is integrated into the shell, and on Windows it *is* the title bar. macOS keeps the system window with overlay traffic lights and their safe leading inset. Windows runs undecorated and draws minimize, maximize, and close at the trailing edge of the top bar, because a system title bar plus a menu strip plus the app's own bar is three rows of chrome no music player ships. Linux keeps its window manager's decorations.

The top bar is a drag region on every platform. So is the library rail's own background, meaning the strip macOS reserves above the mark for its traffic lights, the mark itself, and the gaps between groups, and so is the context panel's header. A window a listener can only move by finding the one strip to the right of the sidebar is a window that feels stuck. A drag region must never overlap a button or a text field, and the window must actually move when it is dragged: that needs `core:window:allow-start-dragging` in the capability file, which `core:default` does not include. Whether a click drags or acts is Tauri's own rule: the bare attribute drags only on a direct hit, `deep` drags anywhere in the subtree, and a button inside either still behaves as a button.

### Window breakpoints

- **≥ 1280 px:** expanded library, workspace, and context panel may coexist.
- **960–1279 px:** context panel closes automatically but can open as an overlay.
- **720–959 px:** library defaults to compact 72 px mode; text labels move to tooltips.
- **< 720 px:** unsupported as a primary layout. Enforce a 680 × 560 minimum window, preserving player controls and current-track identity.

No page may maintain a separate bottom player. The persistent player belongs to the shell and survives every route.

## 3. Library rail

The upper navigation contains Home, Search, and Your Library. The selected destination uses ink plus a quiet raised surface; green is reserved for playback, liked state, and primary actions.

The library section contains:

- Create playlist;
- Liked Songs;
- pinned playlists, albums, and artists;
- one scrollable column holding playlists, artists and albums together, pinned rows first;
- filter chips that narrow that column to Playlists, Artists or Albums, each of which clears by pressing it again or by the clear control beside them;
- local library search;
- compact and expanded display modes;
- user ordering and pinning when supported locally.

A library row uses 48 px artwork, one primary line, and one metadata line. Hover exposes a play button over artwork. Double-click starts the item. Right-click opens the shared collection action menu. Dragging a track over an editable playlist highlights the destination and adding occurs on drop.

Collapsing the rail keeps collection artwork and exposes accessible tooltips. Expansion state, width, sort and the active chip persist per device. Sorting by Recents orders the column by what the listener actually opened, which is the only recency this client records; the rail's chips are its own and do not move the Library page's filter.

## 4. Workspace navigation

Back and Forward are browser-style history operations and must restore each page's prior scroll position. Home, Search, Library, collection, artist, settings, queue, and profile are routes. Opening the full player or context panel is not a route.

The top bar becomes opaque as content scrolls beneath it. The transition is tied directly to scroll offset; do not add a delayed animation. Page title, history buttons, global search where relevant, account menu, and window drag region share this bar.

Loading a route never interrupts audio. A failed shelf stays local to that shelf. A failed page offers Retry without resetting the navigation stack.

## 5. Home

Home opens with a time-aware greeting and a dense grid of recently used collections, followed by horizontal shelves. Initial shelf order:

1. Continue Listening / Recently Played;
2. Daily Mixes;
3. Recently Added;
4. Made for You / generated shelves;
5. Most Played;
6. Random Albums;
7. Genre Mixes.

The first viewport should contain useful content without scrolling. Shelf cards use 168–208 px responsive widths and never stretch artwork. Hover lifts the surface subtly and reveals a green circular play button. The play button starts the collection; clicking elsewhere opens it.

### The ambient wash

Every workspace page is washed at the top with a colour sampled from artwork, fading into the page background over 340 px, with the top bar carrying the same colour so the two meet without a seam. The colour is the sleeve of whatever the pointer is over, whether that is a card, a Home shortcut or a rail row, and it returns to the page's own subject when the pointer leaves: the collection's sleeve on a detail page, the station's opening track on a radio page, Liked Songs' purple, and a neutral grey on Home, Search and the library. A short grace on release means crossing the gutter between two cards does not flash back to neutral in between.

The crossfade is one CSS transition on a custom property, about a second, so a walk across a shelf reads as the page changing its mind rather than as a slideshow. Nothing about it costs a render per frame. Reduced Motion is not a factor here: a colour fade moves nothing.

This replaces the fixed green band Home used to carry, which said nothing about what was on the page and was the loudest object on it. Green stays reserved for playback, liked state and primary actions.

## 6. Search

Search focus is reachable with `Cmd/Ctrl+K`. Results update after a 200 ms debounce and are grouped into Top Result, Songs, Artists, Albums, and Playlists. Empty input presents browse categories and recent searches. `Escape` clears once, then leaves the field on a second press.

Arrow keys move through visible results; Return opens; Shift+Return starts playback. Search history is local and profile-scoped.

Search results apply the same native-over-external duplicate suppression as the iOS client before entering UI state. External octo-fiesta playlists remain hidden when the user's existing setting says so.

## 7. Collection and artist pages

Album and playlist headers use large artwork, metadata, owner/artist identity, track count, duration, primary Play/Shuffle action, like state, download state, and overflow actions.

Desktop track tables expose:

- index / playing indicator;
- title and explicit marker;
- album when the context needs it;
- date added when available;
- compact source quality;
- duration;
- hover actions for like and overflow.

Single click selects a row, double-click plays, Space toggles playback rather than starting the selected row, Return opens the selected row's album, and the context menu owns mutations. Multi-select uses platform conventions (`Cmd` on macOS, `Ctrl` elsewhere, Shift for ranges). Clicking away from the rows drops the selection.

One selected row is a highlighted row and nothing else. The bulk toolbar belongs to a bulk action, so it arrives with the second row; everything it offers for a single track is already in that track's context menu.

Artist pages contain hero identity, Follow/star state, popular tracks, discography, appearances when the server provides them, and artist radio. A missing editorial biography leaves no empty card.

## 8. Persistent player

The player bar has three balanced zones:

- **left:** 56 px artwork, title, artist, source indicator, like;
- **center:** shuffle, previous, play/pause, next, repeat, progress slider and timestamps;
- **right:** lyrics, queue, Splynt Connect devices, mute/volume, context-panel toggle, fullscreen player.

The central transport stays visually centered in the entire window, not merely in the space between the other zones. At narrow widths, secondary controls collapse before title or play/pause.

Clicking the compact artwork toggles the Now Playing context panel. Only the dedicated fullscreen control or platform shortcut expands the player.

### Playing on another device

Whichever Splynt device is playing is the active device, as in Spotify
Connect. This window follows it automatically while it plays nothing of its
own, under the rules in `docs/connect/WIRE-V1.md` ("Following the active
device"), and the listener can also choose a device in the Devices panel.
While it follows:

- the player bar, Now Playing panel, queue, lyrics and expanded player show
  the followed device's track, queue, play state, shuffle, repeat and volume,
  mirrored into the local player without loading any audio;
- every control goes to that device: play and pause, next, previous, seek,
  shuffle, repeat, volume, a queue row, Add to queue, Play next, and starting
  any song, album, playlist or radio, which hands it that queue;
- the queue panel offers no remove, reorder or clear, because no command does
  that to another device's queue;
- a full-width green strip under the player bar reads "Playing on <device>"
  with the device's icon and opens the Devices panel;
- the position shown is projected from the moment the transport received the
  device's frame, so it moves in step without waiting for the next one.

A device on a Splynt release from before the remote control extension cannot
be told to change shuffle, repeat, volume or its queue. Those controls are
disabled with a tooltip saying the device needs an update.

In the Devices panel, This computer is the first row. Choosing it moves
playback here with the same queue, track and position, and pauses the device
that was playing. Following also ends when the followed device leaves the
network, which leaves its queue loaded here, paused.

### Expanded player

The expanded player is content above the existing player bar, not a second player or modal dialog. It fills the window it is already in and leaves the window alone: expanding the artwork is not a request to take over the display. The same 72 px player, transport state, seek position, volume, and utilities remain mounted at the bottom.

Taking the whole display is a separate act with its own control and its own shortcut, and it composes with the expanded player rather than being how the expanded player is reached.

The first viewport is a dominant-color canvas with one centered artwork square, approximately 522 px at a 1440 × 900 window and responsive to the shorter axis. Small top-right controls switch between artwork and lyrics, open Queue or Splynt Connect as overlays, expose current-track actions, and exit fullscreen. Opening a panel never exits the expanded player.

Lyrics sit beside the artwork rather than replacing it. Turning them on moves the artwork off centre and scales it down while the lyrics column arrives from the right; the pair stays centred as a group and the artwork keeps its layout size throughout, so nothing below it shifts. Lyrics never duplicate transport controls. Below roughly 1040 px of window width there is no room for the pair, so the artwork fades out and the lyrics take the centre. Scrolling below the canvas reveals artist context, credits, related album information, and playback quality when the connected server exposes those fields.

Playback behavior must preserve the iOS invariants: manual queue before context, duplicate-safe queue entries, hard cuts on manual skips, gapless/crossfade only on natural transitions, stable history, shuffle order preservation, repeat semantics, recovery, scrobbling, and persisted queue position.

Media keys work when the window is unfocused. The OS now-playing surface publishes title, artist, album, artwork, duration, position, and play state. Closing the window follows the user's background-playback setting; Quit always stops playback.

## 9. Context panel

The panel has mutually exclusive modes:

- Now Playing;
- Queue;
- Lyrics;
- Splynt Connect.

Now Playing contains large artwork, metadata, source quality, artist context, credits, and related collection content when the existing APIs provide it.

Queue begins at the current track, then manual entries, then the remaining playback context, matching the actual engine order. Tracks already behind the playhead are history and belong to Previous; a queue that opens forty played rows deep is answering a question nobody asked. Rows support duplicate-safe reorder and removal, and playing a row moves the playhead within the existing queue rather than rebuilding it, so the manual entries above it survive.

Every row after the playhead can be reordered, by dragging it or with the arrow keys on its handle. Up Next is one list, as on iOS, so a hand-queued track can move down among the context tracks and a context track can move up among the hand-queued ones. Where a row lands decides its kind: above the line it counts as added by hand, below it as part of the context, and a row dropped exactly on the line keeps the kind it had. The drag is a pointer gesture, so the row lifts under the pointer and the rows it passes move aside. It is not HTML5 drag-and-drop, which offers no preview of where the drop lands. A row's context menu carries Play next, which lifts it to the front of the manual run in one step, plus Go to artist, Go to album and Remove from queue. A track the server would not play stays in the list, dimmed and labelled, rather than disappearing.

A manual entry plays next whatever the modes say. Shuffle reorders the context, never the songs someone lined up by hand. It rearranges the queue itself rather than choosing the next index behind it, so the Queue panel always shows the order that will play.

Lyrics follow the existing timed/plain fallback behavior. A timed sheet centres the line being sung. A plain one has no timings to follow, so it walks from the first line to the last across the track instead: one continuous pass, spread over the song's length and driven from the same interpolated clock as the playhead, holding at the top through the intro and at the bottom through the outro. Scrolling by hand stops the crawl for five seconds. Reduced Motion moves it a step per position report rather than per frame. Splynt Connect lists this device and peers, their active track, remote controls, and the handoff action.

Closing and reopening the panel restores its last mode. At reduced widths it becomes a modal side overlay and closes with Escape.

### Track-change motion

A track change cross-slides rather than swapping copy in place. The outgoing
track's content leaves toward the direction of travel — left when the queue
moved forward, right when it moved back — while the incoming track arrives from
the opposite edge. Both layers are on screen together for the length of the
slide; the outgoing one renders the previous track's artwork, tint and copy, and
is inert and hidden from assistive technology while it leaves.

Two surfaces carry it, at two scales:

- The player bar's artwork and identity travel a short distance and fade.
- The expanded player's canvas — tint, artwork and lyrics together — travels the
  full width at full opacity, so the first viewport reads as one card sliding to
  the next rather than as artwork changing inside a fixed frame.

Direction comes from how playback actually moved through the queue, not from the
queue index delta, which shuffle makes meaningless. A natural end-of-track
transition, a manual Next, and starting a new queue all count as forward.
Reduced Motion collapses the slide to an immediate swap.

## 10. Interaction grammar

- Hover feedback begins within 80 ms and does not move adjacent layout.
- Press feedback is immediate; release animations use the existing light desktop spring.
- Tooltips appear after roughly 200 ms and leave immediately, so walking a row of transport controls reads them at the speed of the pointer. They are the app's own, not the OS `title` chrome, and every control that shows one also carries an `aria-label`. A tooltip names the action the press will perform ("Enable repeat"), not the state the control is in.
- Right-click never changes playback merely to show a menu.
- Destructive actions require explicit menu copy; deleting a downloaded file is distinct from removing an item from a playlist.
- Toasts confirm asynchronous mutations and include Undo when the operation is safely reversible.
- Focus rings are always visible for keyboard navigation and use Splynt green with sufficient contrast.

Default shortcuts:

| Action | macOS | Windows/Linux |
| --- | --- | --- |
| Play/pause | Space | Space |
| Search | Cmd+K | Ctrl+K |
| Back/forward | Cmd+[ / Cmd+] | Alt+Left / Alt+Right |
| Volume | Cmd+Up / Cmd+Down | Ctrl+Up / Ctrl+Down |
| Next/previous | Cmd+Right / Cmd+Left | Ctrl+Right / Ctrl+Left |
| Queue | Cmd+Shift+Q | Ctrl+Shift+Q |
| Preferences | Cmd+, | Ctrl+, |
| Full player | Cmd+Shift+F | Ctrl+Shift+F |
| Fullscreen window | Ctrl+Cmd+F | F11 |
| Quit | Cmd+Q | Alt+F4 / Ctrl+Q |

Shortcuts must not fire while an editable field owns the same keystroke.

## 11. Splynt-specific source behavior

IDs matching `ext-{provider}-{type}-{id}` are external and volatile. Apply the shared external-source parser at the data boundary.

- External artwork gets the existing small, low-opacity cloud/link glyph.
- Detail metadata appends ` · {Provider}` to the quality label.
- Native content has no extra marker.
- Provider playlists use the ID type segment, never title heuristics.
- A native result wins over a matching external result, but only when current catalogue evidence supports ownership.
- Pending likes for external tracks use stable title+artist+album metadata, not the volatile ID.

These rules are product behavior and must not be reimplemented independently in individual React components.

## 12. Offline and degraded states

The desktop client keeps metadata, artwork, queue, history, and user-approved audio downloads profile-scoped. Offline mode makes no network requests. Cached and downloaded files remain playable; unavailable rows render honestly rather than disappearing.

Downloads survive process restarts, expose progress, support pause/resume where the server permits it, and never perform file I/O on the UI thread. Storage limits, quality, Wi-Fi policy, and cache clearing live in Settings.

An unavailable server does not sign the user out. Authentication failures alone may return the user to connection setup.

## 13. Accessibility

- Complete keyboard operation is a release requirement.
- Controls have accessible names, states, and shortcuts.
- Text respects the OS scale up to 200% without hiding primary transport.
- Reduced Motion removes parallax/lift and substitutes fades while retaining state clarity.
- High Contrast increases separators and secondary-ink contrast.
- Color is never the only indicator of playing, selected, downloaded, external, or failed state.

## 14. Platform integration

Shared behavior is identical across platforms; conventions differ only where users expect them.

- **macOS:** menu bar commands, traffic-light inset, Keychain, MediaPlayer/Now Playing, notarized DMG.
- **Windows:** system media transport controls, Windows Credential Manager, taskbar thumbnail controls where supported, signed installer.
- **Linux:** MPRIS, Secret Service/libsecret, XDG directories, Wayland and X11, AppImage plus Debian package for the initial beta.

The first release supports Apple Silicon and x86-64 macOS, x86-64 Windows, and x86-64 Linux beta. Linux ARM64 is a later release target.

## 15. Release acceptance

Desktop v1 is complete only when all of the following pass on real macOS and Windows hardware and the Linux test matrix:

- connect, reconnect, switch, forget, and restore multiple server profiles;
- browse, search, open, like, playlist-mutate, and play native and external content;
- queue, shuffle, repeat, seek, volume, media keys, and crossfade invariants;
- handoff in both directions with iOS and tvOS, plus remote transport control;
- network loss, server loss, authentication failure, cold external-track playback, and relaunch recovery;
- keyboard-only navigation and screen-reader smoke tests;
- signed installation, update, downgrade refusal, and clean uninstall without deleting downloaded music unless requested.

## Additions on 2026-09-15

- The library sidebar has no silent cap. It renders 600 rows at a time with a
  Show all row, and playing a collection counts as recent.
- Track tables sort by clicking column headers (ascending, descending, then
  original order), filter by title, artist, and album, and offer a compact
  density. Playback follows the shown order, and playlist drag reorder is off
  while a sort or filter is active.
- Search keeps its filter chip while typing and resets it only for a new or
  cleared query.
- A collection that fails to load offers Retry. Transfers can cancel the rest
  of a batch without deleting finished files, and a paused track is not
  listed as failed.
- The expanded player's About the artist card shows the server's biography
  from `getArtistInfo2`, or is hidden when there is none.
- The tray opens the window on left-click and has Play/Pause, Next, and
  Previous. Linux gets MPRIS. New shortcuts: Cmd/Ctrl+S shuffle, Cmd/Ctrl+R
  repeat, Cmd/Ctrl+Shift+Down mute, Alt/Option+Shift+B like. None fire in a
  text field.
