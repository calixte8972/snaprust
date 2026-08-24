export type PinPoint = Readonly<{ x: number; y: number }>;
export type PinSize = Readonly<{ width: number; height: number }>;
export type PinWorkArea = Readonly<{ position: PinPoint; size: PinSize }>;

export function clamp(value: number, minimum: number, maximum: number): number {
  return Math.max(minimum, Math.min(maximum, value));
}

export function minimumPinZoom(
  baseWidth: number,
  baseHeight: number,
  minimumVisibleEdge: number,
): number {
  return Math.min(
    1,
    Math.max(0.1, minimumVisibleEdge / baseWidth, minimumVisibleEdge / baseHeight),
  );
}

export function clampWindowAxis(
  desired: number,
  windowSize: number,
  workAreaStart: number,
  workAreaSize: number,
  minimumVisibleEdge: number,
): number {
  if (windowSize <= workAreaSize) {
    return clamp(desired, workAreaStart, workAreaStart + workAreaSize - windowSize);
  }
  const visible = Math.min(minimumVisibleEdge, windowSize, workAreaSize);
  return clamp(
    desired,
    workAreaStart + visible - windowSize,
    workAreaStart + workAreaSize - visible,
  );
}

export function zoomedWindowGeometry(
  position: PinPoint,
  size: PinSize,
  anchor: PinPoint,
  ratio: number,
  workArea: PinWorkArea | null,
  minimumVisibleEdge: number,
): Readonly<{ position: PinPoint; size: PinSize }> {
  const targetSize = {
    width: Math.max(1, Math.round(size.width * ratio)),
    height: Math.max(1, Math.round(size.height * ratio)),
  };
  let x = Math.round(position.x + size.width * anchor.x - targetSize.width * anchor.x);
  let y = Math.round(position.y + size.height * anchor.y - targetSize.height * anchor.y);
  if (workArea) {
    x = clampWindowAxis(
      x,
      targetSize.width,
      workArea.position.x,
      workArea.size.width,
      minimumVisibleEdge,
    );
    y = clampWindowAxis(
      y,
      targetSize.height,
      workArea.position.y,
      workArea.size.height,
      minimumVisibleEdge,
    );
  }
  return { position: { x, y }, size: targetSize };
}
