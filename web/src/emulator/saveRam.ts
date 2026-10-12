/**
 * Persistence for battery-backed cartridge RAM.
 *
 * Games without a battery report an empty snapshot from the core, so the host
 * only ever stores data for cartridges that actually save. Entries are keyed by
 * the cartridge's source identity (`RomSource.key`): its file name for a local
 * file, its URL for one loaded from a link.
 */
const STORAGE_PREFIX = 'nes-save:';

/** Read a previously stored save, or null when there is nothing usable. */
export function readSaveRam(romKey: string): Uint8Array | null {
  try {
    const encoded = localStorage.getItem(STORAGE_PREFIX + romKey);
    if (!encoded) return null;

    const binary = atob(encoded);
    const bytes = new Uint8Array(binary.length);
    for (let i = 0; i < binary.length; i += 1) {
      bytes[i] = binary.charCodeAt(i);
    }
    return bytes;
  } catch {
    // Corrupt entry or storage unavailable: behave as if nothing was saved.
    return null;
  }
}

/** Store the cartridge's battery RAM. Best effort: failures are ignored. */
export function writeSaveRam(romKey: string, data: Uint8Array): void {
  try {
    let binary = '';
    for (let i = 0; i < data.length; i += 1) {
      binary += String.fromCharCode(data[i]);
    }
    localStorage.setItem(STORAGE_PREFIX + romKey, btoa(binary));
  } catch {
    // Quota exceeded or storage disabled: losing the save is not fatal.
  }
}

/** Forget a stored save, e.g. when the user wants a fresh start. */
export function clearSaveRam(romKey: string): void {
  try {
    localStorage.removeItem(STORAGE_PREFIX + romKey);
  } catch {
    // Nothing to do.
  }
}
