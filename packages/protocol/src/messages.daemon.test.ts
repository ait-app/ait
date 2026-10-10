import { describe, expect, test } from "vitest";
import { SessionInboundMessageSchema, SessionOutboundMessageSchema } from "./messages.js";
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
