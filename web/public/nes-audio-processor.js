/**
 * AudioWorklet that plays interleaved stereo samples sent from the main thread.
 *
 * The emulator produces one small batch of samples per emulated frame, so the
 * batches are queued here and drained at the audio device's own pace. When the
 * queue runs dry the processor emits silence, which keeps playback glitch-free
 * if the emulator ever falls behind.
 */
const MAX_QUEUED_FRAMES = 24000; // ~0.5s at 48kHz; dropping older audio limits latency.

class NesAudioProcessor extends AudioWorkletProcessor {
  constructor() {
    super();
    this.queue = [];
    this.offset = 0;
    this.queuedFrames = 0;

    this.port.onmessage = (event) => {
      if (event.data === 'flush') {
        this.queue = [];
        this.offset = 0;
        this.queuedFrames = 0;
        return;
      }
      const samples = event.data?.samples;
      if (samples instanceof Float32Array && samples.length > 0) {
        this.queue.push(samples);
        this.queuedFrames += samples.length / 2;
        this.trim();
      }
    };
  }

  trim() {
    while (this.queue.length > 1 && this.queuedFrames > MAX_QUEUED_FRAMES) {
      const dropped = this.queue.shift();
      this.queuedFrames -= dropped.length / 2;
    }
  }

  process(_inputs, outputs) {
    const output = outputs[0];
    if (!output || output.length === 0) {
      return true;
    }

    const left = output[0];
    const right = output.length > 1 ? output[1] : output[0];
    const frames = left.length;
    let written = 0;

    while (written < frames) {
      const chunk = this.queue[0];
      if (!chunk) break;

      const available = Math.floor((chunk.length - this.offset) / 2);
      const take = Math.min(available, frames - written);

      for (let i = 0; i < take; i += 1) {
        left[written + i] = chunk[this.offset + i * 2] ?? 0;
        right[written + i] = chunk[this.offset + i * 2 + 1] ?? 0;
      }

      written += take;
      this.offset += take * 2;
      this.queuedFrames -= take;

      if (this.offset >= chunk.length) {
        this.queue.shift();
        this.offset = 0;
      }
    }

    // Keep the tail silent rather than leaving stale samples in the buffer.
    for (let i = written; i < frames; i += 1) {
      left[i] = 0;
      if (right !== left) right[i] = 0;
    }

    return true;
  }
}

registerProcessor('nes-audio-processor', NesAudioProcessor);
