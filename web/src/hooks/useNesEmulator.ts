import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type RefObject,
} from 'react';
import { KEY_BINDINGS, type ButtonName } from '../emulator/constants';
import { NesEmulator, initWasm, type EmulatorState } from '../emulator/NesEmulator';
import { fetchRomFromUrl, fileRomSource, readRomFile, type RomSource } from '../emulator/rom';
import { clearSaveRam, readSaveRam, writeSaveRam } from '../emulator/saveRam';
import {
  SAVE_STATE_SLOTS,
  clearState,
  readState,
  slotsFilled,
  writeState,
} from '../emulator/saveState';

export interface UseNesEmulatorResult {
  canvasRef: RefObject<HTMLCanvasElement>;
  state: EmulatorState;
  /** Name of the loaded cartridge, for display. */
  romName: string | null;
  /** Where the cartridge came from: its file name or its URL. */
  romKey: string | null;
  error: string | null;
  fps: number;
  muted: boolean;
  busy: boolean;
  /** Whether the loaded cartridge keeps battery-backed save RAM. */
  hasBattery: boolean;
  /** Which quick-save slots hold a state for the current ROM. */
  saveSlots: boolean[];
  loadFile: (file: File) => Promise<void>;
  loadUrl: (url: string) => Promise<void>;
  reset: () => void;
  clearSave: () => void;
  saveSlot: (slot: number) => void;
  loadSlot: (slot: number) => void;
  clearSlot: (slot: number) => void;
  togglePause: () => void;
  toggleMute: () => void;
  clearError: () => void;
  setButton: (button: ButtonName, pressed: boolean) => void;
  wakeAudio: () => void;
}

/**
 * Bridges the imperative {@link NesEmulator} to React state.
 *
 * The emulator instance lives in a ref so re-renders never restart the game
 * loop; only the small amount of UI state below is kept in React.
 */
