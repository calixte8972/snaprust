export type AsyncTask<T> = () => Promise<T>;

/** Runs asynchronous work with a fixed concurrency ceiling. */
export function createAsyncPool(maximumConcurrency: number): <T>(task: AsyncTask<T>) => Promise<T> {
  const limit = Math.max(1, Math.floor(maximumConcurrency));
  const waiting: Array<() => void> = [];
  let active = 0;

  const release = (): void => {
    active -= 1;
    waiting.shift()?.();
  };

  return async <T>(task: AsyncTask<T>): Promise<T> => {
    if (active >= limit) {
      await new Promise<void>((resolve) => waiting.push(resolve));
    }
    active += 1;
    try {
      return await task();
    } finally {
      release();
    }
  };
}
