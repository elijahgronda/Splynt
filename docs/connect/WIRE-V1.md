# Splynt Connect wire protocol v1

This document records the implemented LAN transport. It does not define the
final account-level playback session or Play Everywhere product model. See
[README.md](README.md) for those semantics.

Status: compatibility contract extracted from the iOS client's Connect implementation. The desktop implementation must match this contract byte-for-byte before protocol v2 work begins.

## Purpose and boundary

Splynt Connect discovers compatible clients on the same local network, publishes playback state, sends absolute transport commands, and hands off queues by catalogue ID. It never relays audio. A handoff target resolves the IDs against the same authenticated server and opens its own stream.

## Discovery and transport

- DNS-SD service type: `_spliceconnect._tcp.local.`
- Service name: `Splynt-{deviceID prefix}`, using the first eight device-ID characters with hyphens removed.
- TCP listener port: dynamically assigned by the OS and advertised through DNS-SD.
- Transport: plaintext TCP.
- Frame: one UTF-8 JSON object followed by byte `0x0A`.
- Receiver chunk maximum: 65,536 bytes.
- Buffered incomplete-frame limit: 262,144 bytes. A larger unterminated buffer is discarded.
- On connection readiness, both peers immediately send their current state.
- State heartbeat: every 3 seconds.
- Peer expiry: 12 seconds after the last valid state received locally.

DNS-SD results must be retained and retried. Discovery implementations do not necessarily emit an unchanged service again after a TCP connection attempt races a peer's listener.

## Shared authentication fingerprint

v1 peers connect only when they independently compute the same lowercase SHA-256 hex string.

1. Trim whitespace/newlines from the server value.
2. If it has no scheme, parse it as `//{value}`.
3. Lowercase the host.
4. Preserve the explicit port as `:{port}`.
5. Preserve the URL path after removing trailing `/` characters.
6. If that path ends case-insensitively in `/rest`, remove the final five characters.
7. Lowercase the remaining path.
8. Build `{host}{port}{path}|{lowercased username}|{raw password}`.
9. Compute SHA-256 and lowercase hex encode it.

The fingerprint is included as `authentication` on every frame. A frame with a nonmatching fingerprint is ignored without a response.

The authentication fingerprint functions as a replayable bearer secret on the LAN. Implement v1 for compatibility, keep it out of logs and diagnostics, and do not expose it to the React layer. Protocol v2 will replace it with explicit device pairing and an encrypted session.

## Wire messages

Top-level object:

```json
{
  "kind": "state | command",
  "authentication": "lowercase SHA-256 hex",
  "peer": {},
  "command": {}
}
```

Swift's synthesized optional encoding omits `peer` or `command` when its value is absent. Decoders should accept an explicit `null` for interoperability but encoders must emit the existing omission form.

Every key comes from a Swift property name, because Swift synthesizes its coding
keys from them and this file is extracted from that source. So an identifier key
is spelled `trackID`, `leaderID`, `sessionID`, `deviceID`, never `trackId`. This
is worth stating rather than leaving to the examples: a camelCase serializer
convention on another platform produces the other spelling, which decodes on
that platform and nowhere else. The desktop shipped that way from its first
commit until 2026-08-30, and neither side's tests noticed, because each asserted
the shape it already produced.

### State

```json
{
  "kind": "state",
  "authentication": "…",
  "peer": {
    "id": "persistent device UUID",
    "name": "Living Room TV",
    "platform": "tvOS",
    "playback": {
      "trackID": "catalogue-track-id",
      "title": "Track title",
      "artist": "Artist",
      "album": "Album",
      "coverArtID": "cover-art-id",
      "isPlaying": true,
      "position": 42.25,
      "duration": 218.0
    },
    "updatedAt": 808315200.0
  }
}
```

`trackID`, title, artist, album, and coverArtID are nullable/omitted for idle state. `updatedAt` uses Swift `JSONEncoder`'s default deferred Date representation: seconds since the Apple reference date, 2001-01-01T00:00:00Z. Receivers replace it with their own current time before freshness decisions, so clocks never need to agree.

Receiving a peer with the local device ID closes that self-connection. When two sockets represent one peer, the most recently associated ready connection becomes the command route.

### Commands

Commands use a manually encoded tagged object:

```json
{ "name": "play" }
{ "name": "pause" }
{ "name": "toggle" }
{ "name": "previous" }
{ "name": "next" }
{ "name": "seek", "value": 42.25 }
```

Handoff:

```json
{
  "name": "handoff",
  "handoff": {
    "trackIDs": ["id-1", "id-2", "id-3"],
    "currentTrackID": "id-2",
    "position": 42.25,
    "isPlaying": true
  }
}
```

Command frame example:

```json
{
  "kind": "command",
  "authentication": "…",
  "command": { "name": "pause" }
}
```

After handling any valid command, a peer broadcasts its resulting current state.

## Group-session command extension

