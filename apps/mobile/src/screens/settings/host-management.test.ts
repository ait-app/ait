import { expect, test } from "vitest";
import type { HubShare } from "@ait/client/internal/hub";
import type { HostProfile } from "@/types/host-connection";
import { canManageHost, daemonManagementServerId } from "./host-management";

const shared = { connections: [{ type: "accountRelay", shareId: "grant" }] } as HostProfile;
test("only whole-node shares expose host management; unknown scopes stay hidden", () => {
  expect(canManageHost(null, [])).toBe(false);
  expect(canManageHost({ connections: [{ type: "directTcp" }] } as HostProfile, [])).toBe(true);
  expect(canManageHost(shared, [])).toBe(false);
  for (const resourceKind of ["workspace", "project", "node"] as const) {
    expect(canManageHost(shared, [{ shareId: "grant", resourceKind } as HubShare])).toBe(
      resourceKind === "node",
    );
  }
  expect(canManageHost(shared, [{ shareId: "other", resourceKind: "node" } as HubShare])).toBe(
    false,
  );
});
test("shared restart checks the real daemon identity without relaxing owner checks", () => {
  expect(daemonManagementServerId("shared:grant", "physical")).toBe("physical");
  expect(daemonManagementServerId("physical", "different")).toBe("physical");
  expect(daemonManagementServerId("shared:grant", undefined)).toBe("");
});
