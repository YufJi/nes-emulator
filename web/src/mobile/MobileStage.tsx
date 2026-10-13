import type { ReactNode, RefObject } from 'react';
import { Button, Card, Title } from 'animal-island-ui';
import { PlayIcon } from 'naive-icons';
import { useIntegerScale } from '../hooks/useIntegerScale';
import type { EmulatorState } from '../emulator/NesEmulator';

interface MobileStageProps {
  stageRef: RefObject<HTMLDivElement>;
  canvasRef: RefObject<HTMLCanvasElement>;
  /** Native frame size from the core; null until WASM has loaded. */
  screen: { width: number; height: number } | null;
  state: EmulatorState;
  /** Shown on top of the picture while no game is running. */
  children: ReactNode;
  onResume: () => void;
}

/**
 * The picture, on top of the phone layout.
 *
 * The stage is sized like the picture rather than like the page: on a tall
 * screen it takes the full width, on a wide one its width follows the height it
 * can have, so the frame never lies about the picture it holds.
 *
 * Unlike the wide-screen stage the canvas is also allowed a fractional scale: a
 * 1× picture that leaves a third of a phone screen empty is worse than a
 * slightly uneven pixel grid, and the picture is what the player is looking at.
 */
export function MobileStage({
  stageRef,
  canvasRef,
  screen,
  state,
  children,
  onResume,
}: MobileStageProps) {
  useIntegerScale(stageRef, canvasRef, screen?.width ?? null, screen?.height ?? null, false);

  return (
    <section
      className="stage stage--phone"
      ref={stageRef}
      /* The window is shaped like the picture it holds, and that shape comes
         from the core rather than being repeated here. */
      style={screen ? { aspectRatio: `${screen.width} / ${screen.height}` } : undefined}
    >
      <canvas ref={canvasRef} className="stage__canvas" aria-label="NES 画面" />

      {state === 'idle' && <div className="stage__overlay">{children}</div>}

      {state === 'paused' && (
        // Tapping the picture resumes: on a phone the pause overlay is in the
        // way of the game, so it should not need aiming at a small button.
        <div className="stage__overlay stage__overlay--soft" onClick={onResume}>
          <Card color="app-yellow" className="stage__card">
            <Title size="small" color="app-orange">
              已暂停
            </Title>
            <p className="stage__hint">点按画面继续；其他操作在右上角工具栏里。</p>
            <Button
              type="primary"
              icon={<PlayIcon size={18} color="currentColor" aria-hidden />}
              onClick={onResume}
            >
              继续游戏
            </Button>
          </Card>
        </div>
      )}
    </section>
  );
}
