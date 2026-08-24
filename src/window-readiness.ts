export function waitForCompositorWarmup(timeoutMs = 100): Promise<void> {
  return new Promise((resolve) => {
    let completed = false;
    let timeoutId = 0;
    const complete = (): void => {
      if (completed) return;
      completed = true;
      window.clearTimeout(timeoutId);
      resolve();
    };

    timeoutId = window.setTimeout(complete, timeoutMs);
    window.requestAnimationFrame(() => window.requestAnimationFrame(complete));
  });
}
