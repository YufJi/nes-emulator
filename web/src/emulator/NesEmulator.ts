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
import {
  BUTTON_NAMES,
  TURBO_PERIOD_FRAMES,
  isTurbo,
  turboTarget,
  type ButtonName,
  type PadInput,
} from './constants';
import { coreInfo, initWasm, wasmMemoryBuffer } from './core';
import { t } from '../i18n';

export type EmulatorState = 'idle' | 'running' | 'paused';

export interface NesEmulatorOptions {
  onStateChange?: (state: EmulatorState) => void;
  onFpsChange?: (fps: number) => void;
  /**
   * Called when the core traps mid-instruction and the console has been thrown
   * away.
   *
   * A panic in the WASM core aborts the module, so that instance can never be
   * used again — the host should say so and let the user load again, which
   * builds a fresh console.
   */
  onFatalError?: (message: string) => void;
  /**
   * Called once when a KIL opcode stops the CPU.
   *
   * The machine is still alive — frames keep completing — but nothing more is
   * fetched, so the picture will not change again. Not worth throwing the
   * console away for; just tell the player and let them reload.
   */
  onCpuHalted?: () => void;
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
  /** Scratch buffer the core fills with audio, reused every frame. */
  private readonly audioChunk = new Float32Array(8192);

  private nes: Nes | null = null;
  private buttons: Record<ButtonName, number> | null = null;
  /** Buttons the player is holding, as opposed to the ones turbo is firing. */
  private readonly held = new Set<ButtonName>();
  /** Buttons whose turbo control is held down. */
  private readonly turbo = new Set<ButtonName>();
  /** Frames emulated since the turbo controls started; the pulse's phase. */
  private turboFrame = 0;
  /** Core-owned pacer: catch-up budget, backpressure and FPS all live in Rust. */
  private clock: FrameClock | null = null;

  private rafId: number | null = null;
  private lastReportedFps = 0;
  private framesSinceSaveCheck = 0;
  private lastSaveRam: Uint8Array | null = null;

  private currentState: EmulatorState = 'idle';
  private disposed = false;
  /** Whether the hung-CPU message has already been sent for this cartridge. */
  private cpuHaltReported = false;