export function useNesEmulator(): UseNesEmulatorResult {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const emulatorRef = useRef<NesEmulator | null>(null);

  const [state, setState] = useState<EmulatorState>('idle');
  const [romName, setRomName] = useState<string | null>(null);
  const [romKey, setRomKey] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [fps, setFps] = useState(0);
  const [muted, setMuted] = useState(false);
  const [busy, setBusy] = useState(true);
  const [hasBattery, setHasBattery] = useState(false);
  const [saveSlots, setSaveSlots] = useState<boolean[]>(() =>
    Array(SAVE_STATE_SLOTS).fill(false),
  );
  // The save-RAM callback is created once with the emulator, so it reads the
  // current ROM identity through a ref instead of a captured value.
  const romKeyRef = useRef<string | null>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;

    const emulator = new NesEmulator(canvas, {
      onStateChange: setState,
      onFpsChange: setFps,
      onSaveRamChange: (data) => {
        const key = romKeyRef.current;
        if (key) writeSaveRam(key, data);
      },
    });
    emulatorRef.current = emulator;

    let cancelled = false;
    initWasm()
      .then(() => {
        if (!cancelled) setBusy(false);
      })
      .catch((cause: unknown) => {
        if (cancelled) return;
        setBusy(false);
        setError(`WASM 模块加载失败：${cause instanceof Error ? cause.message : String(cause)}`);
      });

    return () => {
      cancelled = true;
      emulator.dispose();
      emulatorRef.current = null;
    };
  }, []);

  useEffect(() => {
    const held = new Set<string>();

    const matchesTextField = (target: EventTarget | null): boolean =>
      target instanceof HTMLElement &&
      (target.isContentEditable ||
        target.tagName === 'INPUT' ||
        target.tagName === 'TEXTAREA' ||
        target.tagName === 'SELECT');

    const onKeyDown = (event: KeyboardEvent) => {
      const button = KEY_BINDINGS[event.code];
      if (!button || matchesTextField(event.target)) return;

      event.preventDefault();
      held.add(event.code);
      const emulator = emulatorRef.current;
      emulator?.wakeAudio();
      if (event.repeat) return;
      emulator?.setButton(button, true);
    };

    const onKeyUp = (event: KeyboardEvent) => {
      const button = KEY_BINDINGS[event.code];
      if (!button || matchesTextField(event.target)) return;
      event.preventDefault();
      held.delete(event.code);
      emulatorRef.current?.setButton(button, false);
    };

    // Keys held while the window loses focus would otherwise stay pressed.
    const releaseAll = () => {
      held.clear();
      emulatorRef.current?.releaseAllButtons();
    };

    window.addEventListener('keydown', onKeyDown);
    window.addEventListener('keyup', onKeyUp);
    window.addEventListener('blur', releaseAll);
    document.addEventListener('visibilitychange', releaseAll);

    return () => {
      window.removeEventListener('keydown', onKeyDown);
      window.removeEventListener('keyup', onKeyUp);
      window.removeEventListener('blur', releaseAll);
      document.removeEventListener('visibilitychange', releaseAll);
    };
  }, []);

  const refreshSaveSlots = useCallback(async (key: string) => {
    setSaveSlots(await slotsFilled(key));
  }, []);

  /**
   * Shared tail of every load path: the caller only has to produce the bytes
   * and say who they belong to, local file and URL loads behave identically.
   */
  const startRom = useCallback(
    async (read: () => Promise<{ source: RomSource; data: Uint8Array }>) => {
      setBusy(true);
      setError(null);
      try {
        const { source, data } = await read();
        const emulator = emulatorRef.current;
        if (!emulator) return;

        await emulator.loadRom(data);
        // Restore any save the cartridge kept from an earlier session.
        const previous = readSaveRam(source.key);
        if (previous) emulator.setSaveRam(previous);
        romKeyRef.current = source.key;
        setRomKey(source.key);
        setRomName(source.name);
        setHasBattery(emulator.hasBattery());
        void refreshSaveSlots(source.key);
      } catch (cause) {
        setError(cause instanceof Error ? cause.message : String(cause));
      } finally {
        setBusy(false);
      }
    },
    [refreshSaveSlots],
  );

  const loadFile = useCallback(
    (file: File) =>
      startRom(async () => ({ source: fileRomSource(file), data: await readRomFile(file) })),
    [startRom],
  );

  const loadUrl = useCallback((url: string) => startRom(() => fetchRomFromUrl(url)), [startRom]);

  const reset = useCallback(() => {
    emulatorRef.current?.reset();
  }, []);

  /**
   * Delete the current cartridge's stored save and start it over with a blank
   * battery. Without this a dropped ROM always resumes from its old save.
   */
  const clearSave = useCallback(() => {
    const key = romKeyRef.current;
    if (key) clearSaveRam(key);

    const emulator = emulatorRef.current;
    if (!emulator) return;
    emulator.clearSaveRam();
    emulator.reset();
  }, []);

  /** Snapshot the running machine into a slot. */
  const saveSlot = useCallback(
    async (slot: number) => {
      const emulator = emulatorRef.current;
      const key = romKeyRef.current;
      if (!emulator || !key) return;

      const state = emulator.saveState();
      if (!state) return;
      await writeState(key, slot, state);
      await refreshSaveSlots(key);
    },
    [refreshSaveSlots],
  );

  /** Restore a slot into the running machine. */
  const loadSlot = useCallback(async (slot: number) => {
    const emulator = emulatorRef.current;
    const key = romKeyRef.current;
    if (!emulator || !key) return;

    const state = await readState(key, slot);
    if (state) {
      emulator.loadState(state);
      // A frozen display would hide the restored frame, so resume on load.
      emulator.resume();
    }
  }, []);

  /** Forget a slot. */
  const clearSlot = useCallback(
    async (slot: number) => {
      const key = romKeyRef.current;
      if (!key) return;
      await clearState(key, slot);
      await refreshSaveSlots(key);
    },
    [refreshSaveSlots],
  );

  const togglePause = useCallback(() => {
    emulatorRef.current?.togglePause();
  }, []);

  const toggleMute = useCallback(() => {
    setMuted((previous) => {
      const next = !previous;
      emulatorRef.current?.setMuted(next);
      return next;
    });
  }, []);

  const clearError = useCallback(() => setError(null), []);

  const setButton = useCallback((button: ButtonName, pressed: boolean) => {
    const emulator = emulatorRef.current;
    if (pressed) emulator?.wakeAudio();
    emulator?.setButton(button, pressed);
  }, []);

  const wakeAudio = useCallback(() => {
    emulatorRef.current?.wakeAudio();
  }, []);

  return useMemo(
    () => ({
      canvasRef,
      state,
      romName,
      romKey,
      error,
      fps,
      muted,
      busy,
      hasBattery,
      saveSlots,
      loadFile,
      loadUrl,
      reset,
      clearSave,
      saveSlot,
      loadSlot,
      clearSlot,
      togglePause,
      toggleMute,
      clearError,
      setButton,
      wakeAudio,
    }),
    [
      state,
      romName,
      romKey,
      error,
      fps,
      muted,
      busy,
      hasBattery,
      saveSlots,
      loadFile,
      loadUrl,
      reset,
      clearSave,
      saveSlot,
      loadSlot,
      clearSlot,
      togglePause,
      toggleMute,
      clearError,
      setButton,
      wakeAudio,
    ],
  );
}
