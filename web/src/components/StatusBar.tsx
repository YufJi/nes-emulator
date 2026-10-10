import type { EmulatorState } from '../emulator/NesEmulator';

interface StatusBarProps {
  state: EmulatorState;
  romName: string | null;
  fps: number;
  busy: boolean;
}

const STATE_LABEL: Record<EmulatorState, string> = {
  idle: '待机',
  running: '运行中',
  paused: '已暂停',
};

export function StatusBar({ state, romName, fps, busy }: StatusBarProps) {
  return (
    <div className="status">
      <span className={`status__pill status__pill--${busy ? 'busy' : state}`}>
        {busy ? '加载中' : STATE_LABEL[state]}
      </span>
      <span className="status__rom" title={romName ?? undefined}>
        {romName ?? '未加载 ROM'}
      </span>
      <span className="status__fps">{state === 'running' ? `${fps} FPS` : '—'}</span>
    </div>
  );
}
