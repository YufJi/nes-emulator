import { Card, Collapse, Divider, Tag, Title } from 'animal-island-ui';
import { KEY_GUIDE } from '../emulator/constants';
import { useI18n, type MessageKey } from '../i18n';

interface KeyGuideProps {
  /** Names of the controllers the browser currently reports. */
  gamepads: readonly string[];
  /** False in a browser without the Gamepad API, which cannot see a pad. */
  gamepadsSupported: boolean;
}

export function KeyGuide({ gamepads, gamepadsSupported }: KeyGuideProps) {
  const { t } = useI18n();

  // A, B, Start and Select are printed on the console's own buttons; the rows
  // that are not a button of the console have a word of their own to translate.
  const WORDS: Partial<Record<(typeof KEY_GUIDE)[number]['action'], MessageKey>> = {
    dpad: 'keyGuide.dpad',
    TurboA: 'keyGuide.turboA',
    TurboB: 'keyGuide.turboB',
  };
  const label = (action: (typeof KEY_GUIDE)[number]['action']) => {
    const word = WORDS[action];
    return word ? t(word) : action;
  };

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

          {/* A pad is not a key, but it answers the same question the rows
              above do, and the tag doubles as the connect indicator. */}
          <div className="keyguide__row">
            <dt>{t('gamepad.title')}</dt>
            <dd className="keyguide__keys">
              {gamepads.length === 0 ? (
                <Tag size="small" variant="outlined" color="default">
                  {t('gamepad.none')}
                </Tag>
              ) : (
                gamepads.map((name) => (
                  <Tag key={name} size="small" color="app-green">
                    {name}
                  </Tag>
                ))
              )}
            </dd>
          </div>
        </dl>

        <Divider type="squiggle" />

        <p className="panel__note">
          {gamepadsSupported ? t('gamepad.note') : t('gamepad.unsupported')}
        </p>

        <p className="panel__note">{t('keyGuide.note')}</p>

        <Collapse
          question={t('keyGuide.selectQuestion')}
          answer={t('keyGuide.selectAnswer')}
        />
      </Card>
    </section>
  );
}
