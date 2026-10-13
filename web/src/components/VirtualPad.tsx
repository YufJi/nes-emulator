import type { PointerEvent } from 'react';
import { Button, Card, Title } from 'animal-island-ui';
import { ArrowDownIcon, ArrowLeftIcon, ArrowRightIcon, ArrowUpIcon } from 'naive-icons';
import type { PadInput } from '../emulator/constants';
import { useI18n } from '../i18n';

interface VirtualPadProps {
  disabled: boolean;
  onButton: (input: PadInput, pressed: boolean) => void;
}

/** Touch/mouse friendly gamepad mirroring the physical controller layout. */
export function VirtualPad({ disabled, onButton }: VirtualPadProps) {
  const { t } = useI18n();

  const hold = (input: PadInput) => ({
    onPointerDown: (event: PointerEvent<HTMLButtonElement>) => {
      event.preventDefault();
      event.currentTarget.setPointerCapture(event.pointerId);
      onButton(input, true);
    },
    onPointerUp: () => onButton(input, false),
    onPointerCancel: () => onButton(input, false),
    onLostPointerCapture: () => onButton(input, false),
  });

  return (
    <section className="panel">
      <Title size="small" color="app-orange">
        {t('pad.group')}
      </Title>
      <Card className="card--tight">
        <div className={`pad${disabled ? ' pad--disabled' : ''}`}>
          <div className="pad__dpad">
            <Button
              className="pad__key pad__key--up"
              icon={<ArrowUpIcon size={20} color="currentColor" aria-hidden />}
              aria-label={t('pad.up')}
              disabled={disabled}
              {...hold('UP')}
            />
            <Button
              className="pad__key pad__key--left"
              icon={<ArrowLeftIcon size={20} color="currentColor" aria-hidden />}
              aria-label={t('pad.left')}
              disabled={disabled}
              {...hold('LEFT')}
            />
            <Button
              className="pad__key pad__key--right"
              icon={<ArrowRightIcon size={20} color="currentColor" aria-hidden />}
              aria-label={t('pad.right')}
              disabled={disabled}
              {...hold('RIGHT')}
            />
            <Button
              className="pad__key pad__key--down"
              icon={<ArrowDownIcon size={20} color="currentColor" aria-hidden />}
              aria-label={t('pad.down')}
              disabled={disabled}
              {...hold('DOWN')}
            />
          </div>

          <div className="pad__actions">
            {/* The turbo switches sit above the pair they fire, as they do on
                the pad they are modelled on. */}
            <div className="pad__turbo">
              <Button
                className="pad__key--turbo"
                aria-label={t('pad.turboB')}
                disabled={disabled}
                {...hold('TURBO_B')}
              >
                {t('pad.turboB')}
              </Button>
              <Button
                className="pad__key--turbo"
                aria-label={t('pad.turboA')}
                disabled={disabled}
                {...hold('TURBO_A')}
              >
                {t('pad.turboA')}
              </Button>
            </div>

            <div className="pad__pair">
              <Button
                className="pad__key--round"
                aria-label={t('pad.b')}
                disabled={disabled}
                {...hold('B')}
              >
                B
              </Button>
              <Button
                className="pad__key--round"
                aria-label={t('pad.a')}
                disabled={disabled}
                {...hold('A')}
              >
                A
              </Button>
            </div>
          </div>

          <div className="pad__system">
            <Button block size="small" disabled={disabled} {...hold('SELECT')}>
              SELECT
            </Button>
            <Button block size="small" disabled={disabled} {...hold('START')}>
              START
            </Button>
          </div>
        </div>
      </Card>
    </section>
  );
}
