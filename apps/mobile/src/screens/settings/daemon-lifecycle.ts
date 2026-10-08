import type { DesktopDaemonStatus } from "@/desktop/daemon/desktop-daemon";

interface WorkerStatus {
  pid: number;
  serverId: string;
  version: string | null;
  startedAt?: string | null;
}
interface StatusReader {
  getStatus: () => Promise<WorkerStatus>;
}

export interface SettingsDaemonRestartDeps extends StatusReader {
  restartServer: (reason: string) => Promise<unknown>;
  desktop?: {
    getStatus: () => Promise<DesktopDaemonStatus>;
    restart: () => Promise<DesktopDaemonStatus>;
    reconnect: (status: DesktopDaemonStatus) => Promise<void>;
  };
}

/** Confirm a fresh service instance, including Ait's restart within the same process. */
export async function restartDaemonFromSettings(
  hostServerId: string,
  reason: string,
  deps: SettingsDaemonRestartDeps,
): Promise<void> {
  if (deps.desktop) {
    const previous = await deps.desktop.getStatus();
    if (previous.serverId !== hostServerId || !previous.ownedByDesktop) {
      throw new Error("The selected daemon is no longer managed by this desktop.");
    }
    const current = await deps.desktop.restart();
    if (current.serverId !== hostServerId || current.status !== "running") {
      throw new Error("The restarted desktop daemon identity or status could not be confirmed.");
    }
    await deps.desktop.reconnect(current);
    return;
  }
  const previous = await readSelectedWorker(hostServerId, deps);
  let acknowledged = false;
  try {
    await deps.restartServer(reason);
    acknowledged = true;
  } catch (error) {
    if (!isReconnectFailure(error)) throw error;
  }
  try {
    await observeReplacement(previous, deps);
  } catch (error) {
    throw new Error(`Restart acknowledged: ${acknowledged}. ${String(error)}`, { cause: error });
  }
}

export async function updateDaemonFromSettings(
  hostServerId: string,
  deps: StatusReader & {
    updateDaemon: () => Promise<{
      success: boolean;
      error: string | null;
      newVersion: string | null;
    }>;
  },
): Promise<{ workerVersion: string }> {
  const previous = await readSelectedWorker(hostServerId, deps);
  const installed = await deps.updateDaemon();
  if (!installed.success) throw new Error(installed.error ?? "Package installation failed");
  try {
    const worker = await observeReplacement(previous, deps);
    if (!worker.version || !installed.newVersion || worker.version !== installed.newVersion) {
      throw new Error(
        `Expected installed version ${installed.newVersion ?? "unknown"}; observed worker ${worker.version}.`,
      );
    }
    return { workerVersion: worker.version };
  } catch (error) {
    throw new Error(
      `Package installed; replacement worker version was not confirmed. ${String(error)}`,
      { cause: error },
    );
  }
}

async function readSelectedWorker(serverId: string, deps: StatusReader): Promise<WorkerStatus> {
  const worker = await deps.getStatus();
  if (worker.serverId !== serverId) throw new Error("Daemon identity changed");
  return worker;
}

async function observeReplacement(
  previous: WorkerStatus,
  deps: StatusReader,
): Promise<WorkerStatus> {
  const deadline = Date.now() + 600_000;
  while (Date.now() < deadline) {
    try {
      const current = await readSelectedWorker(previous.serverId, deps);
      if (
        current.pid !== previous.pid ||
        (previous.startedAt && current.startedAt && current.startedAt !== previous.startedAt)
      )
        return current;
    } catch (error) {
      if (!isReconnectFailure(error)) throw error;
    }
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  throw new Error("Replacement worker could not be confirmed. Check daemon status and logs.");
}

function isReconnectFailure(error: unknown): boolean {
  return Boolean(
    error &&
    typeof error === "object" &&
    "code" in error &&
    ["DAEMON_CONNECTION_LOST", "DAEMON_REQUEST_TIMEOUT"].includes(String(error.code)),
  );
}
