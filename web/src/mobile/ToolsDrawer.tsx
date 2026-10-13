import { useState } from 'react';
import { Button, Card, Drawer, Switch, Tag, Title } from 'animal-island-ui';
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
import { ClearSaveDialog } from '../components/ClearSaveDialog';
import { Disclaimer } from '../components/Disclaimer';
import { LanguageSwitcher } from '../components/LanguageSwitcher';
import { RomUrlForm } from '../components/RomUrlForm';
import { SaveStatePanel } from '../components/SaveStatePanel';
import type { EmulatorState } from '../emulator/NesEmulator';
import { useI18n, type MessageKey } from '../i18n';

const STATE_TAG: Record<
  EmulatorState,
  { label: MessageKey; color: 'default' | 'app-green' | 'app-yellow' }
> = {
  idle: { label: 'status.idle', color: 'default' },
  running: { label: 'status.running', color: 'app-green' },
  paused: { label: 'status.paused', color: 'app-yellow' },
};

interface ToolsDrawerProps {
  open: boolean;
  onClose: () => void;
  state: EmulatorState;
  romName: string | null;
  hasRom: boolean;
  hasBattery: boolean;
  muted: boolean;
  busy: boolean;
  fps: number;
  saveSlots: boolean[];
  onPickRom: () => void;
  onOpenGames: () => void;
  onLoadUrl: (url: string) => void;
  onTogglePause: () => void;
  onReset: () => void;
  onClearSave: () => void;
  onToggleMute: () => void;
  haptics: boolean;
  /** False on devices without `navigator.vibrate` — iOS, in other words. */
  hapticsSupported: boolean;
  onToggleHaptics: () => void;
  onFullscreen: () => void;
  onSaveSlot: (slot: number) => void;
  onLoadSlot: (slot: number) => void;
  onClearSlot: (slot: number) => void;
}

/**
 * Everything that is not the picture or the pad, folded into a drawer behind a
 * button in the top-right corner.
 *
 * The panel is the wide-screen sidebar's content in the same island dress,
 * which is why `RomUrlForm` and `SaveStatePanel` are reused as they are.
 * Pushing the background is turned off: that effect scales the page down, and a
 * scaled canvas is a blurry canvas.
 *
 * The class on the panel exists because the drawer lives in a portal, outside
 * the page's own safe-area padding, and it is pinned to the right edge — the
 * unsafe one when a phone with a notch is held sideways.
 */
export function ToolsDrawer({
  open,
  onClose,
  state,
  romName,
  hasRom,
  hasBattery,
  muted,
  busy,
  fps,
  saveSlots,
  onPickRom,
  onOpenGames,
  onLoadUrl,
  onTogglePause,
  onReset,
  onClearSave,
  onToggleMute,
  haptics,
  hapticsSupported,
  onToggleHaptics,
  onFullscreen,
  onSaveSlot,
  onLoadSlot,
  onClearSlot,
}: ToolsDrawerProps) {
  const [confirmingClear, setConfirmingClear] = useState(false);
  const paused = state === 'paused';
  const { t } = useI18n();

  return (
    <Drawer
      open={open}
      title={t('tools.title')}
      className="tools-drawer"
      placement="right"
      width="min(88vw, 350px)"
      pushBackground={false}
      onClose={onClose}
    >
      <div className="mtools">
        <div className="mtools__status">
          <Tag color={busy ? 'app-orange' : STATE_TAG[state].color} variant="soft">
            {busy ? t('status.loading') : t(STATE_TAG[state].label)}
          </Tag>
          <Tag size="small" color="brown" variant="soft">
            <span className="ellipsis">{romName ?? t('status.noRom')}</span>
          </Tag>
          <Tag size="small" color={state === 'running' ? 'app-blue' : 'default'} variant="soft">
            {state === 'running' ? `${fps} FPS` : '—'}
          </Tag>
        </div>

        <section className="panel">
          <Title size="small" color="app-teal">
            {t('toolbar.title')}
          </Title>
          <Card className="card--tight">
            <div className="toolbar">
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
                icon={<MaximizeIcon size={18} color="currentColor" aria-hidden />}
                onClick={onFullscreen}
                disabled={!hasRom}
              >
                {t('toolbar.fullscreen')}
              </Button>

              <Button
                icon={<TrashIcon size={18} color="currentColor" aria-hidden />}
                onClick={() => setConfirmingClear(true)}
                disabled={!hasRom || !hasBattery}
              >
                {t('toolbar.clearSave')}
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

            <div className="toolbar__row">
              <span className="toolbar__label">{t('toolbar.haptics')}</span>
              <Switch
                checked={haptics}
                onChange={onToggleHaptics}
                disabled={!hapticsSupported}
                aria-label={t('toolbar.haptics')}
              />
            </div>

            <p className="panel__note">
              {hapticsSupported ? t('toolbar.hapticsHint') : t('toolbar.hapticsUnsupported')}
            </p>

            <div className="toolbar__row">
              <span className="toolbar__label">{t('language.label')}</span>
              <LanguageSwitcher />
            </div>
          </Card>
        </section>

        <section className="panel">
          <Title size="small" color="app-orange">
            {t('tools.load')}
          </Title>
          <Card className="card--tight">
            <div className="toolbar">
              <Button
                type="primary"
                icon={<FolderIcon size={18} color="currentColor" aria-hidden />}
                onClick={onPickRom}
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
            </div>
          </Card>
        </section>

        <RomUrlForm busy={busy} onLoadUrl={onLoadUrl} />

        <SaveStatePanel
          disabled={!hasRom}
          slots={saveSlots}
          onSave={onSaveSlot}
          onLoad={onLoadSlot}
          onClear={onClearSlot}
        />

        <Disclaimer />
      </div>

      <ClearSaveDialog
        open={confirmingClear}
        onCancel={() => setConfirmingClear(false)}
        onConfirm={onClearSave}
      />
    </Drawer>
  );
}
