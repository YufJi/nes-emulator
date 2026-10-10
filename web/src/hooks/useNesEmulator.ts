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
import { readRomFile } from '../emulator/rom';
import { readSaveRam, writeSaveRam } from '../emulator/saveRam';

export interface UseNesEmulatorResult {
  canvasRef: RefObject<HTMLCanvasElement>;
  state: EmulatorState;
  romName: string | null;
  error: string | null;
  fps: number;
  muted: boolean;
  busy: boolean;
  loadFile: (file: File) => Promise<void>;
  reset: () => void;
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
  const [error, setError] = useState<string | null>(null);
  const [fps, setFps] = useState(0);
  const [muted, setMuted] = useState(false);
  const [busy, setBusy] = useState(true);
  // The save-RAM callback is created once with the emulator, so it reads the
  // current ROM name through a ref instead of a captured value.
  const romNameRef = useRef<string | null>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;

    const emulator = new NesEmulator(canvas, {
      onStateChange: setState,
      onFpsChange: setFps,
      onSaveRamChange: (data) => {
        const name = romNameRef.current;
        if (name) writeSaveRam(name, data);
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

  const loadFile = useCallback(async (file: File) => {
    const emulator = emulatorRef.current;
    if (!emulator) return;

    setBusy(true);
    setError(null);
    try {
      const data = await readRomFile(file);
      await emulator.loadRom(data);
      // Restore any save the cartridge kept from an earlier session.
      const previous = readSaveRam(file.name);
      if (previous) emulator.setSaveRam(previous);
      romNameRef.current = file.name;
      setRomName(file.name);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy(false);
    }
  }, []);

  const reset = useCallback(() => {
    emulatorRef.current?.reset();
  }, []);

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
      error,
      fps,
      muted,
      busy,
      loadFile,
      reset,
      togglePause,
      toggleMute,
      clearError,
      setButton,
      wakeAudio,
    }),
    [
      state,
      romName,
      error,
      fps,
      muted,
      busy,
      loadFile,
      reset,
      togglePause,
      toggleMute,
      clearError,
      setButton,
      wakeAudio,
    ],
  );
}
