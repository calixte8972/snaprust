export type Point = Readonly<{
  x: number;
  y: number;
}>;

export type SelectionRect = Readonly<{
  x: number;
  y: number;
  width: number;
  height: number;
  viewportWidth: number;
  viewportHeight: number;
}>;

export type PhysicalSelectionRect = Readonly<{
  x: number;
  y: number;
  width: number;
  height: number;
}>;

export function normalizeSelection(
  start: Point,
  end: Point,
  viewportWidth: number,
  viewportHeight: number,
): SelectionRect {
  const x1 = Math.min(start.x, end.x);
  const y1 = Math.min(start.y, end.y);
  const x2 = Math.max(start.x, end.x);
  const y2 = Math.max(start.y, end.y);

  return {
    x: x1,
    y: y1,
    width: x2 - x1,
    height: y2 - y1,
    viewportWidth,
    viewportHeight,
  };
}

export function selectionHasArea(selection: SelectionRect): boolean {
  return selection.width >= 1 && selection.height >= 1;
}

/**
 * Projects the browser selection onto the captured virtual desktop pixels.
 * The scale is calibrated from the actual WebView viewport and capture size,
 * so it remains correct when adjacent monitors use different Windows DPI scales.
 */
export function selectionToPhysicalPixels(
  selection: SelectionRect,
  captureWidth: number,
  captureHeight: number,
  desktopX: number,
  desktopY: number,
): PhysicalSelectionRect {
  const scaleX = captureWidth / selection.viewportWidth;
  const scaleY = captureHeight / selection.viewportHeight;
  const left = Math.max(0, Math.min(captureWidth, Math.floor(selection.x * scaleX)));
  const top = Math.max(0, Math.min(captureHeight, Math.floor(selection.y * scaleY)));
  const right = Math.max(
    left,
    Math.min(captureWidth, Math.ceil((selection.x + selection.width) * scaleX)),
  );
  const bottom = Math.max(
    top,
    Math.min(captureHeight, Math.ceil((selection.y + selection.height) * scaleY)),
  );

  return {
    x: desktopX + left,
    y: desktopY + top,
    width: right - left,
    height: bottom - top,
  };
}

function squaredDistanceToSegment(point: Point, start: Point, end: Point): number {
  let x = start.x;
  let y = start.y;
  const dx = end.x - x;
  const dy = end.y - y;

  if (dx !== 0 || dy !== 0) {
    const offset = ((point.x - x) * dx + (point.y - y) * dy) / (dx * dx + dy * dy);
    if (offset > 1) {
      x = end.x;
      y = end.y;
    } else if (offset > 0) {
      x += dx * offset;
      y += dy * offset;
    }
  }

  const distanceX = point.x - x;
  const distanceY = point.y - y;
  return distanceX * distanceX + distanceY * distanceY;
}

/**
 * Simplifies a freehand stroke with an iterative Ramer-Douglas-Peucker pass.
 * The first and last points are always retained and the iterative stack avoids
 * overflowing the JavaScript call stack for very long strokes.
 */
export function simplifyPolyline<PointType extends Point>(
  points: ReadonlyArray<PointType>,
  tolerance: number,
): PointType[] {
  if (points.length <= 2 || !Number.isFinite(tolerance) || tolerance <= 0) {
    return [...points];
  }

  const squaredTolerance = tolerance * tolerance;
  const radialPoints: PointType[] = [points[0]];
  let previous = points[0];
  for (let index = 1; index < points.length - 1; index += 1) {
    const point = points[index];
    const dx = point.x - previous.x;
    const dy = point.y - previous.y;
    if (dx * dx + dy * dy > squaredTolerance) {
      radialPoints.push(point);
      previous = point;
    }
  }
  radialPoints.push(points.at(-1)!);
  if (radialPoints.length <= 2) return radialPoints;

  const markers = new Uint8Array(radialPoints.length);
  markers[0] = 1;
  markers[radialPoints.length - 1] = 1;
  const stack: Array<readonly [number, number]> = [[0, radialPoints.length - 1]];

  while (stack.length > 0) {
    const [first, last] = stack.pop()!;
    let farthestIndex = -1;
    let farthestDistance = squaredTolerance;
    for (let index = first + 1; index < last; index += 1) {
      const distance = squaredDistanceToSegment(
        radialPoints[index],
        radialPoints[first],
        radialPoints[last],
      );
      if (distance > farthestDistance) {
        farthestDistance = distance;
        farthestIndex = index;
      }
    }

    if (farthestIndex >= 0) {
      markers[farthestIndex] = 1;
      stack.push([first, farthestIndex], [farthestIndex, last]);
    }
  }

  return radialPoints.filter((_, index) => markers[index] === 1);
}
