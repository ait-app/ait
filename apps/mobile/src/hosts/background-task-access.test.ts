import { describe, expect, it } from "vitest";
import { createAccountRelayHostConnection } from "@/types/host-connection";
import { canManageBackgroundTasks } from "./background-task-access";

describe("background task ownership", () => {
  it("allows owner runtimes and rejects persisted guest runtime IDs", () => {
    expect(canManageBackgroundTasks("owner")).toBe(true);
    expect(canManageBackgroundTasks("shared:grant")).toBe(false);
    expect(canManageBackgroundTasks({ serverId: "shared:grant", label: "Shared" })).toBe(false);
  });

  it.each(["node", "project", "workspace"] as const)(
    "rejects a %s grant even with a legacy runtime ID",
    (shareResourceKind) => {
      const connection = createAccountRelayHostConnection({
        center: "https://center.test/api",
        hostId: "11111111-1111-4111-8111-111111111111",
        shareId: "22222222-2222-4222-8222-222222222222",
        nodeId: "node",
        shareResourceKind,
      });
      expect(
        canManageBackgroundTasks({
          serverId: "legacy",
          label: "Shared",
          connections: [connection],
        }),
      ).toBe(false);
    },
  );
});
