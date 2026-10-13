import { Button, Card, Tag, Title } from 'animal-island-ui';
import { DownloadIcon, SaveIcon, TrashIcon } from 'naive-icons';
import { useI18n } from '../i18n';

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
  const { t } = useI18n();

  return (
    <section className="panel">
      <Title size="small" color="app-yellow">
        {t('saveState.title')}
      </Title>
      <Card className="card--tight">
        <ul className="save-state__list">
          {slots.map((filled, slot) => (
            <li key={slot} className="save-state__row">
              <div className="save-state__info">
                <span className="save-state__name">{t('saveState.slot', { slot: slot + 1 })}</span>
                <Tag
                  size="small"
                  color={filled ? 'app-green' : 'default'}
                  variant={filled ? 'solid' : 'outlined'}
                >
                  {filled ? t('saveState.filled') : t('saveState.empty')}
                </Tag>
              </div>

              <div className="save-state__actions">
                <Button
                  type="primary"
                  size="small"
                  icon={<SaveIcon size={15} color="currentColor" aria-hidden />}
                  aria-label={t('saveState.saveTo', { slot: slot + 1 })}
                  onClick={() => onSave(slot)}
                  disabled={disabled}
                />
                <Button
                  size="small"
                  icon={<DownloadIcon size={15} color="currentColor" aria-hidden />}
                  aria-label={t('saveState.loadFrom', { slot: slot + 1 })}
                  onClick={() => onLoad(slot)}
                  disabled={disabled || !filled}
                />
                <Button
                  danger
                  size="small"
                  icon={<TrashIcon size={15} color="currentColor" aria-hidden />}
                  aria-label={t('saveState.clearSlot', { slot: slot + 1 })}
                  onClick={() => onClear(slot)}
                  disabled={disabled || !filled}
                />
              </div>
            </li>
          ))}
        </ul>
        <p className="panel__note">{t('saveState.note')}</p>
      </Card>
    </section>
  );
}
