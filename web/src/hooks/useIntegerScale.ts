import { useEffect, type RefObject } from 'react';

/**
 * Size the canvas with whole-number scale factors so that every NES pixel maps
 * to an identical block of screen pixels. The canvas backing store stays at the
 * core's native resolution (`width` x `height`); only its CSS box grows.
 *
 * Pass `integer = false` where the picture has to make the most of a small
 * screen (the phone layout): a fractional fit beats a crisp but tiny 1× picture.
 *
 * Does nothing until the core has reported the frame size, which arrives with
 * the WASM module.
 */
export function useIntegerScale(
  stageRef: RefObject<HTMLElement>,
  canvasRef: RefObject<HTMLCanvasElement>,
  width: number | null,
  height: number | null,
  integer = true,
): void {
  useEffect(() => {
    const stage = stageRef.current;
    const canvas = canvasRef.current;
    if (!stage || !canvas || !width || !height) return;

    const applyScale = () => {
      const { clientWidth, clientHeight } = stage;
      if (clientWidth === 0 || clientHeight === 0) return;

      const fit = Math.min(clientWidth / width, clientHeight / height);
      // Below 1× there is nothing to quantise — shrinking to fit is the only
      // option — so rounding only ever applies to an enlarging factor.
      const scale = integer && fit >= 1 ? Math.floor(fit) : fit;

      canvas.style.width = `${Math.round(width * scale)}px`;
      canvas.style.height = `${Math.round(height * scale)}px`;
    };

    const observer = new ResizeObserver(applyScale);
    observer.observe(stage);
    applyScale();

    return () => observer.disconnect();
  }, [stageRef, canvasRef, width, height, integer]);
}
