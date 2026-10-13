import { Card, Title } from 'animal-island-ui';
import { useI18n } from '../i18n';

export function DropOverlay() {
  const { t } = useI18n();

  return (
    <div className="dropzone" aria-hidden="true">
      <Card type="dashed" className="dropzone__card">
        <Title size="middle" color="app-green">
          {t('drop.title')}
        </Title>
        <p className="dropzone__hint">{t('drop.hint')}</p>
      </Card>
    </div>
  );
}
