import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import type { ConnectPeer, ConnectPlayback, ConnectQueue, SongSummary } from "./types";

const { invoke, listeners } = vi.hoisted(() => ({
  invoke: vi.fn(),
  listeners: new Map<string, () => void>(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke }));
// The transport wakes the webview with these events; the tests fire them to
// deliver a new snapshot at once instead of waiting for the next poll.
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn((event: string, handler: () => void) => {
    listeners.set(event, handler);
    return Promise.resolve(() => listeners.delete(event));
  }),
}));

const APPLE_EPOCH_MS = 978_307_200_000;
const appleNow = () => (Date.now() - APPLE_EPOCH_MS) / 1000;

const songs: Record<string, SongSummary> = {
  s1: { id: "s1", title: "Phone Song", artist: "Artist", album: "Album", albumId: "album-1", duration: 200, coverArt: "c" },
  s2: { id: "s2", title: "Second Song", artist: "Artist", album: "Album", albumId: "album-1", duration: 210, coverArt: "c" },
  s3: { id: "s3", title: "Third Song", artist: "Artist", album: "Album", albumId: "album-1", duration: 220, coverArt: "c" },
};
const album = { id: "album-1", title: "Server Album", artist: "Server Artist", year: 2026 };
const connected = { server: { displayHost: "music.example.test", username: "elijah", serverType: "navidrome" }, albums: [album] };

type Sent = { peerId: string; command: { name: string; value?: number; queueItem?: { index: number; trackID: string }; tracks?: { trackIDs: string[] } } };

/// A fake transport: the test edits `state`, then calls `deliver` to have the
/// shell read it the way a frame from the phone would arrive.
function harness(playback: Partial<ConnectPlayback> | undefined, extension = true) {
  const state: { peers: ConnectPeer[]; queues: Record<string, ConnectQueue> } = { peers: [], queues: {} };
  const setPhone = (next: Partial<ConnectPlayback>) => {
    const base: ConnectPlayback = {
      trackID: "s1", title: "Phone Song", artist: "Artist", album: "Album", coverArtID: "c",
      isPlaying: true, position: 30, duration: 200,
      ...(extension ? { shuffle: false, repeatMode: "off", volume: 0.7, queueRevision: 1, queueIndex: 0, queueLength: 3, contextLabel: "Album" } : {}),
    };
    state.peers = [{ id: "phone", name: "Elijah's iPhone", platform: "iPhone", updatedAt: appleNow(), playback: { ...base, ...next } }];
  };
  if (playback) setPhone(playback);
  const sent: Sent[] = [];
  invoke.mockImplementation((command: string, args?: Record<string, unknown>) => {
    if (command === "restore_session") return Promise.resolve(connected);
    if (command === "load_home") return Promise.resolve({ newest: [album], recent: [], frequent: [], random: [], genres: [] });
    if (command === "load_library") return Promise.resolve({ albums: [album], artists: [], playlists: [], starredSongs: [], starredAlbums: [], starredArtists: [] });
    if (command === "get_album") return Promise.resolve({ ...album, songs: [songs.s2, songs.s3] });
    if (command === "get_play_queue") return Promise.resolve(null);
    if (command === "media_url") return Promise.resolve("splice-media://localhost/media");
    if (command === "get_songs_by_ids") return Promise.resolve((args?.ids as string[]).map((id) => songs[id]).filter(Boolean));
    if (command === "connect_snapshot") {
      return Promise.resolve({ isAvailable: true, localDeviceId: "this-mac", peers: state.peers, commands: [], queues: state.queues });
    }
    if (command === "send_connect_command") { sent.push(args as Sent); return Promise.resolve(undefined); }
    return Promise.resolve(undefined);
  });
  const deliver = async () => {
    await act(async () => {
      listeners.get("connect-peers-changed")?.();
      await new Promise((resolve) => setTimeout(resolve, 30));
    });
  };
  const named = (name: string) => sent.filter((entry) => entry.command.name === name);
  return { state, setPhone, sent, deliver, named };
}

function calls(name: string) {
  return invoke.mock.calls.filter(([command]) => command === name).map(([, args]) => args as Record<string, unknown>);
}

async function openShell() {
  render(<App />);
  await screen.findByRole("navigation", { name: "Main navigation" });
  return screen.getByLabelText("Player");
}

