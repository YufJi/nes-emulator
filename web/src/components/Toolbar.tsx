interface ToolbarProps {
  state: 'idle' | 'running' | 'paused';
  hasRom: boolean;
  muted: boolean;
  busy: boolean;
  onPick: () => void;
  onTogglePause: () => void;
  onReset: () => void;
  onToggleMute: () => void;
  onFullscreen: () => void;
}

export function Toolbar({
  state,
  hasRom,
  muted,
  busy,
  onPick,
  onTogglePause,
  onReset,
  onToggleMute,
  onFullscreen,
}: ToolbarProps) {
  return (
    <div className="toolbar">
      <button type="button" className="button button--primary" onClick={onPick} disabled={busy}>
        {hasRom ? '换一个 ROM' : '选择 ROM'}
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
