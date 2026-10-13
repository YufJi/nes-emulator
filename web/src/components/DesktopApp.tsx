import { useCallback, useEffect, useRef, useState } from 'react';
import { Background, Card, Cursor, Footer, Title } from 'animal-island-ui';
import { PlayIcon } from 'naive-icons';
import { useErrorNotification } from '../hooks/useErrorNotification';
import { useNesEmulator } from '../hooks/useNesEmulator';
import { DropOverlay } from './DropOverlay';
import { GameListModal } from './GameListModal';
import { GameScreen } from './GameScreen';
import { KeyGuide } from './KeyGuide';
import { RomUrlForm } from './RomUrlForm';
import { SaveStatePanel } from './SaveStatePanel';
import { StatusBar } from './StatusBar';
import { Toolbar } from './Toolbar';
import { VirtualPad } from './VirtualPad';

const ROM_ACCEPT = '.nes,.fds,.unf,.unif,application/octet-stream';

/** Wide-screen page: picture and controls side by side, with ROM drop support. */
export function DesktopApp() {
  const {
    canvasRef,
    screen,
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
  } = useNesEmulator();

  const stageRef = useRef<HTMLDivElement | null>(null);
  const fileInputRef = useRef<HTMLInputElement | null>(null);
  const dragDepth = useRef(0);
  const [dragging, setDragging] = useState(false);
  const [gamesOpen, setGamesOpen] = useState(false);

  const hasRom = state !== 'idle';

  useErrorNotification(error, clearError);

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

  const openGames = useCallback(() => {
    wakeAudio();
    setGamesOpen(true);
  }, [wakeAudio]);

  const playFromList = useCallback(
    (url: string) => {
      setGamesOpen(false);
      void loadUrl(url);
    },
    [loadUrl],
  );

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
    <Cursor>
      <Background type="sprinkles" className="app-shell">
        <div className="app">
          <header className="topbar">
            <div className="brand">
              <Card color="app-green" className="brand__badge">
                <PlayIcon size={22} color="currentColor" aria-hidden />
              </Card>
              <div className="brand__text">
                <h1 className="visually-hidden">NES Emulator</h1>
                <Title size="small" color="app-teal">
                  NES Emulator
                </Title>
                <p className="brand__sub">Rust + WebAssembly 内核 · 拖入 ROM 即可开玩</p>
              </div>
            </div>
            <StatusBar state={state} romName={romName} romKey={romKey} fps={fps} busy={busy} />
          </header>

          <main className="layout">
            <div className="layout__screen">
              <GameScreen
                stageRef={stageRef}
                canvasRef={canvasRef}
                screen={screen}
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
                hasBattery={hasBattery}
                muted={muted}
                busy={busy}
                onPick={pickRom}
                onOpenGames={openGames}
                onTogglePause={togglePause}
                onReset={reset}
                onClearSave={clearSave}
                onToggleMute={toggleMute}
                onFullscreen={toggleFullscreen}
              />
              <RomUrlForm busy={busy} onLoadUrl={loadUrl} />
              <SaveStatePanel
                disabled={!hasRom}
                slots={saveSlots}
                onSave={saveSlot}
                onLoad={loadSlot}
                onClear={clearSlot}
              />
              <VirtualPad disabled={!hasRom} onButton={setButton} />
              <KeyGuide />
              <Footer text="NES Emulator · Rust + WebAssembly" />
            </aside>
          </main>
        </div>
      </Background>

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

      {dragging && <DropOverlay />}

      {gamesOpen && (
        <GameListModal
          busy={busy}
          currentUrl={romKey}
          onSelect={playFromList}
          onClose={() => setGamesOpen(false)}
        />
      )}
    </Cursor>
  );
}
