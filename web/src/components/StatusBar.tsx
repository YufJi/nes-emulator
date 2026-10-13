import { Tag, Tooltip } from 'animal-island-ui';
import { FileIcon } from 'naive-icons';
import type { EmulatorState } from '../emulator/NesEmulator';
import { useI18n, type MessageKey } from '../i18n';

interface StatusBarProps {
  state: EmulatorState;
  romName: string | null;
  /** File name or URL the cartridge was loaded from, shown on hover. */
  romKey: string | null;
  fps: number;
  busy: boolean;
}

/**
 * Island palette per emulator state, so colour says the same thing as the label.
 * The labels are message keys, not text: the bar re-renders when the language
 * changes, and a table built at import time would keep the old one.
 */
const STATE_TAG: Record<
  EmulatorState,
  { label: MessageKey; color: 'default' | 'app-green' | 'app-yellow' }
> = {
  idle: { label: 'status.idle', color: 'default' },
  running: { label: 'status.running', color: 'app-green' },
  paused: { label: 'status.paused', color: 'app-yellow' },
};

export function StatusBar({ state, romName, romKey, fps, busy }: StatusBarProps) {
  const current = STATE_TAG[state];
  const { t } = useI18n();

  return (
    <div className="status">
      <Tag
        color={busy ? 'app-orange' : current.color}
        variant={busy || state === 'idle' ? 'soft' : 'solid'}
      >
        {busy ? t('status.loading') : t(current.label)}
      </Tag>

      {/* Tag swallows unknown props, so the Tooltip's hover handlers go on a
          plain wrapper element instead. */}
      <Tooltip title={romKey ?? t('status.noCart')}>
        <span className="status__rom">
          <Tag className="status__rom-tag" color="brown" variant="soft" size="small">
            <FileIcon size={13} color="currentColor" aria-hidden />
            <span className="ellipsis">{romName ?? t('status.noRom')}</span>
          </Tag>
        </span>
      </Tooltip>

      <Tag size="small" color={state === 'running' ? 'app-blue' : 'default'} variant="soft">
        {state === 'running' ? `${fps} FPS` : '—'}
      </Tag>
    </div>
  );
}
