import init, {
  get_button_a,
  get_button_b,
  get_button_down,
  get_button_left,
  get_button_right,
  get_button_select,
  get_button_start,
  get_button_up,
  JsNes,
} from '../wasm/nes_wasm.js';
import { AudioOutput } from './AudioOutput';
import {
  BUTTON_NAMES,
  FRAME_DURATION_MS,
  NES_HEIGHT,
  NES_WIDTH,
  type ButtonName,
} from './constants';

export type EmulatorState = 'idle' | 'running' | 'paused';

/** Emulated frames to catch up on in a single animation frame before giving up. */
const MAX_CATCH_UP_FRAMES = 4;

/**
 * Largest rAF gap we will simulate. When a tab is backgrounded the browser
 * stops calling `requestAnimationFrame`, so the next delta can be seconds long;
 * clamping it avoids a lengthy fast-forward burst on return.
 */
const MAX_FRAME_DELTA_MS = 250;

/** How often the measured frame rate is reported back to the UI. */
const FPS_SAMPLE_MS = 500;

export interface NesEmulatorOptions {
  onStateChange?: (state: EmulatorState) => void;
  onFpsChange?: (fps: number) => void;
}

let wasmInit: Promise<unknown> | null = null;

/** Instantiate the WASM module once, no matter how many callers ask for it. */
export function initWasm(): Promise<unknown> {
  wasmInit ??= init();
  return wasmInit;
}

/** Button indices, read from the WASM module so both sides cannot drift apart. */
function readButtonIndices(): Record<ButtonName, number> {
  return {
    A: get_button_a(),
    B: get_button_b(),
    SELECT: get_button_select(),
    START: get_button_start(),
    UP: get_button_up(),
    DOWN: get_button_down(),
    LEFT: get_button_left(),
    RIGHT: get_button_right(),
  };
}

/**
 * Frame-accurate driver for the Rust/WASM NES core.
 *
 * Owns the display canvas, the emulation clock, and audio output; the React
 * layer only forwards input and reflects the state reported through callbacks.
 */
export class NesEmulator {
  private readonly canvas: HTMLCanvasElement;
  private readonly context: CanvasRenderingContext2D;
  private readonly imageData: ImageData;
  private readonly audio = new AudioOutput();
  private readonly options: NesEmulatorOptions;

  private nes: JsNes | null = null;
  private buttons: Record<ButtonName, number> | null = null;

  private rafId: number | null = null;
  private lastTimestamp = 0;
  private frameAccumulator = 0;
  private framesSinceSample = 0;
  private lastFpsSample = 0;

  private currentState: EmulatorState = 'idle';
  private disposed = false;

  constructor(canvas: HTMLCanvasElement, options: NesEmulatorOptions = {}) {
    this.canvas = canvas;
    this.options = options;

    const context = canvas.getContext('2d', { alpha: false });
    if (!context) {
      throw new Error('无法创建 Canvas 2D 上下文。');
    }
    this.context = context;

    // Render at the console's native resolution and let CSS scale the element
    // up with integer factors — cheaper and crisper than scaling in JS.
    canvas.width = NES_WIDTH;
    canvas.height = NES_HEIGHT;
    context.imageSmoothingEnabled = false;
    this.imageData = context.createImageData(NES_WIDTH, NES_HEIGHT);
  }

  get state(): EmulatorState {
    return this.currentState;
  }

  get hasRom(): boolean {
    return this.nes !== null;
  }

  /** Load a validated ROM image and start running it. */
  async loadRom(data: Uint8Array): Promise<void> {
    await initWasm();
    this.assertUsable();

    // Build the new instance before touching the old one so a failed load
    // leaves the currently running game untouched.
    const next = new JsNes();
    try {
      next.load_rom(data);
    } catch (error) {
      next.free();
      throw new Error(`模拟器内核拒绝了该 ROM：${describe(error)}`);
    }

    this.nes?.free();
    this.nes = next;
    this.buttons = readButtonIndices();

    this.audio.flush();
    this.lastTimestamp = 0;
    this.frameAccumulator = 0;

    // Emulate one frame up front so the first paint is not a blank screen.
    this.stepFrames(1);
    this.render();

    this.setState('running');
    this.startLoop();
    void this.audio.ensureStarted().catch((error) => {
      console.warn('音频初始化失败，将继续静音运行。', error);
    });
  }

  /** Warp back to the cartridge's reset vector. */
  reset(): void {
    if (!this.nes) return;
    this.nes.reset();
    this.audio.flush();
    this.lastTimestamp = 0;
    this.frameAccumulator = 0;
    this.stepFrames(1);
    this.render();
  }

  pause(): void {
    if (this.currentState !== 'running') return;
    this.stopLoop();
    this.audio.flush();
    this.setState('paused');
  }

  resume(): void {
    if (this.currentState !== 'paused') return;
    this.lastTimestamp = 0;
    this.frameAccumulator = 0;
    this.setState('running');
    this.startLoop();
    void this.audio.ensureStarted().catch(() => undefined);
  }

