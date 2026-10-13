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
import { RomUrlForm } from '../components/RomUrlForm';
import { SaveStatePanel } from '../components/SaveStatePanel';
import type { EmulatorState } from '../emulator/NesEmulator';

const STATE_TAG: Record<
  EmulatorState,
  { label: string; color: 'default' | 'app-green' | 'app-yellow' }
> = {
  idle: { label: '待机', color: 'default' },
  running: { label: '运行中', color: 'app-green' },
  paused: { label: '已暂停', color: 'app-yellow' },
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

  return (
    <Drawer
      open={open}
      title="工具栏"
      className="tools-drawer"
      placement="right"
      width="min(88vw, 350px)"
      pushBackground={false}
      onClose={onClose}
    >
      <div className="mtools">
        <div className="mtools__status">
          <Tag color={busy ? 'app-orange' : STATE_TAG[state].color} variant="soft">
            {busy ? '加载中' : STATE_TAG[state].label}
          </Tag>
          <Tag size="small" color="brown" variant="soft">
            <span className="ellipsis">{romName ?? '未加载 ROM'}</span>
          </Tag>
          <Tag size="small" color={state === 'running' ? 'app-blue' : 'default'} variant="soft">
            {state === 'running' ? `${fps} FPS` : '—'}
          </Tag>
        </div>

        <section className="panel">
          <Title size="small" color="app-teal">
            控制台
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
                {paused ? '继续' : '暂停'}
              </Button>

              <Button
                icon={<RefreshIcon size={18} color="currentColor" aria-hidden />}
                onClick={onReset}
                disabled={!hasRom}
              >
                重置
              </Button>

              <Button
                icon={<MaximizeIcon size={18} color="currentColor" aria-hidden />}
                onClick={onFullscreen}
                disabled={!hasRom}
              >
                全屏
              </Button>

              <Button
                icon={<TrashIcon size={18} color="currentColor" aria-hidden />}
                onClick={() => setConfirmingClear(true)}
                disabled={!hasRom || !hasBattery}
              >
                清空存档
              </Button>
            </div>

            <div className="toolbar__row">
              <span className="toolbar__label">静音</span>
              <Switch
                checked={muted}
                onChange={onToggleMute}
                disabled={!hasRom}
                aria-label="静音"
                checkedChildren={<VolumeXIcon size={14} color="currentColor" aria-hidden />}
                unCheckedChildren={<VolumeIcon size={14} color="currentColor" aria-hidden />}
              />
            </div>

            <div className="toolbar__row">
              <span className="toolbar__label">震动</span>
              <Switch
                checked={haptics}
                onChange={onToggleHaptics}
                disabled={!hapticsSupported}
                aria-label="震动反馈"
              />
            </div>

            <p className="panel__note">
              {hapticsSupported
                ? '按键与方向都会有轻微振动；觉得吵可以关掉。'
                : '当前浏览器不提供震动接口（iOS Safari 没有该 API），此处无法开启。'}
            </p>
          </Card>
        </section>

        <section className="panel">
          <Title size="small" color="app-orange">
            载入
          </Title>
          <Card className="card--tight">
            <div className="toolbar">
              <Button
                type="primary"
                icon={<FolderIcon size={18} color="currentColor" aria-hidden />}
                onClick={onPickRom}
                loading={busy}
              >
                {hasRom ? '换一个 ROM' : '选择 ROM'}
              </Button>
              <Button
                icon={<ListIcon size={18} color="currentColor" aria-hidden />}
                onClick={onOpenGames}
              >
                游戏列表
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
      </div>

      <ClearSaveDialog
        open={confirmingClear}
        onCancel={() => setConfirmingClear(false)}
        onConfirm={onClearSave}
      />
    </Drawer>
  );
}
