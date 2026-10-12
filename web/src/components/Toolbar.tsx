import { useState } from 'react';
import { Button, Card, Modal, Switch, Title } from 'animal-island-ui';
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

  return (
    <section className="panel">
      <Title size="small" color="app-teal">
        控制台
      </Title>
      <Card className="card--tight">
        <div className="toolbar">
          <Button
            type="primary"
            icon={<FolderIcon size={18} color="currentColor" aria-hidden />}
            onClick={onPick}
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
            icon={<TrashIcon size={18} color="currentColor" aria-hidden />}
            onClick={() => setConfirmingClear(true)}
            disabled={!hasRom || !hasBattery}
          >
            清空存档
          </Button>

          <Button
            icon={<MaximizeIcon size={18} color="currentColor" aria-hidden />}
            onClick={onFullscreen}
            disabled={!hasRom}
          >
            全屏
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

        <p className="panel__note">
          {hasBattery
            ? '清空存档会删除该卡带保存的电池存档，并让游戏从头开始。'
            : '这张卡带没有电池存档，无需清空。'}
        </p>
      </Card>

      <Modal
        open={confirmingClear}
        title="清空卡带存档？"
        width={420}
        typeSpeed={25}
        onClose={() => setConfirmingClear(false)}
        footer={
          <>
            <Button onClick={() => setConfirmingClear(false)}>取消</Button>
            <Button
              type="primary"
              danger
              onClick={() => {
                setConfirmingClear(false);
                onClearSave();
              }}
            >
              清空并重新开始
            </Button>
          </>
        }
      >
        删除该卡带的电池存档并重新开始，此操作无法撤销。
      </Modal>
    </section>
  );
}
