import type { PointerEvent } from 'react';
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

  const renderButton = (button: ButtonName, label: string, className = '') => (
    <button
      type="button"
      className={`pad__key ${className}`}
      disabled={disabled}
      aria-label={button}
      onContextMenu={(event) => event.preventDefault()}
      {...hold(button)}
    >
      {label}
    </button>
  );

  return (
    <div className={`pad${disabled ? ' pad--disabled' : ''}`}>
      <div className="pad__dpad">
        {renderButton('UP', '▲', 'pad__key--up')}
        {renderButton('LEFT', '◀', 'pad__key--left')}
        {renderButton('RIGHT', '▶', 'pad__key--right')}
        {renderButton('DOWN', '▼', 'pad__key--down')}
      </div>

      <div className="pad__actions">
        {renderButton('B', 'B')}
        {renderButton('A', 'A')}
      </div>

      <div className="pad__system">
        {renderButton('SELECT', 'SELECT', 'pad__key--wide')}
        {renderButton('START', 'START', 'pad__key--wide')}
      </div>
    </div>
  );
}
