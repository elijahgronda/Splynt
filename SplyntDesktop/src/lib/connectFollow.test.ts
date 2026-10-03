import { describe, expect, it } from "vitest";
// The file the iOS Swift tests and this repository's Rust tests read too.
import fixtures from "../../../docs/connect/wire-fixtures.json";
import type { ConnectPeer } from "../types";
import {
  decideFollow, followPeers, hasRemoteControl, notePeerActivity, projectedPeerPosition, repeatFromWire,
  repeatToWire, trackGuardIgnores, type FollowDecision, type FollowLocal, type FollowPeer,
} from "./connectFollow";

type Scenario = {
  name: string;
  local: FollowLocal;
  peers: FollowPeer[];
  dropped?: { id: string; until: number };
  now: number;
  expect: FollowDecision;
};

const APPLE_EPOCH_MS = 978_307_200_000;

function peer(id: string, isPlaying: boolean, trackID: string | undefined = "t"): ConnectPeer {
  return { id, name: id, platform: "iPhone", updatedAt: 0, playback: { trackID, isPlaying, position: 0, duration: 200 } };
}

describe("Connect follow rules", () => {
  it.each((fixtures.follow as Scenario[]).map((scenario) => [scenario.name, scenario] as const))("%s", (_, scenario) => {
    expect(decideFollow(scenario.local, scenario.peers, scenario.dropped, scenario.now)).toEqual(scenario.expect);
  });

  it("tells an updated peer from a v1 one the same way the fixtures do", () => {
    for (const fixture of fixtures.peers) {
      expect(hasRemoteControl((fixture.frame as ConnectPeer).playback), fixture.name).toBe(fixture.extension);
    }
  });

  it("records when each peer started and stopped playing", () => {
    let activity = notePeerActivity({}, [peer("phone", true)], 10);
    activity = notePeerActivity(activity, [peer("phone", true)], 20);
    expect(activity.phone.startedAt).toBe(10);
    activity = notePeerActivity(activity, [peer("phone", false)], 30);
    expect(activity.phone.stoppedAt).toBe(30);
    activity = notePeerActivity(activity, [peer("phone", true)], 40);
    expect(followPeers([peer("phone", true)], activity)[0]).toMatchObject({ playStartedAt: 40, stoppedAt: 30 });
    activity = notePeerActivity(activity, [], 50);
    expect(activity.phone).toBeUndefined();
  });

  it("holds the old track for two and a half seconds after a skip", () => {
    const guard = { oldTrackID: "old", until: 2_500 };
    expect(trackGuardIgnores(guard, "old", 1_000)).toBe(true);
    expect(trackGuardIgnores(guard, "new", 1_000)).toBe(false);
    expect(trackGuardIgnores(guard, "old", 2_600)).toBe(false);
  });

  it("projects a playing position from the transport's receive stamp", () => {
    const received = 800_000_000;
    const now = APPLE_EPOCH_MS + (received + 1.5) * 1000;
    const playback = { trackID: "t", isPlaying: true, position: 40, duration: 42 };
    expect(projectedPeerPosition(playback, received, now)).toBeCloseTo(41.5);
    expect(projectedPeerPosition(playback, received, now + 9_000)).toBe(42);
    expect(projectedPeerPosition({ ...playback, isPlaying: false }, received, now + 9_000)).toBe(40);
  });

  it("maps repeat modes to and from the wire", () => {
    expect(repeatToWire("off")).toBe(0);
    expect(repeatToWire("all")).toBe(1);
    expect(repeatToWire("one")).toBe(2);
    expect(repeatFromWire("one")).toBe("one");
    expect(repeatFromWire("shuffle-repeat")).toBe("off");
    expect(repeatFromWire(undefined)).toBeUndefined();
  });
});
