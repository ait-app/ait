import { expect, test, vi } from "vitest";
import {
  PermissionResponses,
  PermissionOutcomeUnknownError,
  PERMISSION_RESPONSE_TIMEOUT_MS,
} from "./permission-response.js";

const resolution = {
  agentId: "agent",
  requestId: "request",
  resolution: { behavior: "allow" as const },
};
function fixture() {
  const send = vi.fn(async () => resolution);
  const check = vi.fn(async (): Promise<typeof resolution | null> => null);
  const responses = new PermissionResponses({
    send,
    check,
    definitive: (error) => error === "refused",
  });
  const submit = () =>
    responses.submit("agent", "request", { behavior: "allow" }, PERMISSION_RESPONSE_TIMEOUT_MS);
  return { responses, send, check, submit };
}

test("duplicate approvals share the submitted operation and never execute twice", async () => {
  let finish!: (result: typeof resolution) => void;
  const f = fixture();
  f.send.mockImplementation(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  const first = f.submit();
  expect(f.submit()).toBe(first);
  finish(resolution);
  await first;
  await expect(f.submit()).resolves.toEqual(resolution);
  expect(f.send).toHaveBeenCalledOnce();
});

test("unknown results survive reconnect, missing permissions, and repeated clicks", async () => {
  const f = fixture();
  f.send.mockRejectedValue(new Error("response lost"));
  f.responses.remember("agent", { id: "request", provider: "test", name: "approve", kind: "tool" });
  await expect(f.submit()).rejects.toBeInstanceOf(PermissionOutcomeUnknownError);
  await f.responses.reconnect();
  await expect(f.submit()).rejects.toBeInstanceOf(PermissionOutcomeUnknownError);
  expect(f.responses.unconfirmed("agent")).toHaveLength(1);
  expect(f.responses.snapshot("agent", "request")?.phase).toBe("uncertain");
  expect(f.send).toHaveBeenCalledOnce();
  f.check.mockResolvedValue(resolution);
  await f.responses.check("agent", "request");
  expect(f.responses.snapshot("agent", "request")?.phase).toBe("confirmed");
  expect(f.responses.unconfirmed("agent")).toEqual([]);
});

test("only a matching resolution event confirms an unknown result", async () => {
  const f = fixture();
  f.send.mockRejectedValue(new Error("timeout"));
  await expect(f.submit()).rejects.toBeInstanceOf(PermissionOutcomeUnknownError);
  f.responses.confirm({ ...resolution, agentId: "another" });
  expect(f.responses.snapshot("agent", "request")?.phase).toBe("uncertain");
  f.responses.confirm(resolution);
  await expect(f.submit()).resolves.toEqual(resolution);
  expect(f.send).toHaveBeenCalledOnce();
});

test("explicit rejection allows a user retry without automatic resubmission", async () => {
  const f = fixture();
  f.send.mockRejectedValueOnce("refused");
  await expect(f.submit()).rejects.toBe("refused");
  expect(f.responses.snapshot("agent", "request")?.phase).toBe("rejected");
  expect(f.send).toHaveBeenCalledOnce();
  await f.submit();
  expect(f.send).toHaveBeenCalledTimes(2);
});