describe("Splynt Connect follow mode", () => {
  beforeEach(() => {
    localStorage.clear();
    invoke.mockReset();
    listeners.clear();
    vi.mocked(HTMLMediaElement.prototype.play).mockClear();
  });
  afterEach(() => vi.restoreAllMocks());

  it("follows the phone on its own when this computer is idle, without making a sound", async () => {
    const { state, deliver, named } = harness({});
    const bar = await openShell();

    expect(await within(bar).findByText("Phone Song")).toBeInTheDocument();
    expect(within(bar).getByRole("button", { name: "Playing on Elijah's iPhone" })).toBeInTheDocument();
    expect(within(bar).getByRole("button", { name: "Pause" })).toBeInTheDocument();
    await waitFor(() => expect(named("queueRequest").map((entry) => entry.peerId)).toEqual(["phone"]));

    state.queues.phone = { revision: 1, offset: 0, index: 0, trackIDs: ["s1", "s2", "s3"], contextLabel: "Album" };
    await deliver();
    fireEvent.click(within(bar).getByRole("button", { name: "Queue" }));
    const queue = await screen.findByRole("complementary", { name: "Queue" });
    expect(await within(queue).findByText("Third Song")).toBeInTheDocument();

    // Following publishes idle playback naming the phone, and nothing here
    // ever asked for a stream or touched an audio element.
    await waitFor(() => {
      const last = calls("publish_connect_playback").at(-1) as { playback: ConnectPlayback; commitment: { controllingPeerID?: string } };
      expect(last.commitment.controllingPeerID).toBe("phone");
      expect(last.playback.isPlaying).toBe(false);
    });
    expect(calls("media_url").some((args) => args.kind === "stream")).toBe(false);
    expect(HTMLMediaElement.prototype.play).not.toHaveBeenCalled();
  });

  it("does not follow a phone that starts while this computer is playing", async () => {
    vi.mocked(HTMLMediaElement.prototype.play).mockImplementation(function (this: HTMLMediaElement) {
      this.dispatchEvent(new Event("play"));
      return Promise.resolve();
    });
    const { setPhone, deliver, named } = harness(undefined);
    const bar = await openShell();
    fireEvent.click(await screen.findByRole("button", { name: "Play Server Album" }));
    expect(await within(bar).findByText("Second Song")).toBeInTheDocument();
    await waitFor(() => expect(within(bar).getByRole("button", { name: "Pause" })).toBeInTheDocument());

    setPhone({});
    await deliver();
    await deliver();
    expect(within(bar).queryByRole("button", { name: /Playing on/ })).not.toBeInTheDocument();
    expect(within(bar).getByText("Second Song")).toBeInTheDocument();
    expect(named("queueRequest")).toEqual([]);
  });

  it("follows the phone to its next track", async () => {
    const { state, setPhone, deliver } = harness({});
    const bar = await openShell();
    state.queues.phone = { revision: 1, offset: 0, index: 0, trackIDs: ["s1", "s2", "s3"] };
    await deliver();
    expect(await within(bar).findByText("Phone Song")).toBeInTheDocument();

    setPhone({ trackID: "s2", title: "Second Song", queueIndex: 1, queueRevision: 2, position: 0 });
    state.queues.phone = { revision: 2, offset: 0, index: 1, trackIDs: ["s1", "s2", "s3"] };
    await deliver();
    expect(await within(bar).findByText("Second Song", {}, { timeout: 500 })).toBeInTheDocument();
  });

  it("ignores frames naming the old track for two and a half seconds after a skip", async () => {
    const { state, deliver, named } = harness({});
    const bar = await openShell();
    state.queues.phone = { revision: 1, offset: 0, index: 0, trackIDs: ["s1", "s2", "s3"] };
    await deliver();
    fireEvent.click(within(bar).getByRole("button", { name: "Queue" }));
    const queue = await screen.findByRole("complementary", { name: "Queue" });
    fireEvent.click(await within(queue).findByText("Third Song"));
    expect(named("skipTo").map((entry) => entry.command.queueItem)).toEqual([{ index: 2, trackID: "s3" }]);
    expect(within(bar).getByText("Third Song")).toBeInTheDocument();

    // The phone's next frame was written before it moved.
    await deliver();
    expect(within(bar).getByText("Third Song")).toBeInTheDocument();

    // After the window, a frame still naming the old track is the truth.
    const now = Date.now();
    vi.spyOn(Date, "now").mockReturnValue(now + 3_000);
    await deliver();
    expect(await within(bar).findByText("Phone Song")).toBeInTheDocument();
  });

  it("never saves the play queue while following", async () => {
    const { state, deliver } = harness({});
    const bar = await openShell();
    state.queues.phone = { revision: 1, offset: 0, index: 0, trackIDs: ["s1", "s2", "s3"] };
    await deliver();
    expect(await within(bar).findByText("Phone Song")).toBeInTheDocument();
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 900)); });
    document.dispatchEvent(new Event("visibilitychange"));
    expect(calls("save_play_queue")).toEqual([]);
  });

  it("sends every control to the phone", async () => {
    const { state, deliver, named, sent } = harness({});
    const bar = await openShell();
    state.queues.phone = { revision: 1, offset: 0, index: 0, trackIDs: ["s1", "s2", "s3"] };
    await deliver();
    expect(await within(bar).findByText("Phone Song")).toBeInTheDocument();
    sent.length = 0;

    fireEvent.click(within(bar).getByRole("button", { name: "Pause" }));
    fireEvent.click(within(bar).getByRole("button", { name: "Next" }));
    fireEvent.click(within(bar).getByRole("button", { name: "Previous" }));
    fireEvent.click(within(bar).getByRole("button", { name: "Shuffle" }));
    fireEvent.click(within(bar).getByRole("button", { name: "Repeat off" }));
    fireEvent.input(within(bar).getByRole("slider", { name: "Volume" }), { target: { value: "0.25" } });
    fireEvent.input(within(bar).getByRole("slider", { name: "Playback position" }), { target: { value: "90" } });
    expect(sent.map((entry) => entry.command.name)).toEqual(["pause", "next", "previous", "setShuffle", "setRepeat", "setVolume", "seek"]);
    expect(sent.every((entry) => entry.peerId === "phone")).toBe(true);
    expect(named("setShuffle")[0].command.value).toBe(1);
    expect(named("setRepeat")[0].command.value).toBe(1);
    expect(named("setVolume")[0].command.value).toBe(0.25);
    expect(named("seek")[0].command.value).toBe(90);

    // Add to queue, and starting an album, both go to the phone.
    const card = screen.getByRole("button", { name: "Play Server Album" }).closest("article") as HTMLElement;
    fireEvent.contextMenu(card);
    fireEvent.click(await screen.findByRole("menuitem", { name: "Add to queue" }));
    await waitFor(() => expect(named("enqueue").map((entry) => entry.command.tracks?.trackIDs)).toEqual([["s2", "s3"]]));
    fireEvent.click(screen.getByRole("button", { name: "Play Server Album" }));
    await waitFor(() => expect(named("handoff")).toHaveLength(1));
    expect(calls("media_url").some((args) => args.kind === "stream")).toBe(false);
  });

  it("disables shuffle, repeat and volume for a phone on an older Splynt", async () => {
    const { deliver, named } = harness({}, false);
    const bar = await openShell();
    await deliver();
    expect(await within(bar).findByText("Phone Song")).toBeInTheDocument();
    for (const name of ["Shuffle", "Repeat off", "Mute"]) {
      const control = within(bar).getByRole("button", { name });
      expect(control).toBeDisabled();
      expect(control).toHaveAttribute("title", "Update Splynt on Elijah's iPhone to change this from here");
    }
    expect(within(bar).getByRole("slider", { name: "Volume" })).toBeDisabled();
    expect(named("queueRequest")).toEqual([]);
  });

  it("moves playback here from the phone", async () => {
    const { state, deliver, named } = harness({});
    const bar = await openShell();
    state.queues.phone = { revision: 1, offset: 0, index: 0, trackIDs: ["s1", "s2", "s3"] };
    await deliver();
    expect(await within(bar).findByText("Phone Song")).toBeInTheDocument();
    fireEvent.click(within(bar).getByRole("button", { name: "Playing on Elijah's iPhone" }));
    const devices = await screen.findByRole("complementary", { name: "Devices" });
    fireEvent.click(within(devices).getByRole("button", { name: "This computer" }));

    await waitFor(() => expect(named("pause").map((entry) => entry.peerId)).toEqual(["phone"]));
    await waitFor(() => expect(within(bar).queryByRole("button", { name: /Playing on/ })).not.toBeInTheDocument());
    // The same track, loaded here and playing.
    await waitFor(() => expect(calls("media_url").some((args) => args.kind === "stream" && args.id === "s1")).toBe(true));
    expect(HTMLMediaElement.prototype.play).toHaveBeenCalled();
    expect(within(bar).getByText("Phone Song")).toBeInTheDocument();
  });
});
