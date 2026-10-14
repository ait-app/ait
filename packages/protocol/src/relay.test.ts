import { describe, expect, it } from "vitest";
import { RelayStatusSchema } from "./relay";

const status = {
  serverId: "stable",
  instanceId: "instance",
  platform: "linux",
  status: { online: true, connecting: false, epoch: null, error: null },
};

describe("Relay ownership", () => {
  it("accepts old daemons and preserves the managed ownership declaration", () => {
    expect(RelayStatusSchema.parse(status).management).toBeUndefined();
    expect(
      RelayStatusSchema.parse({
        ...status,
        management: { mode: "managed", phase: "running", binding: null },
      }).management,
    ).toEqual({ mode: "managed", phase: "running", binding: null });
    expect(
      RelayStatusSchema.safeParse({ ...status, management: { mode: "unknown" } }).success,
    ).toBe(false);
  });
});
