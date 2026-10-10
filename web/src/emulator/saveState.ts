/**
 * Persistence for save states (instant snapshots of the whole machine).
 *
 * States are far larger than battery saves — a few hundred kilobytes once the
 * frame buffers and cartridge RAM are included — so they live in IndexedDB
 * rather than localStorage. Entries are keyed by ROM file name plus slot, the
 * same identity scheme the battery-save store uses.
 */
const DB_NAME = 'nes-emulator';
const DB_VERSION = 1;
const STORE = 'save-states';

/** Number of quick-save slots offered in the UI. */
export const SAVE_STATE_SLOTS = 4;

function slotKey(romName: string, slot: number): string {
  return `${romName}#${slot}`;
}

function openDatabase(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open(DB_NAME, DB_VERSION);
    request.onupgradeneeded = () => {
      if (!request.result.objectStoreNames.contains(STORE)) {
        request.result.createObjectStore(STORE);
      }
    };
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
}

async function runRequest<T>(
  mode: IDBTransactionMode,
  run: (store: IDBObjectStore) => IDBRequest<T>,
): Promise<T> {
  const db = await openDatabase();
  try {
    return await new Promise<T>((resolve, reject) => {
      const request = run(db.transaction(STORE, mode).objectStore(STORE));
      request.onsuccess = () => resolve(request.result);
      request.onerror = () => reject(request.error);
    });
  } finally {
    db.close();
  }
}

/** Store a snapshot in a slot. Best effort: storage failures are ignored. */
export async function writeState(
  romName: string,
  slot: number,
  data: Uint8Array,
): Promise<void> {
  try {
    await runRequest('readwrite', (store) => store.put(data, slotKey(romName, slot)));
  } catch {
    // IndexedDB unavailable or full: losing the state is not fatal.
  }
}

/** Read a slot, or null when it holds nothing. */
export async function readState(romName: string, slot: number): Promise<Uint8Array | null> {
  try {
    const value = await runRequest<Uint8Array | undefined>('readonly', (store) =>
      store.get(slotKey(romName, slot)),
    );
    return value ?? null;
  } catch {
    return null;
  }
}

/** Delete a slot. */
export async function clearState(romName: string, slot: number): Promise<void> {
  try {
    await runRequest('readwrite', (store) => store.delete(slotKey(romName, slot)));
  } catch {
    // Nothing to do.
  }
}

/** Which slots currently hold a state for this ROM. */
export async function slotsFilled(romName: string): Promise<boolean[]> {
  const checks = Array.from({ length: SAVE_STATE_SLOTS }, (_, slot) =>
    runRequest<IDBValidKey | undefined>('readonly', (store) => store.getKey(slotKey(romName, slot)))
      .then((key) => key !== undefined)
      .catch(() => false),
  );
  return Promise.all(checks);
}
