// How the equalizer gets into desktop playback.
//
// The iOS client has to hand-write biquads because AVPlayer only exposes raw
// PCM through an MTAudioProcessingTap. The web platform hands the same filters
// over as BiquadFilterNode, so this file is much smaller than its Swift
// counterpart, and the maths that both clients agree on lives in equalizer.ts.
//
// Three constraints shape everything here:
//
// 1. `createMediaElementSource` is one-way. Once an element is routed into an
//    AudioContext its audio only reaches the speakers through that graph, and
//    there is no API to put it back. So attaching is done lazily, the first
//    time the listener actually turns the equalizer on, and turning it off
//    afterwards flattens the filters rather than trying to undo the routing.
// 2. The element must be CORS-clean or the graph outputs silence. Splynt's
//    audio comes from its own local media proxy, which already answers with
//    `Access-Control-Allow-Origin: *` on both the streamed and the downloaded
//    path, so `crossOrigin = "anonymous"` is safe. It has to be set before any
//    `src` is assigned, which is why createElement in usePlayback sets it
//    rather than this file setting it at attach time.
// 3. An AudioContext can start suspended under an autoplay policy. Every
//    attach and every apply nudges it, because a suspended context is silence
//    that looks exactly like a bug.

import { equalizerBands, bandFilterType, isEqualizerActive, type EqualizerSettings } from "./equalizer";

type Chain = {
  source: MediaElementAudioSourceNode;
  filters: BiquadFilterNode[];
  preamp: GainNode;
};

/// Parameter changes ramp over this many seconds instead of stepping. A step
/// on a running filter is a click; 20 ms is short enough to feel immediate on
/// a drag and long enough to stay silent.
const rampSeconds = 0.02;

export class EqualizerGraph {
  private context: AudioContext | undefined;
  private chains = new Map<HTMLMediaElement, Chain>();
  private settings: EqualizerSettings | undefined;
  /// Elements offered before the equalizer was ever switched on. Held so the
  /// first activation can reach whatever is already playing.
  private pending = new Set<HTMLMediaElement>();

  /// Offer an element. Nothing happens until the equalizer is active, which is
  /// what keeps a listener who never opens the equalizer on the plain
  /// element-to-speakers path.
  register(element: HTMLMediaElement | undefined): void {
    if (!element || this.chains.has(element)) return;
    this.pending.add(element);
    if (this.settings && isEqualizerActive(this.settings)) this.attach(element);
  }

  forget(element: HTMLMediaElement | undefined): void {
    if (!element) return;
    this.pending.delete(element);
    const chain = this.chains.get(element);
    if (!chain) return;
    // Disconnecting the source does not un-route the element, so the chain
    // stays in the map. This only runs on teardown.
    try {
      chain.source.disconnect();
      chain.filters.forEach((filter) => filter.disconnect());
      chain.preamp.disconnect();
    } catch {
      // A context torn down underneath us throws here and there is nothing
      // left to clean up.
    }
    this.chains.delete(element);
  }

  apply(settings: EqualizerSettings): void {
    this.settings = settings;
    if (isEqualizerActive(settings)) {
      for (const element of [...this.pending]) this.attach(element);
    }
    if (!this.chains.size) return;
    void this.context?.resume().catch(() => undefined);
    const now = this.context?.currentTime ?? 0;
    for (const chain of this.chains.values()) {
      // Off means flat, not disconnected: see the note at the top of the file.
      const active = isEqualizerActive(settings);
      chain.filters.forEach((filter, index) => {
        const gain = active ? (settings.gainsDb[index] ?? 0) : 0;
        filter.gain.setTargetAtTime(gain, now, rampSeconds);
      });
      const preamp = active ? Math.pow(10, settings.preampDb / 20) : 1;
      chain.preamp.gain.setTargetAtTime(preamp, now, rampSeconds);
    }
  }

  /// Whether anything is actually routed through Web Audio yet. For tests and
  /// diagnostics.
  get attachedCount(): number {
    return this.chains.size;
  }

  dispose(): void {
    for (const element of [...this.chains.keys()]) this.forget(element);
    void this.context?.close().catch(() => undefined);
    this.context = undefined;
    this.pending.clear();
  }

  private attach(element: HTMLMediaElement): void {
    if (this.chains.has(element)) return;
    const context = this.ensureContext();
    if (!context) return;
    let source: MediaElementAudioSourceNode;
    try {
      source = context.createMediaElementSource(element);
    } catch {
      // Thrown when this element already belongs to another context, which
      // should not happen but would otherwise take the whole player down.
      this.pending.delete(element);
      return;
    }
    const filters = equalizerBands.map((frequency, index) => {
      const filter = context.createBiquadFilter();
      filter.type = bandFilterType(index);
      filter.frequency.value = frequency;
      // Web Audio ignores Q on its shelves, so setting it is harmless there
      // and correct on the eight peaking bands in between.
      filter.Q.value = Math.SQRT2;
      filter.gain.value = this.settings?.gainsDb[index] ?? 0;
      return filter;
    });
    const preamp = context.createGain();
    preamp.gain.value = this.settings ? Math.pow(10, this.settings.preampDb / 20) : 1;

    // source -> band 0 -> ... -> band 9 -> preamp -> speakers
    let node: AudioNode = source;
    for (const filter of filters) {
      node.connect(filter);
      node = filter;
    }
    node.connect(preamp);
    preamp.connect(context.destination);

    this.chains.set(element, { source, filters, preamp });
    this.pending.delete(element);
    void context.resume().catch(() => undefined);
  }

  private ensureContext(): AudioContext | undefined {
    if (this.context) return this.context;
    if (typeof AudioContext === "undefined") return undefined;
    try {
      this.context = new AudioContext();
    } catch {
      return undefined;
    }
    return this.context;
  }
}
