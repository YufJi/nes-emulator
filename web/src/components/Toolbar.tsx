import { useState } from 'react';
import { Button, Card, Switch, Title } from 'animal-island-ui';
import {
  FolderIcon,
  ListIcon,
  MaximizeIcon,
  PauseIcon,
  PlayIcon,
  RefreshIcon,
  TrashIcon,
  VolumeIcon,
  VolumeXIcon,
} from 'naive-icons';
import { ClearSaveDialog } from './ClearSaveDialog';
import { useI18n } from '../i18n';

interface ToolbarProps {
  state: 'idle' | 'running' | 'paused';
  hasRom: boolean;
  hasBattery: boolean;
  muted: boolean;
  busy: boolean;
  onPick: () => void;
  onOpenGames: () => void;
  onTogglePause: () => void;
  onReset: () => void;
  onClearSave: () => void;
  onToggleMute: () => void;
  onFullscreen: () => void;
}

export function Toolbar({
  state,
  hasRom,
  hasBattery,
  muted,
  busy,
  onPick,
  onOpenGames,
  onTogglePause,
  onReset,
  onClearSave,
  onToggleMute,
  onFullscreen,
}: ToolbarProps) {
  const [confirmingClear, setConfirmingClear] = useState(false);
  const paused = state === 'paused';
  const { t } = useI18n();

  return (
    <section className="panel">
      <Title size="small" color="app-teal">
        {t('toolbar.title')}
      </Title>
      <Card className="card--tight">
        <div className="toolbar">
          <Button
            type="primary"
            icon={<FolderIcon size={18} color="currentColor" aria-hidden />}
            onClick={onPick}
            loading={busy}
          >
            {hasRom ? t('toolbar.swapRom') : t('toolbar.chooseRom')}
          </Button>

          <Button
            icon={<ListIcon size={18} color="currentColor" aria-hidden />}
            onClick={onOpenGames}
          >
            {t('toolbar.gameList')}
          </Button>

          <Button
            icon={
              paused ? (
                <PlayIcon size={18} color="currentColor" aria-hidden />
              ) : (
                <PauseIcon size={18} color="currentColor" aria-hidden />
              )
            }
            onClick={onTogglePause}
            disabled={!hasRom}
          >
            {paused ? t('toolbar.resume') : t('toolbar.pause')}
          </Button>

          <Button
            icon={<RefreshIcon size={18} color="currentColor" aria-hidden />}
            onClick={onReset}
            disabled={!hasRom}
          >
            {t('toolbar.reset')}
          </Button>

          <Button
            icon={<TrashIcon size={18} color="currentColor" aria-hidden />}
            onClick={() => setConfirmingClear(true)}
            disabled={!hasRom || !hasBattery}
          >
            {t('toolbar.clearSave')}
          </Button>

          <Button
            icon={<MaximizeIcon size={18} color="currentColor" aria-hidden />}
            onClick={onFullscreen}
            disabled={!hasRom}
          >
            {t('toolbar.fullscreen')}
          </Button>
        </div>

        <div className="toolbar__row">
          <span className="toolbar__label">{t('toolbar.mute')}</span>
          <Switch
            checked={muted}
            onChange={onToggleMute}
            disabled={!hasRom}
            aria-label={t('toolbar.mute')}
            checkedChildren={<VolumeXIcon size={14} color="currentColor" aria-hidden />}
            unCheckedChildren={<VolumeIcon size={14} color="currentColor" aria-hidden />}
          />
        </div>

        <p className="panel__note">
          {hasBattery
            ? t('toolbar.clearSaveHint')
            : t('toolbar.noBatteryHint')}
        </p>
      </Card>

      <ClearSaveDialog
        open={confirmingClear}
        onCancel={() => setConfirmingClear(false)}
        onConfirm={onClearSave}
      />
    </section>
  );
}
