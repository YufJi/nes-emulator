import { useCallback, useEffect, useRef, useState } from 'react';
import { DropOverlay } from './components/DropOverlay';
import { GameScreen } from './components/GameScreen';
import { KeyGuide } from './components/KeyGuide';
import { StatusBar } from './components/StatusBar';
import { Toolbar } from './components/Toolbar';
import { VirtualPad } from './components/VirtualPad';
import { useNesEmulator } from './hooks/useNesEmulator';

const ROM_ACCEPT = '.nes,.fds,.unf,.unif,application/octet-stream';

export default function App() {
  const {
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
  } = useNesEmulator();

  const stageRef = useRef<HTMLDivElement | null>(null);
  const fileInputRef = useRef<HTMLInputElement | null>(null);
  const dragDepth = useRef(0);
  const [dragging, setDragging] = useState(false);

  const hasRom = state !== 'idle';

  const handleFiles = useCallback(
    (files: FileList | null) => {
      const file = files?.[0];
      if (file) void loadFile(file);
    },
    [loadFile],
  );

  const pickRom = useCallback(() => {
    wakeAudio();
    fileInputRef.current?.click();
  }, [wakeAudio]);

  // Window-wide drag & drop so a ROM can be dropped anywhere on the page.
  useEffect(() => {
    const carriesFiles = (event: DragEvent) =>
      Array.from(event.dataTransfer?.types ?? []).includes('Files');

    // dragenter/dragleave fire for every nested element, so count them to know
    // when the pointer has really left the window.
    const onDragEnter = (event: DragEvent) => {
      if (!carriesFiles(event)) return;
      event.preventDefault();
      dragDepth.current += 1;
      setDragging(true);
    };

    const onDragOver = (event: DragEvent) => {
      if (!carriesFiles(event)) return;
      event.preventDefault();
      if (event.dataTransfer) event.dataTransfer.dropEffect = 'copy';
    };

    const onDragLeave = (event: DragEvent) => {
      if (!carriesFiles(event)) return;
      dragDepth.current = Math.max(0, dragDepth.current - 1);
      if (dragDepth.current === 0) setDragging(false);
    };

    const onDrop = (event: DragEvent) => {
      event.preventDefault();
      dragDepth.current = 0;
      setDragging(false);
      handleFiles(event.dataTransfer?.files ?? null);
    };

    window.addEventListener('dragenter', onDragEnter);
    window.addEventListener('dragover', onDragOver);
    window.addEventListener('dragleave', onDragLeave);
    window.addEventListener('drop', onDrop);

    return () => {
      window.removeEventListener('dragenter', onDragEnter);
      window.removeEventListener('dragover', onDragOver);
      window.removeEventListener('dragleave', onDragLeave);
      window.removeEventListener('drop', onDrop);
    };
  }, [handleFiles]);

  // `P` mirrors the pause button for keyboard players.
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.code !== 'KeyP' || event.repeat) return;
      const target = event.target;
      if (
        target instanceof HTMLElement &&
        (target.isContentEditable || ['INPUT', 'TEXTAREA', 'SELECT'].includes(target.tagName))
      ) {
        return;
      }
      togglePause();
    };

    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [togglePause]);

  const toggleFullscreen = useCallback(() => {
    const stage = stageRef.current;
    if (!stage) return;

    if (document.fullscreenElement) {
      void document.exitFullscreen().catch(() => undefined);
      return;
    }
    void stage.requestFullscreen?.().catch(() => undefined);
  }, []);

  return (
    <div className="app">
      <header className="topbar">
        <div className="brand">
          <span className="brand__mark">NES</span>
          <div className="brand__text">
            <h1>NES Emulator</h1>
            <p>Rust + WebAssembly 内核 · 拖入 ROM 即可开玩</p>
          </div>
        </div>
        <StatusBar state={state} romName={romName} fps={fps} busy={busy} />
      </header>

      <main className="layout">
        <div className="layout__screen">
          <GameScreen
            stageRef={stageRef}
            canvasRef={canvasRef}
            state={state}
            romName={romName}
            busy={busy}
            onPickRom={pickRom}
          />
        </div>

        <aside className="layout__side">
          <Toolbar
            state={state}
            hasRom={hasRom}
            muted={muted}
            busy={busy}
            onPick={pickRom}
            onTogglePause={togglePause}
            onReset={reset}
            onToggleMute={toggleMute}
            onFullscreen={toggleFullscreen}
          />
          <VirtualPad disabled={!hasRom} onButton={setButton} />
          <KeyGuide />
        </aside>
      </main>

      <input
        ref={fileInputRef}
        className="visually-hidden"
        type="file"
        accept={ROM_ACCEPT}
        onChange={(event) => {
          handleFiles(event.target.files);
          // Allow re-selecting the same file.
          event.target.value = '';
        }}
      />

      {error && (
        <div className="toast toast--error" role="alert">
          <span>{error}</span>
          <button type="button" className="toast__close" onClick={clearError} aria-label="关闭">
            ×
          </button>
        </div>
      )}

      {dragging && <DropOverlay />}
    </div>
  );
}
