/** Keep native event callbacks from creating unhandled JS exceptions or rejections. */
export function runTerminalCallback(callback: () => void | Promise<void>): void {
  const report = (error: unknown) => console.warn("Terminal callback failed", error);
  try {
    const pending = callback();
    if (pending) void pending.catch(report);
  } catch (error) {
    report(error);
  }
}
