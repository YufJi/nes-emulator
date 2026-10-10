import { SUPPORTED_MAPPERS } from './constants';

const NES_MAGIC = [0x4e, 0x45, 0x53, 0x1a] as const;
const INES_MIN_HEADER_SIZE = 16;
/** Generous ceiling: the largest valid iNES images are a few megabytes. */
const MAX_ROM_BYTES = 16 * 1024 * 1024;

/** Error type used for anything the user can fix by picking a different file. */
export class RomError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'RomError';
  }
}

/** True when the file name carries a cartridge-image extension. */
export function isRomFileName(name: string): boolean {
  return /\.(nes|fds|unf|unif|bin)$/i.test(name);
}

/** Read a dropped/selected file and validate it before handing it to WASM. */
export async function readRomFile(file: File): Promise<Uint8Array> {
  if (file.size === 0) {
    throw new RomError(`“${file.name}” 是空文件。`);
  }
  if (file.size > MAX_ROM_BYTES) {
    throw new RomError(
      `“${file.name}” 有 ${(file.size / 1024 / 1024).toFixed(1)} MB，远超 NES ROM 的正常体积。`,
    );
  }

  const data = new Uint8Array(await file.arrayBuffer());
  validateRom(data, file.name);
  return data;
}

/**
 * Validate the iNES header up front so the user gets a readable message
 * instead of a WASM panic or a silently blank screen.
 */
export function validateRom(data: Uint8Array, name = 'ROM'): void {
  if (data.length < INES_MIN_HEADER_SIZE) {
    throw new RomError(`“${name}” 太小（${data.length} 字节），不是 NES ROM 文件。`);
  }

  const hasMagic = NES_MAGIC.every((byte, index) => data[index] === byte);
  if (!hasMagic) {
    const hint = isRomFileName(name)
      ? '文件头缺少 NES\\x1a 标识，文件可能已损坏。'
      : '扩展名不是 .nes，如果是压缩包请先解压。';
    throw new RomError(`“${name}” 不是 iNES 格式文件：${hint}`);
  }

  if (data.length >= 8 && /^(NESM|Unif)/.test(String.fromCharCode(...data.subarray(0, 4)))) {
    throw new RomError(`“${name}” 使用了 UNIF/NES 2.0 容器，当前仅支持 iNES 格式。`);
  }

  const mapper = readMapperNumber(data);
  if (!SUPPORTED_MAPPERS.includes(mapper)) {
    throw new RomError(
      `“${name}” 使用了 Mapper ${mapper}，当前内核尚未实现该 Mapper（已支持 ${SUPPORTED_MAPPERS.join(', ')}）。`,
    );
  }
}

/** Mapper number encoded in the high nibbles of header bytes 6 and 7. */
export function readMapperNumber(data: Uint8Array): number {
  return ((data[7] ?? 0) & 0xf0) | (((data[6] ?? 0) & 0xf0) >> 4);
}
