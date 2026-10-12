import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { DaemonClient } from "@ait/client/internal/daemon-client";
import { respondToQuestionWithTimeout } from "./question-response";

type Resolution = Awaited<ReturnType<DaemonClient["respondToPermissionAndWait"]>>;
const input = {
  agentId: "agent-a",
  requestId: "question-a",
  response: { behavior: "deny" as const },
};
const resolution: Resolution = {
  agentId: input.agentId,
  requestId: input.requestId,
  resolution: input.response,
};

function deferred() {
  let resolve!: (value: Resolution) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<Resolution>((ok, fail) => {
    resolve = ok;
    reject = fail;
  });
  return { promise, resolve, reject };
}

describe("question response deadline", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("times out a stalled send at five seconds and requests the same RPC deadline", async () => {
    const pending = deferred();
    const client = { respondToPermissionAndWait: vi.fn(() => pending.promise) };
    const result = respondToQuestionWithTimeout(client, input);
    const rejected = vi.fn();
    void result.catch(rejected);
    const assertion = expect(result).rejects.toThrow("Timed out waiting for question response");
    await vi.advanceTimersByTimeAsync(4_999);
    expect(rejected).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(1);
    await assertion;
    expect(client.respondToPermissionAndWait).toHaveBeenCalledWith(
      "agent-a",
      "question-a",
      input.response,
      5_000,
    );
    expect(vi.getTimerCount()).toBe(0);
  });

  it("clears the deadline after a successful response", async () => {
    const client = { respondToPermissionAndWait: vi.fn(async () => resolution) };
    await expect(respondToQuestionWithTimeout(client, input)).resolves.toEqual(resolution);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("preserves immediate failures without leaving a timer", async () => {
    const error = new Error("Connection closed");
    const client = {
      respondToPermissionAndWait: vi.fn(async () => {
        throw error;
      }),
    };
    await expect(respondToQuestionWithTimeout(client, input)).rejects.toBe(error);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("keeps a retry pending when a timed-out attempt later fails", async () => {
    const first = deferred();
    const second = deferred();
    const client = {
      respondToPermissionAndWait: vi
        .fn<DaemonClient["respondToPermissionAndWait"]>()
        .mockReturnValueOnce(first.promise)
        .mockReturnValueOnce(second.promise),
    };
    const original = respondToQuestionWithTimeout(client, input);
    const timeout = expect(original).rejects.toThrow(/Timed out/);
    await vi.advanceTimersByTimeAsync(5_000);
    await timeout;
    const retry = respondToQuestionWithTimeout(client, input);
    const retryFailed = vi.fn();
    void retry.catch(retryFailed);
    first.reject(new Error("Late transport failure"));
    await vi.advanceTimersByTimeAsync(0);
    expect(retryFailed).not.toHaveBeenCalled();
    expect(vi.getTimerCount()).toBe(1);
    second.resolve(resolution);
    await expect(retry).resolves.toEqual(resolution);
    expect(vi.getTimerCount()).toBe(0);
  });
});
