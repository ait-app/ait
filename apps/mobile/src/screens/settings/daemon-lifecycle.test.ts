import { expect, test, vi } from "vitest";
import { restartDaemonFromSettings, updateDaemonFromSettings } from "./daemon-lifecycle";

test("Ait restart completes when the same process serves a fresh instance", async () => {
  let startedAt = "2026-09-28T01:00:00.000Z";
  await restartDaemonFromSettings("ait", "settings", {
    getStatus: async () => ({ pid: 10, serverId: "ait", version: "0.0.7", startedAt }),
    restartServer: async () => {
      startedAt = "2026-09-28T01:00:01.000Z";
    },
  });
  expect(startedAt).toBe("2026-09-28T01:00:01.000Z");
});

test("settings restart completes when a replacement worker is observed without a sampled disconnect", async () => {
  let pid = 10;
  await restartDaemonFromSettings("daemon", "settings", {
    getStatus: async () => ({ pid, version: "1.0.0", serverId: "daemon" }),
    restartServer: async () => {
      pid = 11;
    },
  });
  expect(pid).toBe(11);
});

test("a status permission failure prevents the restart request", async () => {
  let restarted = false;
  await expect(
    restartDaemonFromSettings("daemon", "settings", {
      getStatus: async () => {
        throw new Error("Permission denied");
      },
      restartServer: async () => {
        restarted = true;
      },
    }),
  ).rejects.toThrow("Permission denied");
  expect(restarted).toBe(false);
});

test("a different responder fails confirmation immediately", async () => {
  let serverId = "daemon";
  await expect(
    restartDaemonFromSettings("daemon", "settings", {
      getStatus: async () => ({ pid: 10, version: "1.0.0", serverId }),
      restartServer: async () => {
        serverId = "other";
      },
    }),
  ).rejects.toThrow("identity changed");
});

test("a lost restart acknowledgment can still confirm the replacement", async () => {
  let pid = 10;
  await restartDaemonFromSettings("daemon", "settings", {
    getStatus: async () => ({ pid, version: "1.0.0", serverId: "daemon" }),
    restartServer: async () => {
      pid = 11;
      throw Object.assign(new Error("Connection lost"), { code: "DAEMON_CONNECTION_LOST" });
    },
  });
  expect(pid).toBe(11);
});

test("installation and worker version confirmation are separate outcomes", async () => {
  let pid = 10;
  await expect(
    updateDaemonFromSettings("daemon", {
      getStatus: async () => ({ pid, version: "1.0.0", serverId: "daemon" }),
      updateDaemon: async () => {
        pid = 11;
        return { success: true, error: null, newVersion: "2.0.0" };
      },
    }),
  ).rejects.toThrow("Package installed; replacement worker version was not confirmed");
});

test("an installed version is confirmed only in its replacement worker", async () => {
  let pid = 10,
    version = "1.0.0";
  await expect(
    updateDaemonFromSettings("daemon", {
      getStatus: async () => ({ pid, version, serverId: "daemon" }),
      updateDaemon: async () => {
        pid = 11;
        version = "2.0.0";
        return { success: true, error: null, newVersion: version };
      },
    }),
  ).resolves.toEqual({ workerVersion: "2.0.0" });
});

test("RPC errors mentioning transport are not retried", async () => {
  let requested = false;
  await expect(
    restartDaemonFromSettings("daemon", "settings", {
      getStatus: async () => {
        if (requested)
          throw Object.assign(new Error("Connection policy denied by plugin"), {
            code: "permission_denied",
          });
        return { pid: 10, version: "1.0.0", serverId: "daemon" };
      },
      restartServer: async () => {
        requested = true;
      },
    }),
  ).rejects.toThrow("Connection policy denied by plugin");
});

function desktopRestartFixture() {
  const previous = {
    serverId: "local",
    status: "running" as const,
    listen: "127.0.0.1:7316",
    hostname: "desktop",
    pid: 10,
    home: "/test",
    version: "1.0.0",
    desktopManaged: true,
    ownedByDesktop: true,
    startedAt: "2026-10-08T00:00:00Z",
    error: null,
  };
  const current = {
    ...previous,
    pid: 11,
    listen: "0.0.0.0:8080",
    connectAddress: "127.0.0.1:8080",
  };
  return {
    current,
    deps: {
      getStatus: vi.fn().mockRejectedValue(new Error("Old connection must not be queried")),
      restartServer: vi.fn(),
      desktop: {
        getStatus: vi.fn().mockResolvedValue(previous),
        restart: vi.fn().mockResolvedValue(current),
        reconnect: vi.fn().mockResolvedValue(undefined),
      },
    },
  };
}

test("desktop restart relaunches with saved settings and reconnects at the new listener", async () => {
  const { deps, current } = desktopRestartFixture();
  await restartDaemonFromSettings("local", "settings", deps);
  expect(deps.desktop.restart).toHaveBeenCalledTimes(1);
  expect(deps.desktop.reconnect).toHaveBeenCalledWith(current);
  expect(deps.getStatus).not.toHaveBeenCalled();
  expect(deps.restartServer).not.toHaveBeenCalled();
});

test.each([{ serverId: "other" }, { ownedByDesktop: false }])(
  "desktop restart checks current ownership and identity: %j",
  async (changes) => {
    const { deps, current } = desktopRestartFixture();
    deps.desktop.getStatus.mockResolvedValue({ ...current, ...changes });
    await expect(restartDaemonFromSettings("local", "settings", deps)).rejects.toThrow(
      "no longer managed",
    );
    expect(deps.desktop.restart).not.toHaveBeenCalled();
    expect(deps.restartServer).not.toHaveBeenCalled();
  },
);

test("a desktop restart failure does not fall back to restarting the old service", async () => {
  const { deps } = desktopRestartFixture();
  deps.desktop.restart.mockRejectedValue(new Error("Address in use"));
  await expect(restartDaemonFromSettings("local", "settings", deps)).rejects.toThrow(
    "Address in use",
  );
  expect(deps.desktop.reconnect).not.toHaveBeenCalled();
  expect(deps.restartServer).not.toHaveBeenCalled();
});

test.each([{ serverId: "other" }, { status: "errored" }])(
  "does not register an invalid replacement daemon: %j",
  async (changes) => {
    const { deps, current } = desktopRestartFixture();
    deps.desktop.restart.mockResolvedValue({ ...current, ...changes });
    await expect(restartDaemonFromSettings("local", "settings", deps)).rejects.toThrow(
      "could not be confirmed",
    );
    expect(deps.desktop.reconnect).not.toHaveBeenCalled();
  },
);

test("reports connection registration failures after the desktop daemon restarts", async () => {
  const { deps } = desktopRestartFixture();
  deps.desktop.reconnect.mockRejectedValue(new Error("Registration failed"));
  await expect(restartDaemonFromSettings("local", "settings", deps)).rejects.toThrow(
    "Registration failed",
  );
  expect(deps.restartServer).not.toHaveBeenCalled();
});
