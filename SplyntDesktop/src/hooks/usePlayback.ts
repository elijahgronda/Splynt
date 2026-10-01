import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { EqualizerGraph } from "../lib/equalizerGraph";
import { flatEqualizer, type EqualizerSettings } from "../lib/equalizer";
import type { RepeatMode, SongSummary } from "../types";

type PersistedPlayback = {
  queue: SongSummary[];
  index: number;
  position: number;
  volume: number;
  shuffle: boolean;
  repeat: RepeatMode;
  contextLabel?: string;
  manualQueueCount?: number;
};

export type PlaybackOptions = {
  shuffleMode?: "fewerRepeats" | "random";
  crossfadeSeconds: number;
  equalizer?: EqualizerSettings;
  gapless: boolean;
  autoplay: boolean;
  onQueueExhausted?: (last: SongSummary) => void;
};

const STORAGE_KEY = "splice.playback.v2";
const FADE_TICK_MS = 50;

function readPersisted(storageKey: string): PersistedPlayback {
  try {
    // Migrate the single-profile v2 key once; all subsequent recovery is
    // isolated to the signed-in server/account.
    const parsed = JSON.parse(localStorage.getItem(storageKey) ?? localStorage.getItem(STORAGE_KEY) ?? "{}") as Partial<PersistedPlayback>;
    const queue = Array.isArray(parsed.queue) ? parsed.queue.slice(0, 1000) : [];
    const index = queue.length ? Math.min(Math.max(0, Number(parsed.index) || 0), queue.length - 1) : -1;
    return {
      queue,
      index,
      position: Math.max(0, Number(parsed.position) || 0),
      volume: Math.max(0, Math.min(1, Number(parsed.volume ?? 0.8))),
      shuffle: Boolean(parsed.shuffle),
      repeat: parsed.repeat === "all" || parsed.repeat === "one" ? parsed.repeat : "off",
      contextLabel: parsed.contextLabel,
      manualQueueCount: Math.max(0, Math.min(queue.length, Number(parsed.manualQueueCount) || 0)),
    };
  } catch {
    return { queue: [], index: -1, position: 0, volume: 0.8, shuffle: false, repeat: "off" };
  }
}

function shuffle<T>(items: T[]) {
  for (let index = items.length - 1; index > 0; index -= 1) {
    const swap = Math.floor(Math.random() * (index + 1));
    [items[index], items[swap]] = [items[swap], items[index]];
  }
  return items;
}

/// "Fewer repeats" is the iOS default: everything not played recently comes
/// first, each half shuffled, so a long queue stops replaying the same corner.
function shuffledIndexes(length: number, current: number, staleAt?: (index: number) => boolean) {
  const tail = Array.from({ length }, (_, index) => index).filter((index) => index !== current);
  const ordered = staleAt
    ? [...shuffle(tail.filter((index) => !staleAt(index))), ...shuffle(tail.filter(staleAt))]
    : shuffle(tail);
  return current >= 0 ? [current, ...ordered] : ordered;
}

function createElement() {
  if (typeof Audio === "undefined") return undefined;
  const element = new Audio();
  element.preload = "auto";
  // Set before any `src`, and unconditionally rather than only when the
  // equalizer is on: `crossOrigin` only takes effect on requests made after it
  // is assigned, so flipping it later would need a reload mid-track. Splynt's
  // own media proxy answers with `Access-Control-Allow-Origin: *` on both the
  // streamed and the downloaded path, so this changes nothing about what plays.
  // Without it, routing the element through Web Audio yields silence.
  element.crossOrigin = "anonymous";
  return element;
}

