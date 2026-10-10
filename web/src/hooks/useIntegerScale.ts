import { useEffect, type RefObject } from 'react';
import { NES_HEIGHT, NES_WIDTH } from '../emulator/constants';

/**
 * Size the canvas with whole-number scale factors so that every NES pixel maps
 * to an identical block of screen pixels. The canvas backing store stays at the
 * native 256x240; only its CSS box grows.
 */
export function useIntegerScale(
  stageRef: RefObject<HTMLElement>,
  canvasRef: RefObject<HTMLCanvasElement>,
): void {
  useEffect(() => {
    const stage = stageRef.current;
    const canvas = canvasRef.current;
    if (!stage || !canvas) return;

    const applyScale = () => {
      const { clientWidth, clientHeight } = stage;
      if (clientWidth === 0 || clientHeight === 0) return;

      const fit = Math.min(clientWidth / NES_WIDTH, clientHeight / NES_HEIGHT);
      const scale = fit >= 1 ? Math.floor(fit) : fit;

      canvas.style.width = `${Math.round(NES_WIDTH * scale)}px`;
      canvas.style.height = `${Math.round(NES_HEIGHT * scale)}px`;
    };

    const observer = new ResizeObserver(applyScale);
    observer.observe(stage);
    applyScale();

    return () => observer.disconnect();
  }, [stageRef, canvasRef]);
}
