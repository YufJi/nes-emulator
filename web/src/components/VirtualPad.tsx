import type { PointerEvent } from 'react';
import { Button, Card, Title } from 'animal-island-ui';
import { ArrowDownIcon, ArrowLeftIcon, ArrowRightIcon, ArrowUpIcon } from 'naive-icons';
import type { ButtonName } from '../emulator/constants';

interface VirtualPadProps {
  disabled: boolean;
  onButton: (button: ButtonName, pressed: boolean) => void;
}

/** Touch/mouse friendly gamepad mirroring the physical controller layout. */
export function VirtualPad({ disabled, onButton }: VirtualPadProps) {
  const hold = (button: ButtonName) => ({
    onPointerDown: (event: PointerEvent<HTMLButtonElement>) => {
      event.preventDefault();
      event.currentTarget.setPointerCapture(event.pointerId);
      onButton(button, true);
    },
    onPointerUp: () => onButton(button, false),
    onPointerCancel: () => onButton(button, false),
    onLostPointerCapture: () => onButton(button, false),
  });

  return (
    <section className="panel">
      <Title size="small" color="app-orange">
        虚拟手柄
      </Title>
      <Card className="card--tight">
        <div className={`pad${disabled ? ' pad--disabled' : ''}`}>
          <div className="pad__dpad">
            <Button
              className="pad__key pad__key--up"
              icon={<ArrowUpIcon size={20} color="currentColor" aria-hidden />}
              aria-label="上"
              disabled={disabled}
              {...hold('UP')}
            />
            <Button
              className="pad__key pad__key--left"
              icon={<ArrowLeftIcon size={20} color="currentColor" aria-hidden />}
              aria-label="左"
              disabled={disabled}
              {...hold('LEFT')}
            />
            <Button
              className="pad__key pad__key--right"
              icon={<ArrowRightIcon size={20} color="currentColor" aria-hidden />}
              aria-label="右"
              disabled={disabled}
              {...hold('RIGHT')}
            />
            <Button
              className="pad__key pad__key--down"
              icon={<ArrowDownIcon size={20} color="currentColor" aria-hidden />}
              aria-label="下"
              disabled={disabled}
              {...hold('DOWN')}
            />
          </div>

          <div className="pad__actions">
            <Button
              className="pad__key--round"
              aria-label="B 键"
              disabled={disabled}
              {...hold('B')}
            >
              B
            </Button>
            <Button
              className="pad__key--round"
              aria-label="A 键"
              disabled={disabled}
              {...hold('A')}
            >
              A
            </Button>
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
