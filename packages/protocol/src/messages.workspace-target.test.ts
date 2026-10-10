import { describe, expect, test } from "vitest";
import { SessionInboundMessageSchema, SessionOutboundMessageSchema } from "./messages.js";
import { validateWSOutboundMessage } from "./validation/ws-outbound.js";

describe("Workspace target protocol", () => {
  test("validates the exact target identity and its bounds", () => {
    const request = {
      type: "workspace.get.request",
      workspaceId: "workspace",
      requestId: "target",
    };
    expect(SessionInboundMessageSchema.parse(request)).toEqual(request);
    for (const workspaceId of ["", "x".repeat(257)]) {
      expect(SessionInboundMessageSchema.safeParse({ ...request, workspaceId }).success).toBe(
        false,
      );
    }
  });

  test("accepts availability states through both Zod and the transport's compiled validator", () => {
    for (const state of ["missing", "archived"]) {
      const message = {
        type: "workspace.get.response",
        payload: {
          requestId: "target",
          workspaceId: "workspace",
          state,
          workspace: null,
          project: null,
        },
      };
      expect(SessionOutboundMessageSchema.parse(message)).toEqual(message);
      expect(validateWSOutboundMessage({ type: "session", message }).success).toBe(true);
    }
    expect(
      SessionOutboundMessageSchema.safeParse({
        type: "workspace.get.response",
        payload: {
          requestId: "target",
          workspaceId: "workspace",
          state: "unknown",
          workspace: null,
          project: null,
        },
      }).success,
    ).toBe(false);
  });
});
