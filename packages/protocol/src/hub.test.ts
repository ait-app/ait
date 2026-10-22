import { describe, expect, it } from "vitest";
import { HubStatusSchema } from "./hub";

const status = {
  serverId: "stable",
  instanceId: "instance",
  platform: "linux",
  status: { online: true, connecting: false, link: null, error: null },
};

describe("Hub ownership", () => {
  it("accepts unmanaged daemons and preserves the managed ownership declaration", () => {
    expect(HubStatusSchema.parse(status).management).toBeUndefined();
    expect(
      HubStatusSchema.parse({
        ...status,
        management: { mode: "managed", phase: "running", binding: null },
      }).management,
    ).toEqual({ mode: "managed", phase: "running", binding: null });
    expect(HubStatusSchema.safeParse({ ...status, management: { mode: "unknown" } }).success).toBe(
      false,
    );
  });
});
