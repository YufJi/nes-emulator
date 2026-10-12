import init, {
  get_frame_height,
  get_frame_width,
  get_pixel_format,
  get_pixel_shifts,
  get_supported_mappers,
} from '../wasm/nes_wasm.js';

let wasmInit: Promise<unknown> | null = null;

/** Instantiate the WASM module once, no matter how many callers ask for it. */
export function initWasm(): Promise<unknown> {
  wasmInit ??= init();
  return wasmInit;
}

/**
 * Screen and cartridge contract published by the Rust core.
 *
 * These values are the single source of truth — the host repeats neither the
 * frame resolution, the pixel layout nor the list of supported mappers.
 */
export interface CoreInfo {
  /** Frame width in pixels. */
  width: number;
  /** Frame height in pixels. */
  height: number;
  /** Pixel layout id, e.g. `"argb8888"`. */
  pixelFormat: string;
  /** Bit position of each channel in a pixel, `[red, green, blue, alpha]`. */
  pixelShifts: readonly number[];
  /** Mapper numbers the core can emulate. */
  supportedMappers: readonly number[];
}

let cached: CoreInfo | null = null;

/** Read the core's contract; the WASM module must be initialized first. */
export function readCoreInfo(): CoreInfo {
  if (cached) return cached;

  const pixelFormat = get_pixel_format();
  const pixelShifts = Array.from(get_pixel_shifts());
  // The blit loop destructures four channels, so a layout the host does not
  // understand must fail loudly here rather than paint garbage.
  if (pixelShifts.length !== 4) {
    throw new Error(`内核返回了无法识别的像素格式：${pixelFormat}`);
  }

  cached = {
    width: get_frame_width(),
    height: get_frame_height(),
    pixelFormat,
    pixelShifts,
    supportedMappers: Array.from(get_supported_mappers()),
  };
  return cached;
}

/** The contract cached by {@link readCoreInfo}, available once WASM is ready. */
export function coreInfo(): CoreInfo {
  if (!cached) {
    throw new Error('WASM 内核尚未初始化，无法读取画面规格。');
  }
  return cached;
}
