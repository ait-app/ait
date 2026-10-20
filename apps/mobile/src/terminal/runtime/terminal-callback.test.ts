import { afterEach, expect, it, vi } from "vitest";
import { runTerminalCallback } from "./terminal-callback";

afterEach(() => vi.restoreAllMocks());

it("runs callbacks immediately and never retries input after a failure", async () => {
  const log = vi.spyOn(console, "warn").mockImplementation(() => {});
  const synchronous = new Error("Transport closed while handling a key");
  const asynchronous = new Error("Native input view unmounted");
  const send = vi.fn(() => {
    throw synchronous;
  });
  const focus = vi.fn(async () => {
    throw asynchronous;
  });
  expect(() => runTerminalCallback(send)).not.toThrow();
  runTerminalCallback(focus);
  await Promise.resolve();
  expect(send).toHaveBeenCalledOnce();
  expect(focus).toHaveBeenCalledOnce();
  expect(log.mock.calls).toEqual([
    ["Terminal callback failed", synchronous],
    ["Terminal callback failed", asynchronous],
  ]);
});

it("preserves successful synchronous and asynchronous callbacks", async () => {
  const log = vi.spyOn(console, "warn").mockImplementation(() => {});
  const values: string[] = [];
  runTerminalCallback(() => {
    values.push("input");
  });
  expect(values).toEqual(["input"]);
  runTerminalCallback(async () => {
    await Promise.resolve();
    values.push("focus");
  });
  await Promise.resolve();
  expect(values).toEqual(["input", "focus"]);
  expect(log).not.toHaveBeenCalled();
});
