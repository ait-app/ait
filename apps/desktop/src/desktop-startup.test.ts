import { describe, expect, it } from "vitest";
import { afterEnvironment, runDesktopStartup, type IpcHandleRegistry } from "./desktop-startup";

function deferred(): {
  promise: Promise<void>;
  resolve: () => void;
  reject: (error: Error) => void;
} {
  let resolve!: () => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<void>((onResolve, onReject) => {
    resolve = onResolve;
    reject = onReject;
  });
  return { promise, resolve, reject };
}

describe("desktop startup", () => {
  it("starts the GUI while the login environment resolves", async () => {
    const calls: string[] = [];
    const environment = deferred();
    let environmentReady: Promise<void> | undefined;
    const startup = runDesktopStartup({
      inheritLoginShellEnv: async () => {
        calls.push("env:start");
        await environment.promise;
        calls.push("env:done");
      },
      bootstrapGui: async (ready) => {
        calls.push("gui");
        environmentReady = ready;
        await ready;
        calls.push("gui:after-env");
      },
    });

    await Promise.resolve();
    expect(calls).toEqual(["env:start", "gui"]);
    environment.resolve();
    await startup;
    await expect(environmentReady).resolves.toBeUndefined();
    expect(calls).toEqual(["env:start", "gui", "env:done", "gui:after-env"]);
  });

  it("releases waiting work when the login environment lookup fails", async () => {
    let environmentReady: Promise<void> | undefined;
    await runDesktopStartup({
      inheritLoginShellEnv: () => Promise.reject(new Error("shell failed")),
      bootstrapGui: async (ready) => {
        environmentReady = ready;
      },
    });
    await expect(environmentReady).resolves.toBeUndefined();
  });

  it("holds gated IPC requests until the environment is ready", async () => {
    const handlers = new Map<string, (event: unknown, ...args: unknown[]) => unknown>();
    const registry: IpcHandleRegistry = {
      handle: (channel, listener) => handlers.set(channel, listener),
    };
    const environment = deferred();
    const seen: unknown[] = [];
    afterEnvironment(registry, environment.promise).handle("spawn", (_event, value) => {
      seen.push(value);
      return "spawned";
    });

    const reply = handlers.get("spawn")?.({}, "editor");
    await Promise.resolve();
    expect(seen).toEqual([]);
    environment.resolve();
    await expect(reply).resolves.toBe("spawned");
    expect(seen).toEqual(["editor"]);
  });
});
