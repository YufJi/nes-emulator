import { useCallback, useRef, useState, type PointerEvent } from 'react';
import { Button } from 'animal-island-ui';
import { ArrowDownIcon, ArrowLeftIcon, ArrowRightIcon, ArrowUpIcon } from 'naive-icons';
import { BUTTON_NAMES, type ButtonName } from '../emulator/constants';
import { buzzButton, buzzStick } from './haptics';

interface TouchPadProps {
  disabled: boolean;
  /** Whether presses should buzz; off means the visitor turned it off. */
  haptics: boolean;
  onButton: (button: ButtonName, pressed: boolean) => void;
}

/**
 * Dead zone of the d-pad, as a fraction of its half-size. A finger resting near
 * the middle presses nothing instead of jittering between two directions.
 */
const DEAD_ZONE = 0.24;

/** How far the stick head leans at full deflection, in CSS pixels. */
const STICK_THROW_PX = 16;

/**
 * Buttons a single pointer holds. One finger can hold two at once: the d-pad
 * reports each axis on its own, which is what makes diagonals work.
 */
type Held = ReadonlySet<ButtonName>;

const NOTHING: Held = new Set<ButtonName>();

const DIRECTIONS: readonly ButtonName[] = ['UP', 'DOWN', 'LEFT', 'RIGHT'];

/**
 * The combined key: one thumb cannot hold B (run) and tap A (jump) at once, so
 * this key reports both at the same time — which is exactly the run-jump.
 */
type Control = 'dpad' | 'AB' | ButtonName;

const COMBINED = 'AB';

/** Buttons a control stands for; every control maps to one or two of them. */
function buttonsOf(control: Control): readonly ButtonName[] {
  return control === COMBINED ? ['A', 'B'] : [control as ButtonName];
}

/** What a pointer is holding, and the control it landed on. */
interface Press {
  /** `'dpad'`, the combined key, or the button the finger went down on. */
  control: Control;
  /** Held by this pointer alone; the union across pointers is what is pressed. */
  buttons: Held;
  /** The pad element, kept for the stick so later moves need no hit test. */
  pad: HTMLElement | null;
}

/** The control under a point, if it belongs to this pad. */
function controlAt(
  root: HTMLElement,
  x: number,
  y: number,
): { element: HTMLElement; control: Control } | null {
  const element = document.elementFromPoint(x, y);
  const found = element instanceof Element ? element.closest('[data-control]') : null;
  if (!(found instanceof HTMLElement) || !root.contains(found)) return null;

  const name = found.getAttribute('data-control');
  if (!name) return null;
  const control = name === 'dpad' ? 'dpad' : name === COMBINED ? COMBINED : (name as ButtonName);
  return { element: found, control };
}

/**
 * Direction from the touch offset, axes considered separately.
 *
 * Independent axes are what give a diagonal for free (a thumb in the top-right
 * corner returns `UP` and `RIGHT`), and they keep the boundary stable while the
 * thumb rolls around the middle.
 *
 * The offset is not clamped to the pad: a thumb that slides past the rim keeps
 * its direction (the ratios only grow), which is how a real stick behaves and
 * what stops a drifting thumb from dropping the input mid-jump.
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
 * make slides (d-pad rolls, B→A rolls) impossible.
 *
 * A pointer belongs to the control it landed on:
 *
 * - on the stick, the direction keeps coming from the coordinates, wherever the
 *   thumb goes — sliding past the rim does not release it, only returning to the
 *   dead zone or lifting the finger does;
 * - on a button, sliding onto a neighbouring button hands the pointer over (that
 *   is how B→A rolls work), sliding into the pad's empty space keeps holding the
 *   button, and leaving the pad altogether lets go of it.
 */
export function TouchPad({ disabled, haptics, onButton }: TouchPadProps) {
  const rootRef = useRef<HTMLDivElement | null>(null);
  const pointers = useRef(new Map<number, Press>());
  const held = useRef<Set<ButtonName>>(new Set());
  const [active, setActive] = useState<Held>(NOTHING);

  const sync = useCallback(() => {
    const next = new Set<ButtonName>();
    for (const press of pointers.current.values()) {
      for (const button of press.buttons) next.add(button);
    }

    // Whether the stick was already pushing before this update, so a roll from
    // one direction to the next buzzes once rather than at every quadrant.
    const wasNeutral = !DIRECTIONS.some((button) => held.current.has(button));

    for (const button of BUTTON_NAMES) {
      const was = held.current.has(button);
      const now = next.has(button);
      if (was === now) continue;
      onButton(button, now);
      // Only the press buzzes: a release is felt through the picture.
      if (!now || !haptics) continue;
      if (DIRECTIONS.includes(button)) {
        if (wasNeutral) buzzStick();
      } else {
        buzzButton();
      }
    }

    held.current = next;
    setActive(next);
  }, [haptics, onButton]);

  const onPointerDown = useCallback(
    (event: PointerEvent<HTMLDivElement>) => {
      if (disabled) return;
      const root = rootRef.current;
      if (!root) return;
      const hit = controlAt(root, event.clientX, event.clientY);
      // A press that starts on empty space between the controls is not a press.
      if (!hit) return;

      // Keeps the browser from turning a touch into a scroll, a text selection
      // or a long-press menu, and hands every later move to this element.
      event.preventDefault();
      event.currentTarget.setPointerCapture(event.pointerId);
      pointers.current.set(event.pointerId, {
        control: hit.control,
        buttons:
          hit.control === 'dpad'
            ? resolveDirection(hit.element, event.clientX, event.clientY)
            : new Set(buttonsOf(hit.control)),
        pad: hit.control === 'dpad' ? hit.element : null,
      });
      sync();
    },
    [disabled, sync],
  );

  const onPointerMove = useCallback(
    (event: PointerEvent<HTMLDivElement>) => {
      const press = pointers.current.get(event.pointerId);
      const root = rootRef.current;
      if (!press || !root) return;

      if (press.control === 'dpad' && press.pad) {
        // Stays on the stick: the direction follows the thumb past the rim.
        pointers.current.set(event.pointerId, {
          ...press,
          buttons: resolveDirection(press.pad, event.clientX, event.clientY),
        });
      } else {
        const hit = controlAt(root, event.clientX, event.clientY);
        const rect = root.getBoundingClientRect();
        const inside =
          event.clientX >= rect.left &&
          event.clientX <= rect.right &&
          event.clientY >= rect.top &&
          event.clientY <= rect.bottom;

        // Handing over happens between buttons only; the stick is not a target a
        // button press migrates to. Off the pad entirely, the button lets go.
        const buttons =
          hit && hit.control !== 'dpad'
            ? new Set(buttonsOf(hit.control))
            : inside
              ? press.buttons
              : NOTHING;
        pointers.current.set(event.pointerId, { ...press, buttons });
      }
      sync();
    },
    [sync],
  );

  const onPointerEnd = useCallback(
    (event: PointerEvent<HTMLDivElement>) => {
      if (!pointers.current.delete(event.pointerId)) return;
      sync();
    },
    [sync],
  );

  // The arrows and the head only light up for what the console is being told.
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
        {/* Both face buttons at once, for the run-and-jump a single thumb cannot
            do: it sits above B/A so the same thumb reaches it, and it lights up
            whenever the console is being told both are down. */}
        <Button
          block
          size="small"
          className="tpad__key tpad__key--both"
          data-control="AB"
          data-pressed={direction('A') && direction('B')}
          disabled={disabled}
          tabIndex={-1}
        >
          A+B
        </Button>

        <div className="tpad__pair">
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
    </div>
  );
}
