import { useCallback, useRef, useState, type PointerEvent } from 'react';
import { Button } from 'animal-island-ui';
import { ArrowDownIcon, ArrowLeftIcon, ArrowRightIcon, ArrowUpIcon } from 'naive-icons';
import { BUTTON_NAMES, type ButtonName } from '../emulator/constants';

interface TouchPadProps {
  disabled: boolean;
  onButton: (button: ButtonName, pressed: boolean) => void;
}

/**
 * Dead zone of the d-pad, as a fraction of its half-size. A finger resting near
 * the middle presses nothing instead of jittering between two directions.
 */
const DEAD_ZONE = 0.24;

/** How far the stick head leans at full deflection, in CSS pixels. */
const STICK_THROW_PX = 16;

/** `navigator.vibrate` does not exist on iOS; a missing buzz is not an error. */
function buzz(ms: number): void {
  navigator.vibrate?.(ms);
}

/**
 * Buttons a single pointer holds. One finger can hold two at once: the d-pad
 * reports each axis on its own, which is what makes diagonals work.
 */
type Held = ReadonlySet<ButtonName>;

const NOTHING: Held = new Set<ButtonName>();

/** Which buttons the element under the finger is meant to press, if any. */
function resolve(root: HTMLElement, x: number, y: number): Held {
  const element = document.elementFromPoint(x, y);
  const control = element instanceof Element ? element.closest('[data-control]') : null;
  if (!control || !root.contains(control)) return NOTHING;

  const name = control.getAttribute('data-control');
  if (name === 'dpad') return resolveDirection(control as HTMLElement, x, y);
  return name ? new Set([name as ButtonName]) : NOTHING;
}

/**
 * Direction from the touch offset, axes considered separately.
 *
 * Independent axes are what give a diagonal for free (a thumb in the top-right
 * corner returns `UP` and `RIGHT`), and they keep the boundary stable while the
 * thumb rolls around the middle.
 */
function resolveDirection(pad: HTMLElement, x: number, y: number): Held {
  const rect = pad.getBoundingClientRect();
  const dx = (x - (rect.left + rect.width / 2)) / (rect.width / 2);
  const dy = (y - (rect.top + rect.height / 2)) / (rect.height / 2);

  const held = new Set<ButtonName>();
  if (dx <= -DEAD_ZONE) held.add('LEFT');
  else if (dx >= DEAD_ZONE) held.add('RIGHT');
  if (dy <= -DEAD_ZONE) held.add('UP');
  else if (dy >= DEAD_ZONE) held.add('DOWN');
  return held;
}

/**
 * The phone controller.
 *
 * Every press is handled by the wrapper rather than by the buttons themselves:
 * touch pointers are implicitly captured by whatever is under the finger, so a
 * per-button handler would keep the thumb stuck to the button it landed on and
 * make slides (d-pad rolls, B→A rolls) impossible. Here each pointer is resolved
 * from its coordinates on every move, which is also what lets the d-pad behave
 * as one piece instead of four separate keys.
 */
