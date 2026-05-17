export type PointerPoint = {
  x: number;
  y: number;
};

export const VIEWER_DRAG_SELECT_THRESHOLD_PX = 4;

export function pointerMovedBeyondClickThreshold(
  start: PointerPoint | null,
  current: PointerPoint,
  threshold = VIEWER_DRAG_SELECT_THRESHOLD_PX,
): boolean {
  if (!start) return false;
  return Math.abs(current.x - start.x) > threshold || Math.abs(current.y - start.y) > threshold;
}

export function shouldHandleSelectionClick(input: {
  hideModelWhileBusy: boolean;
  selectionMode: boolean;
  pointerDownAt: PointerPoint | null;
  current: PointerPoint;
}): boolean {
  if (input.hideModelWhileBusy || !input.selectionMode) return false;
  return shouldHandleViewerClick(input);
}

export function shouldHandleViewerClick(input: {
  hideModelWhileBusy: boolean;
  pointerDownAt: PointerPoint | null;
  current: PointerPoint;
}): boolean {
  if (input.hideModelWhileBusy) return false;
  return !pointerMovedBeyondClickThreshold(input.pointerDownAt, input.current);
}
