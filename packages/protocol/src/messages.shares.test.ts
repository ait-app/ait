import { describe, expect, it } from "vitest";
import { SessionInboundMessageSchema, SessionOutboundMessageSchema } from "./messages.js";

const groupId = "11111111-1111-4111-8111-111111111111";
const settingId = "22222222-2222-4222-8222-222222222222";
const shareId = "33333333-3333-4333-8333-333333333333";

describe("daemon share RPC messages", () => {
  it("accepts workspace, project, and node share mutations", () => {
    for (const [resourceKind, resourceKey] of [
      ["workspace", "workspace-1"],
      ["project", "project-1"],
      ["node", ""],
    ] as const) {
      expect(
        SessionInboundMessageSchema.parse({
          type: "share.set.request",
          requestId: `set-${resourceKind}`,
          resourceKind,
          resourceKey,
          groupId,
          permission: "operate",
          audience: "group",
          recipients: [],
          displayName: "Checkout",
        }),
      ).toMatchObject({ resourceKind, resourceKey, groupId });
      expect(
        SessionInboundMessageSchema.parse({
          type: "share.remove.request",
          requestId: `remove-${resourceKind}`,
          resourceKind,
          resourceKey,
          groupId,
        }),
      ).toMatchObject({ resourceKind, resourceKey, groupId });
    }
  });

  it("decodes persisted share state and group visibility from the daemon", () => {
    const message = SessionOutboundMessageSchema.parse({
      type: "share.list.response",
      payload: {
        requestId: "list-1",
        groups: [{ groupId, name: "Design", status: "active", myRole: "member" }],
        shares: [
          {
            resourceKind: "project",
            resourceKey: "project-1",
            groupId,
            permission: "operate",
            audience: "group",
            recipients: [],
            displayName: "Storefront",
            projectName: "Storefront",
            settingId,
            shareId,
            state: "accepted",
            missingRecipients: [],
          },
        ],
      },
    });
    expect(message.type).toBe("share.list.response");
    if (message.type !== "share.list.response") throw new Error("unexpected response");
    expect(message.payload.shares[0]).toMatchObject({ settingId, shareId, state: "accepted" });
    expect(message.payload.groups[0]).toMatchObject({ groupId, name: "Design" });
  });

  it("rejects malformed identifiers at the desktop boundary", () => {
    expect(
      SessionInboundMessageSchema.safeParse({
        type: "share.set.request",
        requestId: "set-invalid",
        resourceKind: "workspace",
        resourceKey: "workspace-1",
        groupId: "not-a-group-id",
        permission: "operate",
        audience: "group",
        recipients: [],
      }).success,
    ).toBe(false);
  });
});