export function TouchPad({ disabled, onButton }: TouchPadProps) {
  const rootRef = useRef<HTMLDivElement | null>(null);
  /** Buttons held per pointer id; the union across pointers is what is pressed. */
  const pointers = useRef(new Map<number, Held>());
  const held = useRef<Set<ButtonName>>(new Set());
  const [active, setActive] = useState<Held>(NOTHING);

  const sync = useCallback(() => {
    const next = new Set<ButtonName>();
    for (const buttons of pointers.current.values()) {
      for (const button of buttons) next.add(button);
    }

    for (const button of BUTTON_NAMES) {
      const was = held.current.has(button);
      const now = next.has(button);
      if (was === now) continue;
      onButton(button, now);
      // Only the press buzzes: a release is felt through the picture.
      if (now) buzz(8);
    }

    held.current = next;
    setActive(next);
  }, [onButton]);

  const track = useCallback(
    (event: PointerEvent<HTMLDivElement>) => {
      const root = rootRef.current;
      if (!root) return;
      pointers.current.set(event.pointerId, resolve(root, event.clientX, event.clientY));
      sync();
    },
    [sync],
  );

  const onPointerDown = useCallback(
    (event: PointerEvent<HTMLDivElement>) => {
      if (disabled) return;
      // Keeps the browser from turning a touch into a scroll, a text selection
      // or a long-press menu, and hands every later move to this element.
      event.preventDefault();
      event.currentTarget.setPointerCapture(event.pointerId);
      track(event);
    },
    [disabled, track],
  );

  const onPointerMove = useCallback(
    (event: PointerEvent<HTMLDivElement>) => {
      if (!pointers.current.has(event.pointerId)) return;
      track(event);
    },
    [track],
  );

  const onPointerEnd = useCallback(
    (event: PointerEvent<HTMLDivElement>) => {
      if (!pointers.current.delete(event.pointerId)) return;
      sync();
    },
    [sync],
  );

  // A thumb that leaves the pad entirely stops holding a direction; the buttons
  // stay reachable because the wrapper holds the capture until the finger lifts.
  const direction = (button: ButtonName) => active.has(button);

  // Direction the stick head leans in. One axis shows a short throw, two (a
  // diagonal) a shorter one each, so the head travels the same distance either
  // way instead of overshooting into the rim.
  const vertical = direction('UP') || direction('DOWN');
  const horizontal = direction('LEFT') || direction('RIGHT');
  const diagonal = vertical && horizontal;
  const throwPx = diagonal ? STICK_THROW_PX * 0.7 : STICK_THROW_PX;
  const stickOffset = {
    x: ((direction('RIGHT') ? 1 : 0) - (direction('LEFT') ? 1 : 0)) * throwPx,
    y: ((direction('DOWN') ? 1 : 0) - (direction('UP') ? 1 : 0)) * throwPx,
  };

  return (
    <div
      ref={rootRef}
      className={`tpad${disabled ? ' tpad--disabled' : ''}`}
      role="group"
      aria-label="虚拟手柄"
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerEnd}
      onPointerCancel={onPointerEnd}
      onLostPointerCapture={onPointerEnd}
      onContextMenu={(event) => event.preventDefault()}
    >
      <div className="tpad__dpad" data-control="dpad">
        {/* Drawn as one continuous pad, not four keys: the whole circle is the
            target, so a thumb never has to find a small button — the direction
            just comes from where it landed. The element itself is a slightly
            larger square than the face it paints, so the corners a thumb
            reaches for a diagonal still count. */}
        <div className="tpad__face" aria-hidden>
          <span className="tpad__dir tpad__dir--up" data-pressed={direction('UP')}>
            <ArrowUpIcon size={26} color="currentColor" aria-hidden />
          </span>
          <span className="tpad__dir tpad__dir--left" data-pressed={direction('LEFT')}>
            <ArrowLeftIcon size={26} color="currentColor" aria-hidden />
          </span>
          <span className="tpad__dir tpad__dir--right" data-pressed={direction('RIGHT')}>
            <ArrowRightIcon size={26} color="currentColor" aria-hidden />
          </span>
          <span className="tpad__dir tpad__dir--down" data-pressed={direction('DOWN')}>
            <ArrowDownIcon size={26} color="currentColor" aria-hidden />
          </span>
          <span
            className="tpad__stick"
            data-pressed={vertical || horizontal}
            style={{ transform: `translate(${stickOffset.x}px, ${stickOffset.y}px)` }}
          />
        </div>
      </div>

      <div className="tpad__system">
        <Button
          block
          size="small"
          className="tpad__key tpad__key--system"
          data-control="SELECT"
          data-pressed={direction('SELECT')}
          disabled={disabled}
          tabIndex={-1}
        >
          SELECT
        </Button>
        <Button
          block
          size="small"
          className="tpad__key tpad__key--system"
          data-control="START"
          data-pressed={direction('START')}
          disabled={disabled}
          tabIndex={-1}
        >
          START
        </Button>
      </div>

      <div className="tpad__actions">
        <Button
          className="tpad__key tpad__key--ab"
          data-control="B"
          data-pressed={direction('B')}
          disabled={disabled}
          tabIndex={-1}
        >
          B
        </Button>
        <Button
          className="tpad__key tpad__key--ab"
          data-control="A"
          data-pressed={direction('A')}
          disabled={disabled}
          tabIndex={-1}
        >
          A
        </Button>
      </div>
    </div>
  );
}