export function usePlayback(storageScope = "default", options: PlaybackOptions = { crossfadeSeconds: 0, gapless: true, autoplay: true }) {
  const storageKey = `${STORAGE_KEY}:${encodeURIComponent(storageScope)}`;
  const initialRef = useRef<PersistedPlayback | undefined>(undefined);
  if (!initialRef.current) initialRef.current = readPersisted(storageKey);
  const initial = initialRef.current;

  // Two elements so a natural transition can overlap or butt up against the
  // next track. Manual skips always hard cut on the active element.
  const elementsRef = useRef<Array<HTMLAudioElement | undefined>>([undefined, undefined]);
  if (!elementsRef.current[0]) elementsRef.current = [createElement(), createElement()];
  const activeSlot = useRef<0 | 1>(0);
  const audio = useCallback(() => elementsRef.current[activeSlot.current], []);
  const partner = useCallback(() => elementsRef.current[activeSlot.current === 0 ? 1 : 0], []);

  // The equalizer owns both elements, not just the active one: a crossfade has
  // two of them audible at once, and only hearing the filters on one would be
  // worse than not having them at all. The graph itself stays dormant until the
  // curve is doing something. See lib/equalizerGraph.ts.
  const equalizerRef = useRef<EqualizerGraph | undefined>(undefined);
  if (!equalizerRef.current) equalizerRef.current = new EqualizerGraph();
  useEffect(() => {
    const graph = equalizerRef.current;
    elementsRef.current.forEach((element) => graph?.register(element));
    return () => graph?.dispose();
  }, []);
  useEffect(() => {
    equalizerRef.current?.apply(options.equalizer ?? flatEqualizer);
  }, [options.equalizer]);

  const [queue, setQueue] = useState<SongSummary[]>(initial.queue);
  const [index, setIndex] = useState(initial.index);
  const [loadToken, setLoadToken] = useState(0);
  const [isPlaying, setIsPlaying] = useState(false);
  const [position, setPosition] = useState(initial.position);
  const [duration, setDuration] = useState(initial.queue[initial.index]?.duration ?? 0);
  const [volume, setVolumeState] = useState(initial.volume);
  const [shuffle, setShuffleState] = useState(initial.shuffle);
  const [repeat, setRepeat] = useState<RepeatMode>(initial.repeat);
  const [contextLabel, setContextLabel] = useState(initial.contextLabel ?? "Queue");
  const [manualQueueCount, setManualQueueCount] = useState(initial.manualQueueCount ?? 0);
  const [undoQueue, setUndoQueue] = useState<{ label: string; queue: SongSummary[]; index: number; manualQueueCount: number }>();
  const [error, setError] = useState<string>();
  const shouldAutoplay = useRef(false);
  const pendingPosition = useRef(initial.position);
  const shuffleOrder = useRef<number[]>(shuffledIndexes(initial.queue.length, initial.index));
  const shuffleCursor = useRef(0);
  const completedTrack = useRef<string | undefined>(undefined);
  const publishedSecond = useRef(-1);
  const failures = useRef(new Map<string, number>());
  const recoveryTimer = useRef<number | undefined>(undefined);
  const current = queue[index];
  const queueRef = useRef(queue);
  queueRef.current = queue;
  const indexRef = useRef(index);
  indexRef.current = index;
  const positionRef = useRef(position);
  positionRef.current = position;
  const manualQueueCountRef = useRef(manualQueueCount);
  manualQueueCountRef.current = manualQueueCount;
  const volumeRef = useRef(volume);
  volumeRef.current = volume;
  const optionsRef = useRef(options);
  optionsRef.current = options;

  // Which way the last track change moved through the queue. The UI reads this
  // to decide whether the outgoing track leaves to the left or the right; it is
  // a ref because every write is immediately followed by the `setIndex` that
  // re-renders, and shuffle makes the raw index delta meaningless.
  const skipDirection = useRef<1 | -1>(1);

  // The slot holding the next track, and the queue index it holds.
  const preloaded = useRef<{ index: number; slot: 0 | 1 } | undefined>(undefined);
  const fadeTimer = useRef<number | undefined>(undefined);
  const rateTimer = useRef<number | undefined>(undefined);
  // True only while a transition is stopping the outgoing element, so its
  // "pause" event is not mistaken for the user pausing playback.
  const adopting = useRef(false);
  // Set when audio for an index is already playing on an element, so the load
  // effect leaves it alone instead of restarting it from the network.
  const alreadyPlaying = useRef<number | undefined>(undefined);

  const recentlyPlayed = useRef<string[]>([]);
  const notePlayed = useCallback((song?: SongSummary) => {
    if (!song) return;
    recentlyPlayed.current = [song.id, ...recentlyPlayed.current.filter((id) => id !== song.id)].slice(0, 200);
  }, []);
  const reshuffle = useCallback((songs: SongSummary[], anchor: number) => {
    if (optionsRef.current.shuffleMode === "random") return shuffledIndexes(songs.length, anchor);
    const stale = new Set(recentlyPlayed.current);
    return shuffledIndexes(songs.length, anchor, (index) => stale.has(songs[index]?.id));
  }, []);
  const reshuffleRef = useRef(reshuffle);
  reshuffleRef.current = reshuffle;

  const nextIndex = useCallback((direction: 1 | -1) => {
    if (!queue.length) return -1;
    if (shuffle && queue.length > 1) {
      const stale = shuffleOrder.current.length !== queue.length || !shuffleOrder.current.includes(index);
      if (stale) {
        shuffleOrder.current = reshuffleRef.current(queueRef.current, index);
        shuffleCursor.current = 0;
      } else {
        shuffleCursor.current = Math.max(0, shuffleOrder.current.indexOf(index));
      }
      const nextCursor = shuffleCursor.current + direction;
      if (nextCursor >= 0 && nextCursor < shuffleOrder.current.length) {
        shuffleCursor.current = nextCursor;
        return shuffleOrder.current[nextCursor];
      }
      if (repeat === "all") {
        shuffleOrder.current = reshuffleRef.current(queueRef.current, index);
        shuffleCursor.current = direction === 1 ? Math.min(1, queue.length - 1) : queue.length - 1;
        return shuffleOrder.current[shuffleCursor.current] ?? index;
      }
      return -1;
    }
    const candidate = index + direction;
    if (candidate >= 0 && candidate < queue.length) return candidate;
    return repeat === "all" ? (candidate < 0 ? queue.length - 1 : 0) : -1;
  }, [index, queue, repeat, shuffle]);

  const nextIndexRef = useRef(nextIndex);
  nextIndexRef.current = nextIndex;

  const cancelFade = useCallback(() => {
    if (fadeTimer.current !== undefined) {
      window.clearInterval(fadeTimer.current);
      fadeTimer.current = undefined;
    }
  }, []);

  /// Stops whatever the idle element was holding. Every manual transition goes
  /// through here so a skip is a hard cut, never a fade.
  const releasePartner = useCallback(() => {
    cancelFade();
    const idle = partner();
    if (idle) {
      idle.pause();
      idle.removeAttribute("src");
      idle.load();
      idle.volume = volumeRef.current;
    }
    preloaded.current = undefined;
  }, [cancelFade, partner]);

  const submitScrobble = useCallback((song?: SongSummary) => {
    if (!song || completedTrack.current === song.id) return;
    completedTrack.current = song.id;
    void invoke("scrobble", { id: song.id, submission: true }).catch(() => undefined);
  }, []);

  /// Hands playback to the element that already holds the next track. Used by
  /// both the gapless join and the end of a crossfade.
  const adoptPartner = useCallback((targetIndex: number) => {
    const outgoing = audio();
    submitScrobble(queueRef.current[indexRef.current]);
    if (outgoing) {
      adopting.current = true;
      outgoing.pause();
      adopting.current = false;
      outgoing.removeAttribute("src");
      outgoing.load();
      outgoing.volume = volumeRef.current;
    }
    activeSlot.current = activeSlot.current === 0 ? 1 : 0;
    const incoming = audio();
    if (incoming) {
      incoming.volume = volumeRef.current;
      setIsPlaying(!incoming.paused);
    }
    preloaded.current = undefined;
    completedTrack.current = undefined;
    publishedSecond.current = -1;
    pendingPosition.current = 0;
    alreadyPlaying.current = targetIndex;
    shouldAutoplay.current = true;
    skipDirection.current = 1;
    notePlayed(queueRef.current[targetIndex]);
    setPosition(0);
    setDuration(queueRef.current[targetIndex]?.duration ?? 0);
    if (manualQueueCountRef.current > 0) setManualQueueCount((count) => Math.max(0, count - 1));
    setIndex(targetIndex);
    const song = queueRef.current[targetIndex];
    if (song) void invoke("scrobble", { id: song.id, submission: false }).catch(() => undefined);
  }, [audio, notePlayed, submitScrobble]);

  const advance = useCallback((direction: 1 | -1) => {
    const candidate = nextIndexRef.current(direction);
    if (candidate < 0) {
      audio()?.pause();
      releasePartner();
      setIsPlaying(false);
      const last = queueRef.current[indexRef.current];
      if (direction === 1 && optionsRef.current.autoplay && last) optionsRef.current.onQueueExhausted?.(last);
      return;
    }
    releasePartner();
    shouldAutoplay.current = true;
    skipDirection.current = direction;
    pendingPosition.current = 0;
    if (direction === 1 && manualQueueCountRef.current > 0) {
      setManualQueueCount((count) => Math.max(0, count - 1));
    }
    setIndex(candidate);
    setLoadToken((value) => value + 1);
  }, [audio, releasePartner]);

  const advanceRef = useRef(advance);
  advanceRef.current = advance;
  const repeatRef = useRef(repeat);
  repeatRef.current = repeat;
  const currentRef = useRef(current);
  currentRef.current = current;
  const persistedRef = useRef<PersistedPlayback>(initial);
  persistedRef.current = { queue, index, position, volume, shuffle, repeat, contextLabel, manualQueueCount };

  const recoverPlayback = useCallback((message: string) => {
    const track = currentRef.current;
    if (!track || recoveryTimer.current !== undefined) return;
    const count = (failures.current.get(track.id) ?? 0) + 1;
    failures.current.set(track.id, count);
    pendingPosition.current = audio()?.currentTime ?? persistedRef.current.position;
    if (count <= 2) {
      setError(`${message} Retrying…`);
      recoveryTimer.current = window.setTimeout(() => {
        recoveryTimer.current = undefined;
        shouldAutoplay.current = true;
        setLoadToken((value) => value + 1);
      }, count * 800);
    } else {
      setError(`${message} Continuing with the queue.`);
      recoveryTimer.current = window.setTimeout(() => {
        recoveryTimer.current = undefined;
        advanceRef.current(1);
      }, 900);
    }
  }, [audio]);

  const beginCrossfade = useCallback((seconds: number, targetIndex: number) => {
    const outgoing = audio();
    const incoming = partner();
    if (!outgoing || !incoming || fadeTimer.current !== undefined) return;
    const startVolume = volumeRef.current;
    const startedAt = Date.now();
    incoming.volume = 0;
    incoming.currentTime = 0;
    void incoming.play().catch(() => undefined);
    fadeTimer.current = window.setInterval(() => {
      const ratio = Math.min(1, (Date.now() - startedAt) / (seconds * 1000));
      outgoing.volume = Math.max(0, startVolume * (1 - ratio));
      incoming.volume = Math.min(1, startVolume * ratio);
      if (ratio < 1) return;
      cancelFade();
      adoptPartner(targetIndex);
    }, FADE_TICK_MS);
  }, [adoptPartner, audio, cancelFade, partner]);

  // Listeners live on both elements; everything but the active one is ignored,
  // so a crossfade partner cannot drive shell state while it ramps up.
  useEffect(() => {
    const elements = elementsRef.current.filter((element): element is HTMLAudioElement => Boolean(element));
    if (!elements.length) return;
    for (const element of elements) element.volume = volumeRef.current;

    const isActive = (element: HTMLAudioElement) => element === elementsRef.current[activeSlot.current];
    const disposers = elements.map((element) => {
      const onTime = () => {
        if (!isActive(element)) return;
        const seconds = Math.floor(element.currentTime || 0);
        if (seconds !== publishedSecond.current) {
          publishedSecond.current = seconds;
          setPosition(element.currentTime || 0);
        }
        const crossfade = optionsRef.current.crossfadeSeconds;
        if (crossfade <= 0 || fadeTimer.current !== undefined || repeatRef.current === "one") return;
        const total = Number.isFinite(element.duration) ? element.duration : 0;
        if (!total) return;
        const remaining = total - element.currentTime;
        if (remaining > crossfade || remaining <= 0) return;
        const target = nextIndexRef.current(1);
        if (target < 0 || preloaded.current?.index !== target) return;
        beginCrossfade(Math.min(crossfade, remaining), target);
      };
      const onDuration = () => {
        if (!isActive(element)) return;
        setDuration(Number.isFinite(element.duration) ? element.duration : currentRef.current?.duration ?? 0);
      };
      const onPlay = () => {
        if (!isActive(element)) return;
        setIsPlaying(true);
        if (currentRef.current) failures.current.delete(currentRef.current.id);
      };
      const onPause = () => {
        // A crossfade or a gapless join stops the outgoing element on purpose;
        // that is not the user pausing playback.
        if (!isActive(element) || fadeTimer.current !== undefined || adopting.current) return;
        setIsPlaying(false);
      };
      const onError = () => {
        if (!isActive(element)) return;
        recoverPlayback("This track could not be played.");
      };
      const onEnded = () => {
        if (!isActive(element)) return;
        if (repeatRef.current === "one") {
          submitScrobble(currentRef.current);
          completedTrack.current = undefined;
          element.currentTime = 0;
          void element.play();
          return;
        }
        const target = nextIndexRef.current(1);
        if (optionsRef.current.gapless && target >= 0 && preloaded.current?.index === target) {
          const incoming = partner();
          if (incoming) {
            incoming.volume = volumeRef.current;
            incoming.currentTime = 0;
            void incoming.play().catch(() => undefined);
            adoptPartner(target);
            return;
          }
        }
        submitScrobble(currentRef.current);
        advanceRef.current(1);
      };
      element.addEventListener("timeupdate", onTime);
      element.addEventListener("durationchange", onDuration);
      element.addEventListener("loadedmetadata", onDuration);
      element.addEventListener("play", onPlay);
      element.addEventListener("pause", onPause);
      element.addEventListener("error", onError);
      element.addEventListener("ended", onEnded);
      return () => {
        element.removeEventListener("timeupdate", onTime);
        element.removeEventListener("durationchange", onDuration);
        element.removeEventListener("loadedmetadata", onDuration);
        element.removeEventListener("play", onPlay);
        element.removeEventListener("pause", onPause);
        element.removeEventListener("error", onError);
        element.removeEventListener("ended", onEnded);
      };
    });
    return () => disposers.forEach((dispose) => dispose());
  }, [adoptPartner, beginCrossfade, partner, recoverPlayback, submitScrobble]);

  useEffect(() => {
    const element = audio();
    if (!element || !current) return;
    if (alreadyPlaying.current === index) {
      // The audio for this index is the element we just adopted.
      alreadyPlaying.current = undefined;
      setError(undefined);
      return;
    }
    let active = true;
    if (recoveryTimer.current !== undefined) {
      window.clearTimeout(recoveryTimer.current);
      recoveryTimer.current = undefined;
    }
    setError(undefined);
    completedTrack.current = undefined;
    publishedSecond.current = -1;
    setPosition(pendingPosition.current);
    setDuration(current.duration ?? 0);
    invoke<string>("media_url", { kind: "stream", id: current.id })
      .then(async (url) => {
        if (!active) return;
        element.volume = volumeRef.current;
        element.src = url;
        const startPosition = pendingPosition.current;
        if (startPosition > 0) {
          element.addEventListener("loadedmetadata", () => {
            element.currentTime = Math.min(startPosition, Number.isFinite(element.duration) ? element.duration : startPosition);
            setPosition(element.currentTime);
          }, { once: true });
        }
        element.load();
        notePlayed(current);
        void invoke("scrobble", { id: current.id, submission: false }).catch(() => undefined);
        if (shouldAutoplay.current) await element.play();
      })
      .catch((reason) => {
        if (active) recoverPlayback(typeof reason === "string" ? reason : "Could not prepare this track.");
      });
    return () => { active = false };
  }, [audio, current?.id, index, loadToken, notePlayed, recoverPlayback]);

  // Stage the next track on the idle element. This is what makes the join
  // gapless and what a crossfade ramps into.
  useEffect(() => {
    if (!options.gapless && options.crossfadeSeconds <= 0) {
      preloaded.current = undefined;
      return;
    }
    const target = nextIndex(1);
    const song = target >= 0 ? queue[target] : undefined;
    if (!song || repeat === "one") {
      preloaded.current = undefined;
      return;
    }
    if (preloaded.current?.index === target) return;
    let active = true;
    void invoke<string>("media_url", { kind: "stream", id: song.id })
      .then((url) => {
        const idle = partner();
        if (!active || !idle) return;
        idle.volume = 0;
        idle.src = url;
        idle.load();
        preloaded.current = { index: target, slot: activeSlot.current === 0 ? 1 : 0 };
      })
      .catch(() => undefined);
    return () => { active = false };
  }, [index, nextIndex, options.crossfadeSeconds, options.gapless, partner, queue, repeat]);

  useEffect(() => {
    const timeout = window.setTimeout(() => {
      localStorage.setItem(storageKey, JSON.stringify(persistedRef.current));
      localStorage.removeItem(STORAGE_KEY);
    }, 250);
    return () => window.clearTimeout(timeout);
  }, [contextLabel, index, manualQueueCount, queue, repeat, shuffle, storageKey, volume]);

  useEffect(() => {
    const persist = () => {
      localStorage.setItem(storageKey, JSON.stringify(persistedRef.current));
      localStorage.removeItem(STORAGE_KEY);
    };
    const interval = window.setInterval(persist, 5_000);
    window.addEventListener("beforeunload", persist);
    return () => {
      window.clearInterval(interval);
      window.removeEventListener("beforeunload", persist);
      persist();
    };
  }, [storageKey]);

  const playQueue = useCallback((songs: SongSummary[], startIndex = 0, autoplay = true, startPosition = 0, label = "Queue") => {
    if (!songs.length) return;
    const safeIndex = Math.min(Math.max(0, startIndex), songs.length - 1);
    releasePartner();
    alreadyPlaying.current = undefined;
    shouldAutoplay.current = autoplay;
    skipDirection.current = 1;
    pendingPosition.current = Math.max(0, startPosition);
    shuffleOrder.current = reshuffle(songs, safeIndex);
    shuffleCursor.current = 0;
    setQueue(songs);
    setIndex(safeIndex);
    setManualQueueCount(0);
    setUndoQueue(undefined);
    setContextLabel(label);
    setLoadToken((value) => value + 1);
  }, [releasePartner]);

  const toggle = useCallback(async () => {
    const element = audio();
    if (!element || !current) return;
    if (element.paused) {
      shouldAutoplay.current = true;
      await element.play().catch(() => setError("Playback was blocked. Choose the track again."));
    } else {
      cancelFade();
      element.pause();
    }
  }, [audio, cancelFade, current]);

  const seek = useCallback((seconds: number) => {
    const element = audio();
    if (!element) return;
    cancelFade();
    element.currentTime = Math.max(0, Math.min(seconds, duration || seconds));
    setPosition(element.currentTime);
  }, [audio, cancelFade, duration]);

  /// The exact playhead, read from the audio element at the moment of the call.
  ///
  /// `position` is React state that is deliberately published only when the
  /// whole second changes, so the shell does not re-render several times a
  /// second; the progress rail interpolates between those anchors locally, so
  /// the throttle is invisible on screen. It is not invisible to anything that
  /// treats the value as a *measurement* — every Splynt Connect path did, and
  /// inherited up to a second of error both in the clock it published and in
  /// the drift it computed against another device's clock. Those read this.
  const positionNow = useCallback(() => {
    const element = audio();
    if (element && element.readyState > 0 && Number.isFinite(element.currentTime)) {
      return Math.max(0, element.currentTime);
    }
    // Mid-load the element has no meaningful time yet; the last published
    // anchor is closer to the truth than zero.
    return positionRef.current;
  }, [audio]);

  /// Nudges the audio clock instead of jumping it, for Splynt Connect group
  /// drift correction.
  ///
  /// `preservesPitch` is on by default in both webviews Splynt ships against,
  /// but it is set here anyway: a 2% correction that shifts pitch is a
  /// correction the listener can hear, which is the whole thing this avoids.
  /// The restore has its own deadline because the leader can go quiet
  /// mid-convergence, and a follower left at 98% walks away from the room at
  /// exactly the rate meant to catch it up.
  const convergeRate = useCallback((rate: number, seconds: number) => {
    const element = audio();
    if (!element || element.paused) return;
    if (rateTimer.current !== undefined) window.clearTimeout(rateTimer.current);
    element.preservesPitch = true;
    element.playbackRate = rate;
    rateTimer.current = window.setTimeout(() => {
      rateTimer.current = undefined;
      const current = audio();
      if (current) current.playbackRate = 1;
    }, Math.max(0, seconds) * 1000);
  }, [audio]);

  /// Returns to normal speed. Safe when nothing is converging.
  const endConvergence = useCallback(() => {
    if (rateTimer.current !== undefined) {
      window.clearTimeout(rateTimer.current);
      rateTimer.current = undefined;
    }
    const element = audio();
    if (element && element.playbackRate !== 1) element.playbackRate = 1;
  }, [audio]);

  const setVolume = useCallback((value: number) => {
    const safe = Math.max(0, Math.min(1, value));
    volumeRef.current = safe;
    const element = audio();
    if (element && fadeTimer.current === undefined) element.volume = safe;
    setVolumeState(safe);
  }, [audio]);

  const setShuffle = useCallback((value: boolean | ((current: boolean) => boolean)) => {
    setShuffleState((currentValue) => {
      const nextValue = typeof value === "function" ? value(currentValue) : value;
      if (nextValue !== currentValue) {
        shuffleOrder.current = reshuffleRef.current(queueRef.current, index);
        shuffleCursor.current = 0;
        preloaded.current = undefined;
      }
      return nextValue;
    });
  }, [index, queue.length]);

  const cycleRepeat = useCallback(() => setRepeat((value) => value === "off" ? "all" : value === "all" ? "one" : "off"), []);
  const recordUndo = useCallback((label: string) => {
    setUndoQueue({
      label,
      queue: queueRef.current,
      index: indexRef.current,
      manualQueueCount: manualQueueCountRef.current,
    });
  }, []);
  const enqueue = useCallback((song: SongSummary) => {
    recordUndo(`Added ${song.title} to the queue`);
    const target = Math.max(0, indexRef.current + manualQueueCountRef.current + 1);
    setQueue((items) => [...items.slice(0, target), song, ...items.slice(target)]);
    setManualQueueCount((count) => count + 1);
  }, [recordUndo]);
  const playNext = useCallback((song: SongSummary) => {
    recordUndo(`Added ${song.title} next`);
    setQueue((items) => {
      const target = Math.max(0, indexRef.current + 1);
      return [...items.slice(0, target), song, ...items.slice(target)];
    });
    setManualQueueCount((count) => count + 1);
  }, [recordUndo]);
  const removeQueueItem = useCallback((target: number) => {
    if (target === index) return;
    const removed = queueRef.current[target];
    recordUndo(removed ? `Removed ${removed.title}` : "Changed the queue");
    if (target > index && target <= index + manualQueueCountRef.current) {
      setManualQueueCount((count) => Math.max(0, count - 1));
    }
    setQueue((items) => items.filter((_, itemIndex) => itemIndex !== target));
    if (target < index) setIndex((value) => value - 1);
    preloaded.current = undefined;
  }, [index, recordUndo]);
  const moveQueueItem = useCallback((from: number, to: number) => {
    if (from === to || from < 0 || to < 0 || from >= queue.length || to >= queue.length) return;
    recordUndo("Reordered the queue");
    setQueue((items) => {
      const next = [...items];
      const [moved] = next.splice(from, 1);
      next.splice(to, 0, moved);
      return next;
    });
    if (from === index) setIndex(to);
    else if (from < index && to >= index) setIndex((value) => value - 1);
    else if (from > index && to <= index) setIndex((value) => value + 1);
    preloaded.current = undefined;
  }, [index, queue, recordUndo]);
  const clearUpcoming = useCallback(() => {
    if (!current) return;
    recordUndo("Cleared the upcoming queue");
    releasePartner();
    setQueue([current]);
    setIndex(0);
    setManualQueueCount(0);
  }, [current, recordUndo, releasePartner]);
  const clearManualQueue = useCallback(() => {
    const count = manualQueueCountRef.current;
    if (!count) return;
    recordUndo("Cleared the manual queue");
    const start = indexRef.current + 1;
    setQueue((items) => [...items.slice(0, start), ...items.slice(start + count)]);
    setManualQueueCount(0);
    preloaded.current = undefined;
  }, [recordUndo]);
  const undoQueueMutation = useCallback(() => {
    if (!undoQueue) return;
    setQueue(undoQueue.queue);
    setIndex(undoQueue.index);
    setManualQueueCount(undoQueue.manualQueueCount);
    setUndoQueue(undefined);
    preloaded.current = undefined;
  }, [undoQueue]);
  /// Keeps the playing element untouched while the surrounding queue is
  /// replaced — used by a Connect handoff once the full track list arrives.
  const replaceQueuePreservingCurrent = useCallback((songs: SongSummary[]) => {
    const active = currentRef.current;
    if (!active || !songs.length) return;
    const activeIndex = songs.findIndex((song) => song.id === active.id);
    if (activeIndex < 0) return;
    alreadyPlaying.current = activeIndex;
    setQueue(songs);
    setIndex(activeIndex);
    setManualQueueCount(0);
    shuffleOrder.current = reshuffle(songs, activeIndex);
    shuffleCursor.current = 0;
    preloaded.current = undefined;
  }, []);

  /// Appends without disturbing what is playing — how autoplay extends a queue
  /// that ran out.
  const appendToQueue = useCallback((songs: SongSummary[]) => {
    if (!songs.length) return;
    shuffleOrder.current = [];
    setQueue((items) => {
      const known = new Set(items.map((item) => item.id));
      const fresh = songs.filter((song) => !known.has(song.id));
      return fresh.length ? [...items, ...fresh] : items;
    });
  }, []);

  useEffect(() => {
    if (!("mediaSession" in navigator)) return;
    const session = navigator.mediaSession;
    session.setActionHandler("play", () => { const element = audio(); if (element?.paused) void element.play(); });
    session.setActionHandler("pause", () => audio()?.pause());
    session.setActionHandler("previoustrack", () => position > 4 ? seek(0) : advance(-1));
    session.setActionHandler("nexttrack", () => advance(1));
    session.setActionHandler("seekto", (details) => details.seekTime !== undefined && seek(details.seekTime));
    return () => {
      for (const action of ["play", "pause", "previoustrack", "nexttrack", "seekto"] as MediaSessionAction[]) {
        session.setActionHandler(action, null);
      }
    };
  }, [advance, audio, position, seek]);

  useEffect(() => {
    if (!("mediaSession" in navigator) || !current) return;
    let active = true;
    const publish = (artwork?: string) => {
      if (!active) return;
      navigator.mediaSession.metadata = new MediaMetadata({
        title: current.title,
        artist: current.artist,
        album: current.album,
        artwork: artwork ? [{ src: artwork }] : undefined,
      });
    };
    if (current.coverArt) void invoke<string>("media_url", { kind: "cover", id: current.coverArt }).then(publish).catch(() => publish());
    else publish();
    return () => { active = false };
  }, [current]);

  useEffect(() => {
    if (!("mediaSession" in navigator)) return;
    navigator.mediaSession.playbackState = current ? (isPlaying ? "playing" : "paused") : "none";
  }, [current, isPlaying]);

  useEffect(() => {
    if (!("mediaSession" in navigator) || !current || !Number.isFinite(duration) || duration <= 0) return;
    try {
      navigator.mediaSession.setPositionState({
        duration,
        playbackRate: audio()?.playbackRate ?? 1,
        position: Math.max(0, Math.min(position, duration)),
      });
    } catch {
      // Some webviews expose Media Session before position updates are supported.
    }
  }, [audio, current, duration, position]);

  useEffect(() => () => {
    if (recoveryTimer.current !== undefined) window.clearTimeout(recoveryTimer.current);
    if (fadeTimer.current !== undefined) window.clearInterval(fadeTimer.current);
    if (rateTimer.current !== undefined) window.clearTimeout(rateTimer.current);
    for (const element of elementsRef.current) {
      if (!element) continue;
      element.pause();
      element.removeAttribute("src");
      element.load();
    }
  }, []);

  return useMemo(() => ({
    current, queue, index, isPlaying, position, duration, volume, shuffle, repeat, contextLabel, error,
    trackDirection: skipDirection.current,
    manualQueueCount, undoQueueLabel: undoQueue?.label,
    playQueue, toggle, next: () => advance(1), previous: () => position > 4 ? seek(0) : advance(-1),
    seek, setVolume, setShuffle, cycleRepeat, enqueue, playNext, removeQueueItem, moveQueueItem, clearUpcoming, clearManualQueue, undoQueueMutation,
    replaceQueuePreservingCurrent, appendToQueue, positionNow, convergeRate, endConvergence,
  }), [advance, appendToQueue, clearManualQueue, clearUpcoming, contextLabel, convergeRate, current, cycleRepeat, duration, endConvergence, enqueue, error, index, isPlaying,
    manualQueueCount, moveQueueItem, playNext, playQueue, position, positionNow, queue, removeQueueItem, repeat, replaceQueuePreservingCurrent, seek, setShuffle, setVolume,
    shuffle, toggle, undoQueue, undoQueueMutation, volume]);
}

export type PlaybackController = ReturnType<typeof usePlayback>;
