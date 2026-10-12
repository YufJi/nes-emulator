import type { RefObject } from 'react';
import { Button, Card, Tag, Title } from 'animal-island-ui';
import { FolderIcon } from 'naive-icons';
import { useIntegerScale } from '../hooks/useIntegerScale';
import type { EmulatorState } from '../emulator/NesEmulator';

interface GameScreenProps {
  stageRef: RefObject<HTMLDivElement>;
  canvasRef: RefObject<HTMLCanvasElement>;
  /** Native frame size from the core; null until WASM has loaded. */
  screen: { width: number; height: number } | null;
  state: EmulatorState;
  romName: string | null;
  busy: boolean;
  onPickRom: () => void;
}

export function GameScreen({
  stageRef,
  canvasRef,
  screen,
  state,
  romName,
  busy,
  onPickRom,
}: GameScreenProps) {
  useIntegerScale(stageRef, canvasRef, screen?.width ?? null, screen?.height ?? null);

  return (
    <section className="stage" ref={stageRef}>
      <canvas
        ref={canvasRef}
        className="stage__canvas"
        aria-label="NES 画面"
      />

      {state === 'idle' && (
        <div className="stage__overlay">
          <Card type="dashed" className="stage__card">
            <Title size="middle" color="app-blue">
              把 .nes 文件拖到这里
            </Title>
            <p className="stage__hint">
              {busy ? '正在加载 WASM 内核…' : '或者选择本地文件 / 填入链接 / 打开游戏列表'}
            </p>
            <Button
              type="primary"
              icon={<FolderIcon size={18} color="currentColor" aria-hidden />}
              onClick={onPickRom}
              loading={busy}
            >
              选择 ROM 文件
            </Button>
          </Card>
        </div>
      )}

      {state === 'paused' && (
        <div className="stage__overlay stage__overlay--soft">
          <Card color="app-yellow" className="stage__card">
            <Title size="small" color="app-orange">
              已暂停
            </Title>
            <p className="stage__hint">按 P 或点击继续按钮恢复游戏</p>
          </Card>
        </div>
      )}

      {romName && state !== 'idle' && (
        <Tag className="stage__label" color="brown" variant="solid">
          <span className="ellipsis">{romName}</span>
        </Tag>
      )}
    </section>
  );
}
