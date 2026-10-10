interface SaveStatePanelProps {
  /** True when no ROM is loaded, which disables every slot. */
  disabled: boolean;
  /** Which slots currently hold a state for the loaded ROM. */
  slots: boolean[];
  onSave: (slot: number) => void;
  onLoad: (slot: number) => void;
  onClear: (slot: number) => void;
}

/**
 * Quick-save slots for instant save states.
 *
 * Unlike battery saves the whole machine is snapshotted, so a state can be
 * loaded at any point in a game rather than only from its title screen.
 */
export function SaveStatePanel({ disabled, slots, onSave, onLoad, onClear }: SaveStatePanelProps) {
  return (
    <section className="panel save-state">
      <h2 className="panel__title">即时存档</h2>
      <p className="panel__note">保存整个机器状态，可随时读取。</p>
      <ul className="save-state__list">
        {slots.map((filled, slot) => (
          <li key={slot} className="save-state__row">
            <span className={`save-state__slot${filled ? ' save-state__slot--filled' : ''}`}>
              槽位 {slot + 1}
            </span>
            <div className="save-state__actions">
              <button
                type="button"
                className="button button--small"
                onClick={() => onSave(slot)}
                disabled={disabled}
              >
                保存
              </button>
              <button
                type="button"
                className="button button--small"
                onClick={() => onLoad(slot)}
                disabled={disabled || !filled}
              >
                读取
              </button>
              <button
                type="button"
                className="button button--small"
                onClick={() => onClear(slot)}
                disabled={disabled || !filled}
              >
                清除
              </button>
            </div>
          </li>
        ))}
      </ul>
    </section>
  );
}