Group playback extends the v1 command envelope without changing discovery,
authentication, framing, or the original command encodings. A leader sends a
complete catalogue-ID handoff when the listener explicitly chooses **Play on
every Splynt device**, then publishes an absolute playback clock about once per
second. This is a v1-compatible application command extension, not protocol v2;
pairing and encrypted transport remain future v2 work.

```json
{
  "name": "groupJoin",
  "groupJoin": {
    "group": {
      "id": "group UUID",
      "leaderID": "leader device UUID",
      "trackID": "catalogue-track-id",
      "position": 42.25,
      "isPlaying": true,
      "sentAt": 1800000000000
    },
    "handoff": {
      "trackIDs": ["id-1", "id-2", "id-3"],
      "currentTrackID": "id-2",
      "position": 42.25,
      "isPlaying": true
    }
  }
}
```

`groupSync` carries the same `group` object. `groupLeave` carries the final
`group` object so a follower only leaves the matching session. Unlike peer
`updatedAt`, `sentAt` is Unix milliseconds on every platform. A playing
follower projects the leader position by elapsed transit time, ignores jitter
up to 1.25 seconds, and seeks when drift exceeds that threshold. Transit
compensation is capped at two seconds so wall-clock skew cannot cause a large
seek. When the leader changes tracks it sends a fresh `groupJoin` with the
latest bounded queue before returning to lightweight `groupSync` frames.

Transferred queues retain occurrence order and legitimate duplicates, are
capped at 1,000 entries, and are resolved in bounded batches rather than with
an unbounded burst of `getSong` requests. A receiver resolves and starts the
active track first, then fills the remaining queue without restarting audio.
Clients that do not implement this
extension continue to interoperate with all original v1 state, transport, seek,
and handoff frames; they ignore an unknown group command frame.

## Session commitment and acknowledged membership

v1 had no way for a device to say what it was already doing, and no way to say
yes. Both gaps showed up in one evening's device logs on 2026-08-30: a leader
recorded `members: 1` for a session the other device never joined, and the two
devices spent nine seconds each acting as the other's remote.

Neither is a coding slip. `members` was built from whether the local socket
write succeeded, which is the only signal the protocol offered, and a
stale-but-open TCP connection accepts writes long after the peer behind it has
stopped reading. And nothing on the wire described commitment, so a device
choosing what to control had only "is it playing" to go on.

Both additions are v1-compatible: an old client ignores the unknown command
names and omits the new fields, which decode as absent.

### Commitment on every state frame

`peer.commitment` is optional and additive:

```json
{
  "commitment": {
    "sessionID": "session uuid",
    "leaderID": "leader device uuid",
    "revision": 4,
    "controllingPeerID": "device this one is driving"
  }
}
```

`sessionID` and `leaderID` are present while the device renders a session.
`controllingPeerID` is present while it drives another device's audio instead
of its own. A peer that publishes no commitment at all is a v1 peer and is
treated as available, not as busy.

Two rules read this field:

- **An output belongs to one session at a time.** A leader invites only devices
  whose `sessionID` is absent. A controller's own audio remains available.
  Joining or starting a session ends remote control first. A device already
  in a different session declines rather than switching.
- **A device driving someone else is not an idle player.** Adoption and
  "playing now" checks use "is playing its own audio", which excludes a peer
  publishing `controllingPeerID`. Without that exclusion two devices can each
  adopt the other and neither can leave.

### Join acknowledgement

```json
{ "name": "groupAccept",  "groupReply": { "sessionID": "…", "deviceID": "…", "revision": 4 } }
{ "name": "groupDecline", "groupReply": { "sessionID": "…", "deviceID": "…", "revision": 4,
                                          "reason": "in another session" } }
```

A follower sends `groupAccept` only after it is actually rendering the session:
the track is resolved and playback has been started at the joined position.
Every path that refuses sends `groupDecline` with a short, stable reason meant
for the log rather than the listener. The current reasons are
`in another session`, `no server connection`, and `track unresolved`.

A leader sends its invitations, then waits up to **3 seconds** for answers. Its
members are the devices that accepted. Devices that declined and devices that
never answered are counted and logged separately, because they are different
failures: a decline is a device working correctly, and silence is the case v1
could not tell apart from success. A session that nobody accepts reports that
instead of starting.

The wait is bounded because a follower has real work to do first. It resolves
the current track against the server before it can honestly claim to have
joined, and a cold track takes about a second.

### Session revision

`group.revision` is optional, monotonic within a session, and advanced by the
leader on every accepted change. Today that means session start and each track
change; the per-second sync frames in between carry the revision they belong
to, unchanged.

A follower applies a frame whose revision is at or above the one it has already
applied, and drops anything older, so a controller that has fallen behind
cannot walk the session backwards. Absent is read as `0`, which is what keeps
an un-updated leader's frames acceptable to an updated follower.

This is the smallest useful piece of the revision-based conflict handling in
[README.md](README.md). It is not yet a full session object with transferable
authority, and it does not survive a restart.

## Clock probe extension

