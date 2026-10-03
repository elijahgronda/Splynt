# Connect product semantics

This document defines how playback sessions, handoff, and Play Everywhere
behave. It is independent of the current wire format. See [WIRE-V1.md](WIRE-V1.md)
for the implemented local transport.

## Session model

An account can own multiple playback sessions. Each session has one queue,
current item, timeline, play state, mode set, and output set. A controller must
show which session it is controlling. An output belongs to one session at a
time.

At minimum, a durable session snapshot contains:

- a stable session identifier and account identity;
- queue identity and ordered items;
- current item and confirmed position;
- play or pause state and playback modes;
- selected outputs and current coordinator;
- a monotonic revision or equivalent conflict-ordering value.

Session state is durable. Closing the final participating app records Pause and
does not transfer audio to an idle client. The next explicit Play resumes that
session unless the listener starts separate playback.

## Authority and updates

Controllers send commands to a session, not directly to an assumed player.
Every accepted state change advances the session revision. Clients reconcile
against the canonical revision so stale controllers cannot overwrite newer
queue or timeline state.

The exact authority algorithm remains an implementation decision. It must allow
coordination to transfer among outputs that are already playing, survive a
coordinator departure, and avoid automatically waking an idle output.

## Active device

Whichever Splynt device is playing is the active device. Every other Splynt
app on the same account and network shows that playback as if it were local:
the track, a progress bar that moves with it, play state, shuffle, repeat,
volume and the queue. Every control on those apps drives the active device,
and none of them plays audio. A strip under the player names the device, as
Spotify's "Playing on" strip does.

The device picker lists this device first. Picking it moves playback here
with the same queue, track and position, and pauses the device that was
playing. The exact follow rules, including when an idle app starts following
and when it lets go, are in [WIRE-V1.md](WIRE-V1.md#following-the-active-device).

Only the device rendering audio saves the play queue to the server. A device
that is following, or that holds a queue it has not played, leaves the
server's copy alone, because that copy belongs to whoever played last.

## Handoff

Handoff moves playback to a selected output while retaining session identity,
queue, item, modes, and position. The destination buffers before the switch.
After confirmation, the previous output leaves the session's output set unless
the listener selected Play Everywhere.

## Play Everywhere

Play Everywhere is one session rendered by selected outputs. The picker offers
individual selection and Select all. Discovery never opts every device into
audio automatically.

Joining follows this sequence:

1. The new output resolves the current item and silently prebuffers.
2. The coordinator chooses a future start instant understood by all outputs.
3. The new output begins at that instant while existing outputs continue.
4. Ongoing clock and drift correction converge without audible hard seeks.

The strict acceptance target is no more than 10 ms steady-state skew and no more
than 20 ms during starts and track changes. An output or route that cannot meet
the target may be offered as best effort with a visible label.

If no outputs remain, the session becomes paused at the latest confirmed
position. The session remains available for later resume.

## Route calibration

Microphone calibration is explicit, optional, and tied to a route and output
set. A capable device placed at the listening position records sequential test
tones and calculates route offsets. Any relevant route change invalidates the
saved calibration.

Calibration supplements the synchronization protocol. It cannot compensate for
missing clock-offset estimation, scheduled starts, or drift control.

## Receivers

A receiver is an Output that is not a Splynt install: a UPnP renderer on the
local network, such as a network streamer or an amplifier. Splynt hands it the
queue as authenticated Subsonic stream URLs, and the receiver fetches every
song from the server itself. Splynt's own audio processing therefore does not
apply to it, custom request headers cannot reach the server, and the receiver
must be able to reach the server's address on its own.

Handoff to a receiver follows the rules above. The receiver must confirm it is
playing the current song before the iPhone stops. Because iOS suspends Splynt
once its own audio stops, only a receiver that holds the whole queue and
advances through it without Splynt is offered, which today means an OpenHome
Playlist service. When Splynt returns to the foreground, the receiver's state
is the newer one and replaces the iPhone's.

## Discovery, approval, and revocation

Version 1 discovery is LAN-only. Devices on the same account can be visible
without manual address entry. The first request to control a device or add it to
Play Everywhere requires approval. Approval creates a per-device encrypted
credential that either side can revoke.

Remote discovery and control are later work. They must preserve the same account,
session, approval, and revocation rules rather than introducing a second product
model.
