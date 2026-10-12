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
  return (
    <div className="toolbar">
      <button type="button" className="button button--primary" onClick={onPick} disabled={busy}>
        {hasRom ? '换一个 ROM' : '选择 ROM'}
      </button>

      <button type="button" className="button" onClick={onOpenGames}>
        游戏列表
      </button>

      <button type="button" className="button" onClick={onTogglePause} disabled={!hasRom}>
        {state === 'paused' ? '继续' : '暂停'}
      </button>

      <button type="button" className="button" onClick={onReset} disabled={!hasRom}>
        重置
      </button>

      <button
        type="button"
        className="button"
        onClick={onClearSave}
        disabled={!hasRom || !hasBattery}
        title={hasBattery ? '删除该卡带的电池存档并重新开始' : '该卡带没有电池存档'}
      >
        清空存档
      </button>

      <button
        type="button"
        className="button"
        onClick={onToggleMute}
        aria-pressed={muted}
        disabled={!hasRom}
      >
        {muted ? '取消静音' : '静音'}
      </button>

      <button type="button" className="button" onClick={onFullscreen} disabled={!hasRom}>
        全屏
      </button>
    </div>
  );
}
