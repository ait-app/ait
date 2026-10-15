// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { resolveUsageHostId } from "@/usage/model";
import { toUsageReport } from "@/usage/native-report";
import { resolvePinnedUsage } from "@/usage/pinned";
import type { UsagePreferences } from "@/usage/preferences";
import type { HostRuntimeConnectionStatus, useHostRuntimeConnectionStatuses } from "./host-runtime";

const require = createRequire(import.meta.url);

class ConnectionStore {
  private version = 0;
  private listeners = new Set<() => void>();
  private statuses = new Map<string, HostRuntimeConnectionStatus>([
    ["local", "online"],
    ["remote", "connecting"],
  ]);

  getVersion() {
    return this.version;
  }

  getSnapshot(serverId: string) {
    const connectionStatus = this.statuses.get(serverId);
    return connectionStatus ? { connectionStatus } : null;
  }

  subscribeAll(listener: () => void) {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  setStatus(serverId: string, status: HostRuntimeConnectionStatus) {
    this.statuses.set(serverId, status);
    this.version += 1;
    for (const listener of this.listeners) listener();
  }
}

/** Exercise the shipped hook after the same compiler that optimizes Expo production builds. */
function compileConnectionHook(store: ConnectionStore): typeof useHostRuntimeConnectionStatuses {
  const source = readFileSync(
    resolve(dirname(fileURLToPath(import.meta.url)), "host-runtime.ts"),
    "utf8",
  );
  const start = source.indexOf("export function useHostRuntimeConnectionStatuses(");
  const end = source.indexOf("\nexport function ", start + 1);
  expect(start).toBeGreaterThanOrEqual(0);
  expect(end).toBeGreaterThan(start);
  const compiled: { code: string } = require("@babel/core").transformSync(
    `import { useMemo, useSyncExternalStore } from "react";\n${source.slice(start, end)}`,
    {
      babelrc: false,
      configFile: false,
      filename: "connection-hook.ts",
      plugins: [
        ["babel-plugin-react-compiler", { target: "19" }],
        "@babel/plugin-transform-typescript",
        "@babel/plugin-transform-modules-commonjs",
      ],
    },
  );
  const exports: { useHostRuntimeConnectionStatuses?: typeof useHostRuntimeConnectionStatuses } =
    {};
  new Function("require", "exports", "getHostRuntimeStore", compiled.code)(
    require,
    exports,
    () => store,
  );
  if (!exports.useHostRuntimeConnectionStatuses)
    throw new Error("Connection hook was not compiled");
  return exports.useHostRuntimeConnectionStatuses;
}

afterEach(cleanup);

describe("compiled aggregate host connection subscription", () => {
  it("tracks connection changes with a stable host list, including disconnect and reconnect", () => {
    const store = new ConnectionStore();
    const useStatuses = compileConnectionHook(store);
    const serverIds = ["local", "remote"];
    const { result } = renderHook(() => useStatuses(serverIds));
    expect(result.current.get("remote")).toBe("connecting");

    for (const status of ["online", "offline", "online"] as const) {
      act(() => store.setStatus("remote", status));
      expect(result.current.get("remote")).toBe(status);
      expect(result.current.get("local")).toBe("online");
    }
  });

  it("shows the Antigravity pin when the selected usage host connects after the local host", () => {
    const store = new ConnectionStore();
    const useStatuses = compileConnectionHook(store);
    const serverIds = ["local", "remote"];
    const fetchedAt = "2026-10-15T00:00:00Z";
    const codex = toUsageReport(
      {
        providerId: "codex",
        displayName: "Codex",
        status: "available",
        planLabel: null,
        windows: [{ id: "codex:primary", label: "Primary", usedPct: 27 }],
      },
      fetchedAt,
    );
    const antigravity = toUsageReport(
      {
        providerId: "antigravity",
        displayName: "Antigravity",
        status: "available",
        planLabel: null,
        windows: [{ id: "gemini-weekly", label: "Gemini Models · Weekly", usedPct: 0 }],
      },
      fetchedAt,
    );
    const preferences: UsagePreferences = {
      serverId: "remote",
      displayAs: "used",
      pins: [
        { sourceId: "antigravity", windowId: "gemini-weekly" },
        { sourceId: "codex", windowId: "codex:primary" },
      ],
    };
    const { result } = renderHook(() => {
      const statuses = useStatuses(serverIds);
      const hostId = resolveUsageHostId({
        pickedServerId: preferences.serverId,
        activeServerId: "local",
        hosts: serverIds.map((serverId) => ({
          serverId,
          label: serverId,
          isConnected: statuses.get(serverId) === "online",
          supportsUsage: true,
        })),
      });
      return resolvePinnedUsage(hostId === "remote" ? [antigravity, codex] : [codex], preferences);
    });
    expect(result.current.map((source) => source.key)).toEqual(["codex"]);

    act(() => store.setStatus("remote", "online"));

    expect(result.current.map((source) => source.key)).toEqual(["antigravity", "codex"]);
    expect(result.current[0]?.windows[0]?.percentText).toBe("0%");
    expect(result.current[1]?.windows[0]?.percentText).toBe("27%");
  });
});
