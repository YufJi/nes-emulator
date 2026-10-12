import {
  get_button_a,
  get_button_b,
  get_button_down,
  get_button_left,
  get_button_right,
  get_button_select,
  get_button_start,
  get_button_up,
  FrameClock,
  Nes,
} from '../wasm/nes_wasm.js';
import { AudioOutput } from './AudioOutput';
import { BUTTON_NAMES, type ButtonName } from './constants';
import { coreInfo, initWasm } from './core';

export type EmulatorState = 'idle' | 'running' | 'paused';

export interface NesEmulatorOptions {
  onStateChange?: (state: EmulatorState) => void;
  onFpsChange?: (fps: number) => void;
  /** Called whenever the cartridge's battery RAM changes. */
  onSaveRamChange?: (data: Uint8Array) => void;
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
  private readonly width: number;
  private readonly height: number;
  /** Scratch buffer the core fills with audio, reused every frame. */
  private readonly audioChunk = new Float32Array(8192);

  private nes: Nes | null = null;
  private buttons: Record<ButtonName, number> | null = null;
  /** Core-owned pacer: catch-up budget, backpressure and FPS all live in Rust. */
  private clock: FrameClock | null = null;

  private rafId: number | null = null;
  private lastReportedFps = 0;
  private framesSinceSaveCheck = 0;
  private lastSaveRam: Uint8Array | null = null;

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

    // Render at the console's native resolution — taken from the core so the
    // host never repeats it — and let CSS scale the element up with integer
    // factors, cheaper and crisper than scaling in JS.
    const { width, height } = coreInfo();
    this.width = width;
    this.height = height;
    canvas.width = width;
    canvas.height = height;
    context.imageSmoothingEnabled = false;
    this.imageData = context.createImageData(width, height);
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
    const next = new Nes();
    try {
      next.load_rom(data);
    } catch (error) {
      next.free();
      throw new Error(`模拟器内核拒绝了该 ROM：${describe(error)}`);
    }

    this.nes?.free();
    this.nes = next;
    this.buttons = readButtonIndices();

    // Pace at the cartridge's own rate — PAL runs at ~50.007 Hz — and start
    // from a clean clock so the load itself is not emulated.
    this.clock?.free();
    this.clock = new FrameClock(next.get_frame_rate());
    this.lastReportedFps = 0;

    this.audio.flush();

    // Emulate one frame up front so the first paint is not a blank screen.
    this.stepFrames(1);
    this.render();

