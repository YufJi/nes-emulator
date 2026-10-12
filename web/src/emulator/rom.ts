import { SUPPORTED_MAPPERS } from './constants';

const NES_MAGIC = [0x4e, 0x45, 0x53, 0x1a] as const;
const INES_MIN_HEADER_SIZE = 16;
/** Generous ceiling: the largest valid iNES images are a few megabytes. */
const MAX_ROM_BYTES = 16 * 1024 * 1024;
/** Give up on a slow server rather than leave the UI stuck on "loading". */
const ROM_FETCH_TIMEOUT_MS = 30_000;

/** Error type used for anything the user can fix by picking a different file. */
export class RomError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'RomError';
  }
}

/** Identity of a cartridge image the host handed to the core. */
export interface RomSource {
  /** Name shown in the UI. */
  name: string;
  /**
   * Stable identity used to key battery saves and save states. A local file is
   * keyed by its file name, a downloaded one by its normalized URL, so the two
   * never share (or overwrite) each other's data.
   */
  key: string;
}

/** True when the file name carries a cartridge-image extension. */
export function isRomFileName(name: string): boolean {
  return /\.(nes|fds|unf|unif|bin)$/i.test(name);
}

/** The source a picked/dropped local file maps to. */
export function fileRomSource(file: File): RomSource {
  return { name: file.name, key: file.name };
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
 * Turn whatever the user typed into an absolute http(s) URL.
 *
 * A paste usually arrives without a scheme ("example.com/game.nes"), and https
 * is what people mean in that case, so add it instead of rejecting the input.
 */
export function parseRomUrl(raw: string): URL {
  const trimmed = raw.trim();
  if (!trimmed) {
    throw new RomError('请先填入 ROM 链接。');
  }

  const withScheme = /^[a-z][a-z0-9+.-]*:/i.test(trimmed) ? trimmed : `https://${trimmed}`;
  let url: URL;
  try {
    url = new URL(withScheme);
  } catch {
    throw new RomError(`“${trimmed}” 不是有效的链接。`);
  }

  if (url.protocol !== 'http:' && url.protocol !== 'https:') {
    throw new RomError(`只能加载 http(s) 直链，暂不支持 ${url.protocol}// 链接。`);
  }
  return url;
}

/** Name a downloaded image after the last path segment of its URL. */
export function romNameFromUrl(url: URL): string {
  const segment = url.pathname.split('/').filter(Boolean).pop();
  if (!segment) return url.hostname;

  try {
    // URLs spell non-ASCII names as escapes; show the real name to the user.
    return decodeURIComponent(segment);
  } catch {
    return segment;
  }
}

/**
 * Point GitHub links at the raw host, which is the same file served as a plain
 * downloadable response.
 *
 * Both `github.com/<o>/<r>/blob/…` (what the file viewer gives you) and
 * `github.com/<o>/<r>/raw/…` (what the Raw button gives you) are web routes.
 * The latter just redirects to `raw.githubusercontent.com`, and that redirect
 * carries no `Access-Control-Allow-Origin`, so a cross-origin browser fetch is
 * dropped and all the user sees is "Failed to fetch". The raw host serves the
 * same bytes with `Access-Control-Allow-Origin: *`.
 */
function toCorsFriendlyUrl(url: URL): URL {
  if (url.hostname !== 'github.com' && url.hostname !== 'www.github.com') return url;

  // Path segments keep their escapes, so non-ASCII file names survive as-is.
  const [owner, repo, route, ...rest] = url.pathname.split('/').filter(Boolean);
  if (!owner || !repo || (route !== 'raw' && route !== 'blob') || rest.length === 0) return url;

  const rewritten = new URL(`https://raw.githubusercontent.com/${owner}/${repo}/${rest.join('/')}`);
  rewritten.search = url.search;
  return rewritten;
}

/**
 * Download a cartridge image from a URL and validate it.
 *
 * Remote images go through exactly the same checks as local files, so a link
 * that points at something other than a ROM fails with a readable message.
 */
export async function fetchRomFromUrl(
  raw: string,
): Promise<{ source: RomSource; data: Uint8Array }> {
  const url = toCorsFriendlyUrl(parseRomUrl(raw));
  const target = url.toString();
  const name = romNameFromUrl(url);

  // The timeout covers the whole download, body included, so a server that
  // stalls half way through cannot leave the UI loading forever.
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), ROM_FETCH_TIMEOUT_MS);
  try {
    // Redirects are common for ROM links (e.g. GitHub's `raw/` URLs), so follow
    // them rather than treating the 3xx as an error.
    const response = await fetch(target, { signal: controller.signal, redirect: 'follow' });

    if (!response.ok) {
      // HTTP/2 responses often carry no reason phrase, so do not leave a gap.
      const status = response.statusText
        ? `${response.status} ${response.statusText}`
        : `${response.status}`;
      throw new RomError(`下载失败：服务器返回 ${status}。`);
    }

    // A web page instead of a file almost always means the link is a viewer URL
    // rather than the file itself, so say so before spending bandwidth on it.
    const contentType = response.headers.get('content-type') ?? '';
    if (contentType.includes('text/html')) {
      throw new RomError(`“${target}” 返回的是网页而不是 ROM 文件，请改用文件的直链。`);
    }

    const declaredSize = Number(response.headers.get('content-length') ?? 0);
    if (Number.isFinite(declaredSize) && declaredSize > MAX_ROM_BYTES) {
      throw oversizeError(declaredSize);
    }

    const buffer = await response.arrayBuffer();
    if (buffer.byteLength === 0) {
      throw new RomError(`“${target}” 是空文件。`);
    }
    if (buffer.byteLength > MAX_ROM_BYTES) {
      throw oversizeError(buffer.byteLength);
    }

    const data = new Uint8Array(buffer);
    validateRom(data, name);
    return { source: { name, key: target }, data };
  } catch (cause) {
    // Messages that already tell the user what to fix pass through untouched.
    if (cause instanceof RomError) throw cause;
    if (controller.signal.aborted) {
      throw new RomError(`下载超时（${ROM_FETCH_TIMEOUT_MS / 1000} 秒无响应）：${target}`);
    }
    throw new RomError(`无法下载“${target}”：${describeFetchFailure(cause)}`);
  } finally {
    clearTimeout(timer);
  }
}

function oversizeError(bytes: number): RomError {
  return new RomError(
    `该链接有 ${(bytes / 1024 / 1024).toFixed(1)} MB，远超 NES ROM 的正常体积。`,
  );
}

/**
 * Cross-origin and offline failures both surface as "Failed to fetch", so point
 * the user at the two things they can actually check.
 */
function describeFetchFailure(cause: unknown): string {
  const detail = cause instanceof Error && cause.message ? `${cause.message}；` : '';
  return `${detail}链接可能已失效，或该服务器未允许跨域访问（CORS）。`;
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