Group playback needs the two devices to agree on what time it is. v1 had no
such exchange: a follower subtracted the leader's `sentAt` from its own wall
clock and called the difference transit. Two devices half a second apart
measured half a second of phantom drift on every frame, and a follower whose
clock ran behind measured none at all and stayed late. The cap of two seconds
on that term was a limit on the damage, not a correction.

Like the group commands, this extends the v1 command envelope without touching
discovery, authentication, framing, or any existing encoding. A client that
does not implement it ignores the unknown command name and keeps working; it
just never has an offset, and every projection falls back to the v1 wall-clock
form.

```json
{ "name": "timePing", "time": { "id": "probe id", "t1": 1800000000000.0 } }
{ "name": "timePong", "time": { "id": "probe id", "t1": 1800000000000.0,
                                "t2": 1800000000012.0, "t3": 1800000000013.0 } }
```

All four timestamps are Unix milliseconds, as `sentAt` is and unlike peer
`updatedAt`. `t1` is the requester's clock as it wrote the request, `t2` the
responder's as it read it, `t3` the responder's as it wrote the reply, and `t4`
the requester's as the reply lands, which never travels. `t2` and `t3` are
omitted on the request rather than sent as null.

Two rules make the measurement mean anything:

- The transport answers a `timePing` itself. It must not reach the player, the
  webview, or any command queue, and it must not be followed by the state
  broadcast that every other command ends with. A probe changes no state, and
  at one per peer per heartbeat that broadcast would double the state traffic.
- `t1` is stamped as the frame is written and `t4` as it arrives, before the
  JSON is parsed. Anything the responder spends between `t2` and `t3` cancels
  in the estimator, so a busy peer still answers accurately, but time a client
  hides inside its own stamps does not.

```
offset    = ((t2 - t1) + (t3 - t4)) / 2
roundTrip = (t4 - t1) - (t3 - t2)
```

`offset` is added to the local clock to read the peer's. A negative or
non-finite round trip describes an exchange that cannot have happened, and that
sample is discarded. Clients keep the last 8 samples per peer over a 30 s
window and believe the one with the lowest round trip: queueing delay only ever
adds error, so the fastest exchange is the most accurate. Averaging would fold
every slow sample back in.

Probes run continuously, one per peer on the existing 3 s heartbeat, not only
during a group session. A session that starts then already has an offset
instead of spending its first seconds measuring one.

## Drift correction

The correction policy is not on the wire, but every client has to reach the
same decision from the same numbers or a group is only as synchronised as its
worst follower. Drift is the follower's position minus the leader's projected
position, so positive means it is ahead.

| Drift | Action |
| --- | --- |
| up to 8 ms | none |
| 8 ms to 400 ms | play at 1 ∓ 2% for `drift / 0.02` seconds, then return to 1.0 |
| over 400 ms | absolute seek |

v1 ignored everything under 1.25 s and then seeked. That window was never a
tolerance: a seek is audible, so correcting 40 ms with one costs more than the
drift does. A 2% rate change through a pitch-preserving time-domain algorithm
is not audible, which is what lets the deadband fall below the product's 10 ms
steady-state target. 400 ms is the largest gap 2% closes within 20 s, and
converging for longer than that leaves the room out of step for most of a
verse.

A convergence carries its own deadline rather than waiting for the next sync
frame to end it. A leader that goes quiet mid-correction would otherwise leave
a follower at 98% indefinitely, walking away from the room at exactly the rate
meant to catch it up.

## Desktop identity

- Persist one random UUID per application installation in OS application data.
- The default name is the OS computer name, editable in Settings.
- Platform strings emitted by desktop clients are `macOS`, `Windows`, or `Linux`.
- Reconfiguring with a different server/account fingerprint stops discovery, closes every socket, clears peers, and starts a fresh listener/browser.

## Compatibility fixtures

Before enabling Connect in the UI, automated tests must cover:

- server normalization with a scheme, without a scheme, with a port, with a reverse-proxy path, with trailing slashes, and with `/rest`;
- exact SHA-256 output for a fixed non-secret test credential;
- exact frames for idle state, playing state, every command, seek, and handoff;
- exact frames for group join, sync, and leave, including Unix-millisecond `sentAt`;
- exact frames for a clock probe request and reply, including the omission of
  `t2`/`t3` on the request, and the offset the estimator produces for a known
  exchange;
- exact frames for group accept and decline, a peer frame with no `commitment`,
  and a group frame with no `revision`, since both must still decode;
- fragmented frames, multiple frames in one chunk, invalid JSON, authentication mismatch, and oversized unterminated data;
- peer freshness based on receiver time;
- iOS ↔ macOS, iOS ↔ Windows, tvOS ↔ desktop, and desktop ↔ desktop discovery and command exchange.

## Protocol v2 direction

v2 is deliberately not part of the initial compatibility implementation. Its migration design must provide:

- explicit user-approved pairing;
- random per-device credentials unrelated to the server password;
- encrypted and mutually authenticated sessions;
- version negotiation;
- revocation in Settings;
- dual-stack discovery so v1 mobile/tvOS releases continue to work during migration.
