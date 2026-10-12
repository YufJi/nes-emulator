import { Button, Card, Tag, Title } from 'animal-island-ui';
import { DownloadIcon, SaveIcon, TrashIcon } from 'naive-icons';

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
    <section className="panel">
      <Title size="small" color="app-yellow">
        即时存档
      </Title>
      <Card className="card--tight">
        <ul className="save-state__list">
          {slots.map((filled, slot) => (
            <li key={slot} className="save-state__row">
              <div className="save-state__info">
                <span className="save-state__name">槽位 {slot + 1}</span>
                <Tag
                  size="small"
                  color={filled ? 'app-green' : 'default'}
                  variant={filled ? 'solid' : 'outlined'}
                >
                  {filled ? '已保存' : '空'}
                </Tag>
              </div>

              <div className="save-state__actions">
                <Button
                  type="primary"
                  size="small"
                  icon={<SaveIcon size={15} color="currentColor" aria-hidden />}
                  aria-label={`保存到槽位 ${slot + 1}`}
                  onClick={() => onSave(slot)}
                  disabled={disabled}
                />
                <Button
                  size="small"
                  icon={<DownloadIcon size={15} color="currentColor" aria-hidden />}
                  aria-label={`读取槽位 ${slot + 1}`}
                  onClick={() => onLoad(slot)}
                  disabled={disabled || !filled}
                />
                <Button
                  danger
                  size="small"
                  icon={<TrashIcon size={15} color="currentColor" aria-hidden />}
                  aria-label={`清除槽位 ${slot + 1}`}
                  onClick={() => onClear(slot)}
                  disabled={disabled || !filled}
                />
              </div>
            </li>
          ))}
        </ul>
        <p className="panel__note">保存整个机器状态，可随时读取。</p>
      </Card>
    </section>
  );
}
