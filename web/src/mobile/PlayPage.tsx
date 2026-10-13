import { useCallback, useEffect, useRef, useState, type ChangeEvent } from 'react';
import { Background, Button, Card, Tag, Title } from 'animal-island-ui';
import { ChevronLeftIcon, FolderIcon, ListIcon, SlidersIcon } from 'naive-icons';
import { findGame } from '../games';
import { useErrorNotification } from '../hooks/useErrorNotification';
import { useGameList } from '../hooks/useGameList';
import { useNesEmulator } from '../hooks/useNesEmulator';
import { useI18n } from '../i18n';
import { navigate } from '../router';
import { MobileStage } from './MobileStage';
import { hapticsAvailable } from './haptics';
import { takePendingRom } from './romHandoff';
import { ToolsDrawer } from './ToolsDrawer';
import { TouchPad } from './TouchPad';
import { useTap } from './useTap';

const ROM_ACCEPT = '.nes,.fds,.unf,.unif,application/octet-stream';

interface PlayPageProps {
  query: URLSearchParams;
}

/**
 * Player page (`/m/play?game=<名字>`).
 *
 * Picture on top, touch pad below, and everything else behind the button in the
 * top-right corner — the phone layout gives the game the whole screen and keeps
 * the controls under the thumbs.
 */
export function PlayPage({ query }: PlayPageProps) {
  const gameParam = query.get('game');
  const { games, error: listError } = useGameList();
  const { t } = useI18n();
  const {
    canvasRef,
    screen,
    state,
    romName,
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
  } = useNesEmulator();

  const stageRef = useRef<HTMLDivElement | null>(null);
  const fileInputRef = useRef<HTMLInputElement | null>(null);
  const [toolsOpen, setToolsOpen] = useState(false);
  /** Starts off on the devices that can do it; the switch in the drawer is the
   *  escape hatch for a phone whose buzz is more annoying than useful. */
  const [haptics, setHaptics] = useState(hapticsAvailable);
  /** What the page has already put in the console, so it is not loaded twice. */
  const started = useRef<string | null>(null);

  const hasRom = state !== 'idle';
  const game = gameParam && games ? findGame(games, gameParam) : undefined;
  const missing = Boolean(gameParam) && games !== null && !game;
  const statusLabel = busy
    ? t('status.loading')
    : state === 'running'
      ? `${fps} FPS`
      : state === 'paused'
        ? t('status.paused')
        : t('status.idle');
  const statusColor = busy ? 'app-orange' : state === 'running' ? 'app-green' : 'default';

  // One notification channel for both the cartridge and the catalogue, kept
  // clear of the pad at the bottom.
  useErrorNotification(error ?? listError, clearError, 'top');

  // Start whatever this location asks for, once the console is up. A file
  // handed over by the dashboard wins: the player is the page that picked it up.
  useEffect(() => {
    if (busy || started.current === 'file') return;

    const pending = takePendingRom();
    if (pending) {
      started.current = 'file';
      void loadFile(pending);
      return;
    }

    if (!game || started.current === game.url) return;
    started.current = game.url;
    void loadUrl(game.url);
  }, [busy, game, loadFile, loadUrl]);

  const pickRom = useCallback(() => fileInputRef.current?.click(), []);

  const openGames = useCallback(() => {
    setToolsOpen(false);
    navigate('/m');
  }, []);

  // Same reason as the game rows: a tap is what gets through while the previous
  // page is still coasting.
  const back = useTap(openGames);
  const openTools = useTap(
    useCallback(() => {
      setToolsOpen(true);
    }, []),
  );

  const loadFromInput = useCallback(
    (event: ChangeEvent<HTMLInputElement>) => {
      const file = event.target.files?.[0];
      // Allow re-selecting the same file.
      event.target.value = '';
      if (file) void loadFile(file);
    },
    [loadFile],
  );

  /** Fullscreen covers the whole layout, so the pad stays reachable. */
  const toggleFullscreen = useCallback(() => {
    if (document.fullscreenElement) {
      void document.exitFullscreen().catch(() => undefined);
      return;
    }
    // The document rather than the picture: the tools drawer, the dialogs and
    // the notifications are all portalled to `body`, and a fullscreen element
    // hides everything outside itself.
    void document.documentElement.requestFullscreen?.().catch(() => undefined);
  }, []);

  return (
    <Background type="sprinkles" className="app-shell">
      <div className="mplay">
        <header className="mplay__head">
          <Button
            type="text"
            icon={<ChevronLeftIcon size={18} color="currentColor" aria-hidden />}
            aria-label={t('play.back')}
            {...back}
          />

          <div className="mplay__title">
            <span className="ellipsis mplay__name">{romName ?? t('play.noCartName')}</span>
            <Tag size="small" color={statusColor} variant="soft">
              {statusLabel}
            </Tag>
          </div>

          <Button
            icon={<SlidersIcon size={18} color="currentColor" aria-hidden />}
            aria-label={t('play.openTools')}
            {...openTools}
          />
        </header>

        <MobileStage
          stageRef={stageRef}
          canvasRef={canvasRef}
          screen={screen}
          state={state}
          onResume={togglePause}
        >
          <Card type="dashed" className="stage__card">
            <Title size="small" color="app-blue">
              {missing
                ? t('play.missingTitle')
                : busy
                  ? t('play.preparingTitle')
                  : t('play.noCartTitle')}
            </Title>
            <p className="stage__hint">
              {missing
                ? t('play.missingHint', { game: gameParam ?? '' })
                : t('play.noCartHint')}
            </p>
            <div className="stage__actions">
              <Button
                type="primary"
                icon={<FolderIcon size={18} color="currentColor" aria-hidden />}
                onClick={pickRom}
              >
                {t('stage.chooseRom')}
              </Button>
              <Button
                icon={<ListIcon size={18} color="currentColor" aria-hidden />}
                onClick={openGames}
              >
                {t('toolbar.gameList')}
              </Button>
            </div>
          </Card>
        </MobileStage>

        <div className="mplay__controls">
          <TouchPad disabled={!hasRom} haptics={haptics} onButton={setButton} />
        </div>
      </div>

      <input
        ref={fileInputRef}
        className="visually-hidden"
        type="file"
        accept={ROM_ACCEPT}
        onChange={loadFromInput}
      />

      <ToolsDrawer
        open={toolsOpen}
        onClose={() => setToolsOpen(false)}
        state={state}
        romName={romName}
        hasRom={hasRom}
        hasBattery={hasBattery}
        muted={muted}
        busy={busy}
        fps={fps}
        saveSlots={saveSlots}
        onPickRom={pickRom}
        onOpenGames={openGames}
        onLoadUrl={loadUrl}
        onTogglePause={togglePause}
        onReset={reset}
        onClearSave={clearSave}
        onToggleMute={toggleMute}
        haptics={haptics}
        hapticsSupported={hapticsAvailable()}
        onToggleHaptics={() => setHaptics((on) => !on)}
        onFullscreen={toggleFullscreen}
        onSaveSlot={saveSlot}
        onLoadSlot={loadSlot}
        onClearSlot={clearSlot}
      />

    </Background>
  );
}