  constructor(canvas: HTMLCanvasElement, options: NesEmulatorOptions = {}) {
    this.canvas = canvas;
    this.options = options;

    const context = canvas.getContext('2d', { alpha: false });
    if (!context) {
      throw new Error(t('error.canvasUnavailable'));
    }
    this.context = context;

    // Render at the console's native resolution — taken from the core so the
    // host never repeats it — and let CSS scale the element up with integer
    // factors, cheaper and crisper than scaling in JS.
    const { width, height } = coreInfo();
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

  /**
   * Load a validated ROM image and start running it.
   *
   * `saveRam` is the cartridge's stored battery RAM, written back before the
   * first frame: boot code may read its save as early as that, and a frame
   * emulated against blank SRAM would look like "no save" to the game.
   *
   * Throws when the cartridge is rejected, including when its very first frame
   * makes the core panic.
   */
  async loadRom(data: Uint8Array, saveRam: Uint8Array | null = null): Promise<void> {
    await initWasm();
    this.assertUsable();

    // Build the new instance before touching the old one so a failed load
    // leaves the currently running game untouched.
    const next = new Nes();
    try {
      next.load_rom(data);
    } catch (error) {
      freeQuietly(next);
      throw new Error(t('error.coreRejected', { reason: describe(error) }));
    }

    this.nes?.free();
    this.nes = next;
    this.cpuHaltReported = false;
    if (saveRam) {
      next.set_save_ram(saveRam);
    }
    // Seed the change detector: the restored bytes (or none) are the baseline,
    // so the first poll does not treat them as a change and write them back.
    this.lastSaveRam = saveRam;
    this.buttons = readButtonIndices();

    // Pace at the cartridge's own rate — PAL runs at ~50.007 Hz — and start
    // from a clean clock so the load itself is not emulated.
    this.clock?.free();
    this.clock = new FrameClock(next.get_frame_rate());
    this.lastReportedFps = 0;

    this.audio.flush();

    // Emulate one frame up front so the first paint is not a blank screen. A
    // cartridge that panics on that frame is reported as a failed load.
    try {
      this.stepFrames(1);
    } catch (error) {
      throw new Error(t('error.coreRejected', { reason: describe(error) }));
    }
    this.render();
    this.reportCpuHalt();

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
    try {
      this.nes.reset();
      this.audio.flush();
      this.clock?.reset();
      this.stepFrames(1);
      this.render();
      this.reportCpuHalt();
    } catch (error) {
      this.handleCorePanic(error);
    }
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

  setButton(input: PadInput, pressed: boolean): void {
    if (isTurbo(input)) {
      this.setTurbo(turboTarget(input), pressed);
      return;
    }
    this.setHeld(input, pressed);
  }

  /**
   * Press or release a button the player is working by hand.
   *
   * Applied at once rather than at the next frame: the core latches the
   * controller only when the game strobes it, so there is nothing to gain from
   * waiting, and a press that lands between two frames is still seen by the
   * frame that follows.
   */
  private setHeld(button: ButtonName, pressed: boolean): void {
    if (pressed) this.held.add(button);
    else this.held.delete(button);
    this.applyButton(button, pressed);
  }

  /** Turn a turbo control on or off; the pulse itself runs in the frame loop. */
  private setTurbo(button: ButtonName, pressed: boolean): void {
    if (pressed) {
      // Begin the cycle on the press, so the first shot lands on the next frame
      // rather than waiting out whatever half of the cycle was left over.
      if (this.turbo.size === 0) this.turboFrame = 0;
      this.turbo.add(button);
      return;
    }

    this.turbo.delete(button);
    // Let the button go the moment the turbo control is released, or the last
    // pulse would keep firing until the next frame looks at it again.
    if (!this.held.has(button)) this.applyButton(button, false);
  }

  /** Release every button, e.g. when the window loses focus mid-press. */
  releaseAllButtons(): void {
    this.held.clear();
    this.turbo.clear();
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
    this.discardConsole();
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

    try {
      this.stepFrames(frames);
    } catch (error) {
      this.handleCorePanic(error);
      return;
    }

    this.render();
    this.pumpAudio();
    this.reportFps();
    this.reportCpuHalt();
  };

  /**
   * Emulate `count` frames from the current clock tick.
   *
   * Throws if the core reports a panic; the callers decide what that means —
   * a failed load, or the running loop shutting down.
   */
  private stepFrames(count: number): void {
    for (let i = 0; i < count; i += 1) {
      this.applyTurbo();
      this.nes?.frame();
    }
    this.pollSaveRam(count);
  }

  /**
   * Set the turbo buttons' state for the frame about to run.
   *
   * The phase is counted in emulated frames rather than in wall-clock time: it
   * is what the console is being told that matters, and a frame counter keeps a
   * replay of the same inputs identical to the run it came from. A button the
   * player is holding by hand is left alone — the manual press wins, and the
   * pulse picks up again on the next frame after it is released.
   */
  private applyTurbo(): void {
    const phase = this.turboFrame % TURBO_PERIOD_FRAMES;
    this.turboFrame += 1;
    if (this.turbo.size === 0) return;

    const pressed = phase < TURBO_PERIOD_FRAMES / 2;
    for (const button of this.turbo) {
      if (!this.held.has(button)) this.applyButton(button, pressed);
    }
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

  /**
   * Blit the core's frame buffer into the canvas, honouring its pixel layout.
   *
   * The pixels are read straight out of the core's own memory instead of
   * through a copy of it, and written into the canvas image one 32-bit pixel at
   * a time. The canvas still needs RGBA while the core stores `0xAARRGGBB`
   * (i.e. BGRA in memory), so this swizzle cannot be skipped without moving the
   * picture to WebGL — worth doing if this ever shows up in a profile.
   */
  private render(): void {
    const nes = this.nes;
    if (!nes) return;

    const [redShift, greenShift, blueShift, alphaShift] = coreInfo().pixelShifts;
    // Re-read the pointer every frame: the buffer is the core's, and
    // `load_state` may move it.
    const pixels = new Uint32Array(
      wasmMemoryBuffer(),
      nes.get_frame_buffer_ptr(),
      nes.get_frame_buffer_len(),
    );
    const target = new Uint32Array(this.imageData.data.buffer);
    const count = Math.min(pixels.length, target.length);

    for (let i = 0; i < count; i += 1) {
      const pixel = pixels[i];
      target[i] =
        ((pixel >>> redShift) & 0xff) |
        (((pixel >>> greenShift) & 0xff) << 8) |
        (((pixel >>> blueShift) & 0xff) << 16) |
        (((pixel >>> alphaShift) & 0xff) << 24);
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

  /**
   * Tell the host once when a KIL opcode has hung the CPU.
   *
   * The machine keeps running and frames keep completing, but no instruction is
   * fetched any more, so the picture is frozen. That is not a reason to throw
   * the console away — the player just needs to know.
   */
  private reportCpuHalt(): void {
    if (this.cpuHaltReported || !this.nes?.is_cpu_halted()) return;

    this.cpuHaltReported = true;
    this.options.onCpuHalted?.();
  }

  /**
   * The core panicked mid-instruction: a bug in the core rather than anything
   * the cartridge did (undefined opcodes hang the CPU instead).
   *
   * `wasm32-unknown-unknown` aborts on panic, so Rust cannot catch this — a
   * `catch_unwind` there never runs, the trap surfaces as a JS exception and the
   * instance is left borrowed. The page survives, but the console is spent:
   * throw it away, stop the loop and let the host tell the user. The next
   * `loadRom` builds a fresh one.
   */
  private handleCorePanic(error: unknown): void {
    this.stopLoop();
    this.audio.flush();
    this.discardConsole();
    // Nothing can advance the machine any more, so show it as stopped.
    this.setState('paused');
    this.options.onFatalError?.(describe(error));
  }

  /** Drop the console and everything tied to it. */
  private discardConsole(): void {
    freeQuietly(this.nes);
    this.nes = null;
    this.buttons = null;
    this.clock?.free();
    this.clock = null;
    this.lastSaveRam = null;
  }

  private setState(state: EmulatorState): void {
    if (this.currentState === state) return;
    this.currentState = state;
    this.options.onStateChange?.(state);
  }

  private assertUsable(): void {
    if (this.disposed) {
      throw new Error(t('error.disposed'));
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

/**
 * Free a WASM handle, tolerating failure.
 *
 * A console the core has trapped in is still borrowed, and wasm-bindgen throws
 * when asked to take ownership of a borrowed object. Its memory goes back with
 * the module, so the only thing left to do is move on.
 */
function freeQuietly(nes: Nes | null): void {
  try {
    nes?.free();
  } catch {
    // See above: a trapped instance cannot be freed.
  }
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
