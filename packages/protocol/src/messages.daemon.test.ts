import { describe, expect, test } from "vitest";
import {
  MutableDaemonConfigPatchSchema,
  MutableDaemonConfigSchema,
  SessionInboundMessageSchema,
  SessionOutboundMessageSchema,
} from "./messages.js";
import { validateWSOutboundMessage } from "./validation/ws-outbound.js";

describe("Daemon pairing offer removal", () => {
  test("rejects the retired request while retaining daemon status queries", () => {
    const request = { type: "daemon.get_pairing_offer.request", requestId: "retired" };
    expect(SessionInboundMessageSchema.safeParse(request).success).toBe(false);
    expect(
      SessionInboundMessageSchema.safeParse({ ...request, type: "daemon.get_status.request" })
        .success,
    ).toBe(true);
  });

  test("rejects the retired response in Zod and the compiled transport validator", () => {
    const message = {
      type: "daemon.get_pairing_offer.response",
      payload: { requestId: "retired", url: "", qr: null, relayEnabled: false },
    };
    expect(SessionOutboundMessageSchema.safeParse(message).success).toBe(false);
    expect(validateWSOutboundMessage({ type: "session", message }).success).toBe(false);
  });
});

describe("Daemon config mcp.acceptPeers", () => {
  test("accepts a daemon config with and without mcp.acceptPeers", () => {
    const without = MutableDaemonConfigSchema.parse({ mcp: { injectIntoAgents: true } });
    expect(without.mcp.acceptPeers).toBeUndefined();

    const withValue = MutableDaemonConfigSchema.parse({
      mcp: { injectIntoAgents: true, acceptPeers: true },
    });
    expect(withValue.mcp.acceptPeers).toBe(true);
  });

  test("accepts a patch that sets mcp.acceptPeers alone", () => {
    expect(MutableDaemonConfigPatchSchema.parse({ mcp: { acceptPeers: true } }).mcp).toEqual({
      acceptPeers: true,
    });
    expect(MutableDaemonConfigPatchSchema.parse({ mcp: { acceptPeers: false } }).mcp).toEqual({
      acceptPeers: false,
    });
  });

  test("rejects a non-boolean mcp.acceptPeers", () => {
    expect(
      MutableDaemonConfigSchema.safeParse({
        mcp: { injectIntoAgents: true, acceptPeers: "yes" },
      }).success,
    ).toBe(false);
    expect(MutableDaemonConfigPatchSchema.safeParse({ mcp: { acceptPeers: "yes" } }).success).toBe(
      false,
    );
  });
});
