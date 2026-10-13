import { Card, Collapse, Divider, Tag, Title } from 'animal-island-ui';
import { KEY_GUIDE } from '../emulator/constants';
import type { PadInput } from '../emulator/constants';
import type { GamepadStatus } from '../hooks/useGamepad';
import { useI18n, type MessageKey } from '../i18n';

interface KeyGuideProps {
  /** The controllers the browser reports, and what they are holding. */
  gamepad: GamepadStatus;
}

/** Inputs whose printed name is a word rather than a letter on a key cap. */
const INPUT_WORDS: Partial<Record<PadInput, MessageKey>> = {
  TURBO_A: 'pad.turboA',
  TURBO_B: 'pad.turboB',
  UP: 'pad.up',
  DOWN: 'pad.down',
  LEFT: 'pad.left',
  RIGHT: 'pad.right',
};

export function KeyGuide({ gamepad }: KeyGuideProps) {
  const { t } = useI18n();
  const { names, inputs, supported } = gamepad;

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
              {names.length === 0 ? (
                <Tag size="small" variant="outlined" color="default">
                  {t('gamepad.none')}
                </Tag>
              ) : (
                <>
                  {names.map((name) => (
                    <Tag key={name} size="small" color="app-green">
                      {name}
                    </Tag>
                  ))}
                  {/* What the pad is holding, as it is pressed: the pad's name
                      alone cannot say whether a button ever arrived. */}
                  {inputs.map((input) => (
                    <Tag key={input} size="small" color="app-orange">
                      {INPUT_WORDS[input] ? t(INPUT_WORDS[input]) : input}
                    </Tag>
                  ))}
                </>
              )}
            </dd>
          </div>
        </dl>

        <Divider type="squiggle" />

        <p className="panel__note">
          {supported ? t('gamepad.note') : t('gamepad.unsupported')}
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