    this.setState('running');
    this.startLoop();
    void this.audio
      .ensureStarted()
      .then(() => this.matchDeviceSampleRate())
      .catch((error) => {
        console.warn('音频初始化失败，将继续静音运行。', error);
      });
  }

  /** Warp back to the cartridge's reset vector. */
  reset(): void {
    if (!this.nes) return;
    this.nes.reset();
    this.audio.flush();
    this.clock?.reset();
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
    // The pause was not emulation time, so restart the pacing from here.
    this.clock?.reset();
    this.setState('running');
    this.startLoop();
    void this.audio
      .ensureStarted()
      .then(() => this.matchDeviceSampleRate())
      .catch(() => undefined);
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
    void this.audio
      .ensureStarted()
      .then(() => this.matchDeviceSampleRate())
      .catch(() => undefined);
  }

  /**
   * Ask the core to resample to the audio device's own rate.
   *
   * The device rate is only known once the audio graph exists — and it can
   * change, as it does when a phone switches output route — so this runs after
   * every successful start rather than once at construction.
   */
  private matchDeviceSampleRate(): void {
    const nes = this.nes;
    const deviceRate = this.audio.sampleRate;
    if (!nes || deviceRate <= 0 || deviceRate === nes.get_audio_sample_rate()) return;

    nes.set_audio_sample_rate(deviceRate);
  }

  dispose(): void {
    this.disposed = true;
    this.stopLoop();
    this.nes?.free();
    this.nes = null;
    this.buttons = null;
    this.clock?.free();
    this.clock = null;
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
   * Emulation clock. The core's `FrameClock` turns the animation-frame
   * timestamp into a whole number of frames — absorbing catch-up bursts and
   * dropping what it cannot repay — so the game follows the console's own rate
   * rather than the display's refresh rate.
   */
  private readonly tick = (timestamp: number): void => {
    this.rafId = requestAnimationFrame(this.tick);

    if (this.currentState !== 'running' || !this.nes) return;

    const frames = this.clock?.frames_due(timestamp) ?? 0;
    if (frames === 0) return;

    this.stepFrames(frames);
    this.render();
    this.pumpAudio();
    this.reportFps();
  };

  private stepFrames(count: number): void {
    for (let i = 0; i < count; i += 1) {
      this.nes?.frame();
    }
    this.pollSaveRam(count);
  }

  /**
   * Battery RAM can only change while the emulated console runs, so poll it a
   * few times per second and hand any change to the host.
   */
  private pollSaveRam(frames: number): void {
    const nes = this.nes;
    const handler = this.options.onSaveRamChange;
    if (!nes || !handler) return;

    this.framesSinceSaveCheck += frames;
    if (this.framesSinceSaveCheck < SAVE_RAM_POLL_FRAMES) return;
    this.framesSinceSaveCheck = 0;

    if (!nes.has_battery()) return;

    const current = nes.get_save_ram();
    if (this.lastSaveRam && sameBytes(this.lastSaveRam, current)) return;
    this.lastSaveRam = current;
    handler(current);
  }

  /** Snapshot of the cartridge's battery RAM, or null when it has none. */
  getSaveRam(): Uint8Array | null {
    if (!this.nes?.has_battery()) return null;
    return this.nes.get_save_ram();
  }

  /** Whether the loaded cartridge keeps battery-backed save RAM. */
  hasBattery(): boolean {
    return this.nes?.has_battery() ?? false;
  }

  /** Restore a previously saved snapshot into the cartridge. */
  setSaveRam(data: Uint8Array): void {
    this.nes?.set_save_ram(data);
    this.lastSaveRam = null;
  }

  /** Serialize the whole machine into a save state, or null before loading. */
  saveState(): Uint8Array | null {
    return this.nes?.save_state() ?? null;
  }

  /** Restore a state produced by {@link NesEmulator.saveState}. */
  loadState(data: Uint8Array): void {
    this.nes?.load_state(data);
  }

  /**
   * Blank the cartridge's battery RAM in memory.
   *
   * Persistence belongs to the host, so this only wipes the live cartridge;
   * the caller deletes the stored copy. The blank snapshot is remembered so
   * the save poll does not treat the wipe as a change and write it straight
   * back to storage.
   */
  clearSaveRam(): void {
    const current = this.getSaveRam();
    if (!current) return;

    const blank = new Uint8Array(current.length);
    this.nes?.set_save_ram(blank);
    this.lastSaveRam = blank;
  }

  /** Push a button state straight through to the emulated controller. */
  private applyButton(button: ButtonName, pressed: boolean): void {
    const index = this.buttons?.[button];
    if (index === undefined) return;
    this.nes?.set_button(1, index, pressed);
  }

  /** Blit the core's frame buffer into the canvas, honouring its pixel layout. */
  private render(): void {
    if (!this.nes) return;

    const [redShift, greenShift, blueShift, alphaShift] = coreInfo().pixelShifts;
    const pixels = this.nes.get_frame_buffer();
    const target = this.imageData.data;
    const count = Math.min(pixels.length, this.width * this.height);

    for (let i = 0, j = 0; i < count; i += 1, j += 4) {
      const pixel = pixels[i];
      target[j] = (pixel >>> redShift) & 0xff;
      target[j + 1] = (pixel >>> greenShift) & 0xff;
      target[j + 2] = (pixel >>> blueShift) & 0xff;
      target[j + 3] = (pixel >>> alphaShift) & 0xff;
    }

    this.context.putImageData(this.imageData, 0, 0);
  }

  private pumpAudio(): void {
    const nes = this.nes;
    if (!nes) return;

    // Pull a device period's worth out of the core; a quiet frame can bring
    // back nothing, and a busy one can leave a remainder for the next tick.
    const written = nes.fill_audio(this.audioChunk);
    if (written > 0) {
      this.audio.enqueue(this.audioChunk.subarray(0, written));
    }
  }

  /** Hand the core's measured frame rate to the UI whenever it changes. */
  private reportFps(): void {
    const fps = Math.round(this.clock?.fps() ?? 0);
    if (fps <= 0 || fps === this.lastReportedFps) return;

    this.lastReportedFps = fps;
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

/** Poll battery RAM roughly twice a second. */
const SAVE_RAM_POLL_FRAMES = 30;

function sameBytes(a: Uint8Array, b: Uint8Array): boolean {
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i += 1) {
    if (a[i] !== b[i]) return false;
  }
  return true;
}
