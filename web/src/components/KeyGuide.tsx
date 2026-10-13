import { Card, Collapse, Divider, Tag, Title } from 'animal-island-ui';
import { KEY_GUIDE } from '../emulator/constants';
import { useI18n } from '../i18n';

export function KeyGuide() {
  const { t } = useI18n();
  // A, B, Start and Select are printed on the console's own buttons, so only
  // the D-pad row has a word to translate.
  const label = (action: (typeof KEY_GUIDE)[number]['action']) =>
    action === 'dpad' ? t('keyGuide.dpad') : action;

  return (
    <section className="panel">
      <Title size="small" color="app-green">
        {t('keyGuide.title')}
      </Title>
      <Card className="card--tight">
        <dl className="keyguide">
          {KEY_GUIDE.map(({ action, keys }) => (
            <div className="keyguide__row" key={action}>
              <dt>{label(action)}</dt>
              <dd className="keyguide__keys">
                {keys.map((key) => (
                  <Tag key={key} size="small" variant="outlined" color="default">
                    {key}
                  </Tag>
                ))}
              </dd>
            </div>
          ))}
        </dl>

        <Divider type="squiggle" />

        <p className="panel__note">{t('keyGuide.note')}</p>

        <Collapse
          question={t('keyGuide.selectQuestion')}
          answer={t('keyGuide.selectAnswer')}
        />
      </Card>
    </section>
  );
}
