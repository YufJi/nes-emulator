import type { RefObject } from 'react';
import { Button, Card, Tag, Title } from 'animal-island-ui';
import { FolderIcon } from 'naive-icons';
import { useIntegerScale } from '../hooks/useIntegerScale';
import { useI18n } from '../i18n';
import type { EmulatorState } from '../emulator/NesEmulator';

interface GameScreenProps {
  stageRef: RefObject<HTMLDivElement>;
  canvasRef: RefObject<HTMLCanvasElement>;
  /** Native frame size from the core; null until WASM has loaded. */
  screen: { width: number; height: number } | null;
  state: EmulatorState;
  romName: string | null;
  busy: boolean;
  onPickRom: () => void;
}

export function GameScreen({
  stageRef,
  canvasRef,
  screen,
  state,
  romName,
  busy,
  onPickRom,
}: GameScreenProps) {
  useIntegerScale(stageRef, canvasRef, screen?.width ?? null, screen?.height ?? null);
  const { t } = useI18n();

  return (
    <section className="stage" ref={stageRef}>
      <canvas
        ref={canvasRef}
        className="stage__canvas"
        aria-label={t('stage.canvasLabel')}
      />

      {state === 'idle' && (
        <div className="stage__overlay">
          <Card type="dashed" className="stage__card">
            <Title size="middle" color="app-blue">
              {t('stage.dropTitle')}
            </Title>
            <p className="stage__hint">
              {busy ? t('stage.loadingCore') : t('stage.dropHint')}
            </p>
            <Button
              type="primary"
              icon={<FolderIcon size={18} color="currentColor" aria-hidden />}
              onClick={onPickRom}
              loading={busy}
            >
              {t('stage.chooseRom')}
            </Button>
          </Card>
        </div>
      )}

      {state === 'paused' && (
        <div className="stage__overlay stage__overlay--soft">
          <Card color="app-yellow" className="stage__card">
            <Title size="small" color="app-orange">
              {t('stage.paused')}
            </Title>
            <p className="stage__hint">{t('stage.pausedHint')}</p>
          </Card>
        </div>
      )}

      {romName && state !== 'idle' && (
        <Tag className="stage__label" color="brown" variant="solid">
          <span className="ellipsis">{romName}</span>
        </Tag>
      )}
    </section>
  );
}
