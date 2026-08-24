/** Applies the same integer RGBA block averaging used by Rust's mosaic renderer. */
export function mosaicRgbaPixels(
  data: Uint8ClampedArray,
  width: number,
  height: number,
  blockSize: number,
): void {
  if (data.length !== width * height * 4) {
    throw new Error("RGBA buffer length does not match its dimensions");
  }
  const block = Math.max(2, Math.round(blockSize));
  for (let blockY = 0; blockY < height; blockY += block) {
    for (let blockX = 0; blockX < width; blockX += block) {
      const endX = Math.min(blockX + block, width);
      const endY = Math.min(blockY + block, height);
      const totals = [0, 0, 0, 0];
      let count = 0;

      for (let pixelY = blockY; pixelY < endY; pixelY += 1) {
        for (let pixelX = blockX; pixelX < endX; pixelX += 1) {
          const offset = (pixelY * width + pixelX) * 4;
          totals[0] += data[offset];
          totals[1] += data[offset + 1];
          totals[2] += data[offset + 2];
          totals[3] += data[offset + 3];
          count += 1;
        }
      }

      const averaged = totals.map((total) => Math.floor(total / count));
      for (let pixelY = blockY; pixelY < endY; pixelY += 1) {
        for (let pixelX = blockX; pixelX < endX; pixelX += 1) {
          const offset = (pixelY * width + pixelX) * 4;
          data[offset] = averaged[0];
          data[offset + 1] = averaged[1];
          data[offset + 2] = averaged[2];
          data[offset + 3] = averaged[3];
        }
      }
    }
  }
}
