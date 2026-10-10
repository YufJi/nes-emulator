import type { RefObject } from 'react';
import { NES_HEIGHT, NES_WIDTH } from '../emulator/constants';
import { useIntegerScale } from '../hooks/useIntegerScale';
import type { EmulatorState } from '../emulator/NesEmulator';

interface GameScreenProps {
  stageRef: RefObject<HTMLDivElement>;
  canvasRef: RefObject<HTMLCanvasElement>;
  state: EmulatorState;
  romName: string | null;
  busy: boolean;
  onPickRom: () => void;
}

export function GameScreen({
  stageRef,
  canvasRef,
  state,
  romName,
  busy,
  onPickRom,
}: GameScreenProps) {
  useIntegerScale(stageRef, canvasRef);

  return (
    <section className="stage" ref={stageRef}>
      <canvas
        ref={canvasRef}
        className="stage__canvas"
        width={NES_WIDTH}
        height={NES_HEIGHT}
        aria-label="NES 画面"
      />

      {state === 'idle' && (
        <div className="stage__overlay">
          <p className="stage__headline">把 .nes 文件拖到这里</p>
          <p className="stage__hint">
            {busy ? '正在加载 WASM 内核…' : '或者点击下面的按钮选择 ROM'}
          </p>
          <button type="button" className="button button--primary" onClick={onPickRom} disabled={busy}>
            选择 ROM 文件
          </button>
        </div>
      )}

      {state === 'paused' && (
        <div className="stage__overlay stage__overlay--translucent">
          <p className="stage__headline">已暂停</p>
          <p className="stage__hint">按 P 或点击继续按钮恢复游戏</p>
        </div>
      )}

      {romName && state !== 'idle' && (
        <div className="stage__label" title={romName}>
          {romName}
        </div>
      )}
    </section>
  );
}
