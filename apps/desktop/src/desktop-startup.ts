export interface DesktopStartupDependencies {
  /** Apply the login shell environment; the GUI starts while this runs. */
  inheritLoginShellEnv: () => Promise<void>;
  /** Start the GUI. Anything that spawns processes or reads the environment awaits `environmentReady`. */
  bootstrapGui: (environmentReady: Promise<void>) => Promise<void>;
}

/** Channel registry with Electron's `ipcMain.handle` shape. */
export interface IpcHandleRegistry {
  handle(channel: string, listener: (event: unknown, ...args: unknown[]) => unknown): void;
}

export async function runDesktopStartup(deps: DesktopStartupDependencies): Promise<void> {
  // A failed lookup keeps the inherited environment, so it must still release waiting work.
  const environmentReady = deps.inheritLoginShellEnv().then(
    () => undefined,
    () => undefined,
  );
  await Promise.all([environmentReady, deps.bootstrapGui(environmentReady)]);
}

/**
 * Register handlers that run only after `environmentReady`, so requests arriving while the
 * login shell environment resolves still spawn processes with the user's `PATH`.
 */
export function afterEnvironment(
  ipc: IpcHandleRegistry,
  environmentReady: Promise<void>,
): IpcHandleRegistry {
  return {
    handle: (channel, listener) =>
      ipc.handle(channel, async (event, ...args) => {
        await environmentReady;
        return await listener(event, ...args);
      }),
  };
}
