/**
 * Wraps Web Audio playback for the emulator.
 *
 * Samples arrive interleaved (`[left, right, left, right, …]`) once per
 * emulated frame. They are handed to an `AudioWorkletNode` which drains them at
 * the device's own rate; browsers without AudioWorklet fall back to the older
 * (deprecated but ubiquitous) `ScriptProcessorNode`.
 */
export class AudioOutput {
  private context: AudioContext | null = null;
  private gain: GainNode | null = null;
  private worklet: AudioWorkletNode | null = null;
  private scriptNode: ScriptProcessorNode | null = null;

  /** Fallback queue used only when AudioWorklet is unavailable. */
  private queue: Float32Array[] = [];
  private offset = 0;
  private queuedFloats = 0;

  private muted = false;
  private startPromise: Promise<void> | null = null;

  get enabled(): boolean {
    return this.context !== null;
  }

  /** Create the audio graph on first use and resume playback. */
  async ensureStarted(): Promise<void> {
    if (this.context) {
      await this.resume();
      return;
    }
    this.startPromise ??= this.start();
    await this.startPromise;
  }

  async resume(): Promise<void> {
    if (this.context?.state === 'suspended') {
      try {
        await this.context.resume();
      } catch {
        // Autoplay policy can reject this before the first user gesture; the
        // next gesture will try again.
      }
    }
  }

  /** Queue one batch of interleaved stereo samples. */
  enqueue(samples: Float32Array): void {
    if (samples.length === 0) return;

    const copy = samples.slice();
    if (this.worklet) {
      this.worklet.port.postMessage({ samples: copy }, [copy.buffer]);
      return;
    }

    this.queue.push(copy);
    this.queuedFloats += copy.length;
    const maxQueuedFloats = 48000 * 2;
    while (this.queue.length > 1 && this.queuedFloats > maxQueuedFloats) {
      const dropped = this.queue.shift();
      if (!dropped) break;
      this.queuedFloats -= dropped.length;
    }
  }

  /** Drop every queued sample, e.g. when loading or resetting a cartridge. */
  flush(): void {
    this.queue = [];
    this.offset = 0;
    this.queuedFloats = 0;
    this.worklet?.port.postMessage('flush');
  }

  setMuted(muted: boolean): void {
    this.muted = muted;
    if (this.gain && this.context) {
      this.gain.gain.setTargetAtTime(muted ? 0 : 1, this.context.currentTime, 0.01);
    }
  }

  dispose(): void {
    this.flush();
    try {
      this.worklet?.disconnect();
      if (this.scriptNode) {
        this.scriptNode.onaudioprocess = null;
        this.scriptNode.disconnect();
      }
      this.gain?.disconnect();
    } catch {
      // Disconnecting an already-torn-down graph is harmless.
    }
    void this.context?.close().catch(() => undefined);
    this.worklet = null;
    this.scriptNode = null;
    this.gain = null;
    this.context = null;
    this.startPromise = null;
  }

  private async start(): Promise<void> {
    const Ctor =
      window.AudioContext ??
      (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext;
    if (!Ctor) return;

    const context = new Ctor();
    this.context = context;

    const gain = context.createGain();
    gain.gain.value = this.muted ? 0 : 1;
    gain.connect(context.destination);
    this.gain = gain;

    try {
      const workletUrl = `${import.meta.env.BASE_URL}nes-audio-processor.js`;
      await context.audioWorklet.addModule(workletUrl);
      const node = new AudioWorkletNode(context, 'nes-audio-processor', {
        numberOfInputs: 0,
        numberOfOutputs: 1,
        outputChannelCount: [2],
      });
      node.connect(gain);
      this.worklet = node;
    } catch (error) {
      console.warn('AudioWorklet unavailable, falling back to ScriptProcessorNode.', error);
      const node = context.createScriptProcessor(4096, 0, 2);
      node.onaudioprocess = (event) => this.fillOutput(event.outputBuffer);
      node.connect(gain);
      this.scriptNode = node;
    }

    await this.resume();
  }

  /** Pull queued samples into a ScriptProcessor output buffer. */
  private fillOutput(buffer: AudioBuffer): void {
    const left = buffer.getChannelData(0);
    const right = buffer.numberOfChannels > 1 ? buffer.getChannelData(1) : left;
    let written = 0;

    while (written < left.length) {
      const chunk = this.queue[0];
      if (!chunk) break;

      const available = Math.floor((chunk.length - this.offset) / 2);
      const take = Math.min(available, left.length - written);

      for (let i = 0; i < take; i += 1) {
        left[written + i] = chunk[this.offset + i * 2] ?? 0;
        right[written + i] = chunk[this.offset + i * 2 + 1] ?? 0;
      }

      written += take;
      this.offset += take * 2;
      this.queuedFloats -= take * 2;

      if (this.offset >= chunk.length) {
        this.queue.shift();
        this.offset = 0;
      }
    }

    for (let i = written; i < left.length; i += 1) {
      left[i] = 0;
      if (right !== left) right[i] = 0;
    }
  }
}
