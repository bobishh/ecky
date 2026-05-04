export type WindowRect = {
  x: number;
  y: number;
  width: number;
  height: number;
};

export type WindowMinSize = {
  width: number;
  height: number;
};

export type ViewportSize = {
  width: number;
  height: number;
};

function clamp(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(value, max));
}

export function fitRectToViewport(
  rect: WindowRect,
  minSize: WindowMinSize,
  viewport: ViewportSize,
): WindowRect {
  const width = clamp(rect.width, Math.min(minSize.width, viewport.width), viewport.width);
  const height = clamp(rect.height, Math.min(minSize.height, viewport.height), viewport.height);
  const maxX = Math.max(0, viewport.width - width);
  const maxY = Math.max(0, viewport.height - height);
  return {
    x: clamp(rect.x, 0, maxX),
    y: clamp(rect.y, 0, maxY),
    width,
    height,
  };
}
