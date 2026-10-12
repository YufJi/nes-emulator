import { Tag, Tooltip } from 'animal-island-ui';
import { FileIcon } from 'naive-icons';
import type { EmulatorState } from '../emulator/NesEmulator';

interface StatusBarProps {
  state: EmulatorState;
  romName: string | null;
  /** File name or URL the cartridge was loaded from, shown on hover. */
  romKey: string | null;
  fps: number;
  busy: boolean;
}

/** Island palette per emulator state, so colour says the same thing as the label. */
const STATE_TAG: Record<
  EmulatorState,
  { label: string; color: 'default' | 'app-green' | 'app-yellow' }
> = {
  idle: { label: '待机', color: 'default' },
  running: { label: '运行中', color: 'app-green' },
  paused: { label: '已暂停', color: 'app-yellow' },
};

export function StatusBar({ state, romName, romKey, fps, busy }: StatusBarProps) {
  const current = STATE_TAG[state];

  return (
    <div className="status">
      <Tag
        color={busy ? 'app-orange' : current.color}
        variant={busy || state === 'idle' ? 'soft' : 'solid'}
      >
        {busy ? '加载中' : current.label}
      </Tag>

      {/* Tag swallows unknown props, so the Tooltip's hover handlers go on a
          plain wrapper element instead. */}
      <Tooltip title={romKey ?? '还没有选择卡带'}>
        <span className="status__rom">
          <Tag className="status__rom-tag" color="brown" variant="soft" size="small">
            <FileIcon size={13} color="currentColor" aria-hidden />
            <span className="ellipsis">{romName ?? '未加载 ROM'}</span>
          </Tag>
        </span>
      </Tooltip>

      <Tag size="small" color={state === 'running' ? 'app-blue' : 'default'} variant="soft">
        {state === 'running' ? `${fps} FPS` : '—'}
      </Tag>
    </div>
  );
}