  togglePause(): void {
    if (this.currentState === 'paused') {
      this.resume();
    } else {
      this.pause();
    }
  }

  setButton(button: ButtonName, pressed: boolean): void {
    this.applyButton(button, pressed);
  }

  /** Release every button, e.g. when the window loses focus mid-press. */
  releaseAllButtons(): void {
    for (const button of BUTTON_NAMES) {
      this.applyButton(button, false);
    }
  }

  setMuted(muted: boolean): void {
    this.audio.setMuted(muted);
  }

  /** Called from user-gesture handlers to satisfy the browser autoplay policy. */
  wakeAudio(): void {
    void this.audio.ensureStarted().catch(() => undefined);
  }

  dispose(): void {
    this.disposed = true;
    this.stopLoop();
    this.nes?.free();
    this.nes = null;
    this.buttons = null;
    this.audio.dispose();
  }

  private startLoop(): void {
    if (this.rafId !== null || this.disposed) return;
    this.rafId = requestAnimationFrame(this.tick);
  }

  private stopLoop(): void {
    if (this.rafId === null) return;
    cancelAnimationFrame(this.rafId);
    this.rafId = null;
  }

  /**
   * Emulation clock. Frames are released on a fixed 60.0988 Hz schedule rather
   * than once per animation frame, so the game speed follows the console rather
   * than the display's refresh rate.
   */
  private readonly tick = (timestamp: number): void => {
    this.rafId = requestAnimationFrame(this.tick);

    if (this.currentState !== 'running' || !this.nes) {
      this.lastTimestamp = timestamp;
      return;
    }
    if (this.lastTimestamp === 0) {
      this.lastTimestamp = timestamp;
      return;
    }

    let delta = timestamp - this.lastTimestamp;
    this.lastTimestamp = timestamp;
    if (delta > MAX_FRAME_DELTA_MS) delta = MAX_FRAME_DELTA_MS;

    this.frameAccumulator += delta;

    let frames = 0;
    while (this.frameAccumulator >= FRAME_DURATION_MS && frames < MAX_CATCH_UP_FRAMES) {
      this.frameAccumulator -= FRAME_DURATION_MS;
      frames += 1;
    }

    if (frames === 0) return;

    // If we are still behind after the catch-up budget, drop the backlog
    // instead of accumulating latency that can never be repaid.
    if (this.frameAccumulator > FRAME_DURATION_MS * MAX_CATCH_UP_FRAMES) {
      this.frameAccumulator = 0;
    }

    this.stepFrames(frames);
    this.render();
    this.pumpAudio();
    this.reportFps(timestamp, frames);
  };

  private stepFrames(count: number): void {
    for (let i = 0; i < count; i += 1) {
      this.nes?.frame();
    }
  }

  /** Push a button state straight through to the emulated controller. */
  private applyButton(button: ButtonName, pressed: boolean): void {
    const index = this.buttons?.[button];
    if (index === undefined) return;
    this.nes?.set_button(1, index, pressed);
  }

  /** Blit the 32-bit ARGB frame buffer into the canvas. */
  private render(): void {
    if (!this.nes) return;

    const pixels = this.nes.get_frame_buffer();
    const target = this.imageData.data;
    const count = Math.min(pixels.length, NES_WIDTH * NES_HEIGHT);

    for (let i = 0, j = 0; i < count; i += 1, j += 4) {
      const pixel = pixels[i];
      target[j] = (pixel >>> 16) & 0xff;
      target[j + 1] = (pixel >>> 8) & 0xff;
      target[j + 2] = pixel & 0xff;
      target[j + 3] = 0xff;
    }

    this.context.putImageData(this.imageData, 0, 0);
  }

  private pumpAudio(): void {
    if (!this.nes) return;
    this.audio.enqueue(this.nes.get_audio_samples());
  }

  private reportFps(timestamp: number, frames: number): void {
    this.framesSinceSample += frames;

    if (this.lastFpsSample === 0) {
      this.lastFpsSample = timestamp;
      return;
    }

    const elapsed = timestamp - this.lastFpsSample;
    if (elapsed < FPS_SAMPLE_MS) return;

    const fps = Math.round((this.framesSinceSample * 1000) / elapsed);
    this.framesSinceSample = 0;
    this.lastFpsSample = timestamp;
    this.options.onFpsChange?.(fps);
  }

  private setState(state: EmulatorState): void {
    if (this.currentState === state) return;
    this.currentState = state;
    this.options.onStateChange?.(state);
  }

  private assertUsable(): void {
    if (this.disposed) {
      throw new Error('模拟器实例已销毁。');
    }
    if (!this.canvas.isConnected) {
      console.warn('画布当前不在文档中，画面将不会被显示。');
    }
  }
}

function describe(error: unknown): string {
  if (error instanceof Error) return error.message;
  return String(error);
}
