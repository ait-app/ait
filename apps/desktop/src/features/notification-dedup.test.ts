import { describe, expect, it } from "vitest";
import { claimDesktopNotification } from "./notification-dedup.js";
describe("desktop notification deduplication", () => {
  it("shares semantic attention ownership across renderers and transports", () => {
    expect(
      claimDesktopNotification({ attentionKey: "agent-a-at-time", eventId: "hub-event" }),
    ).toBe(true);
    expect(claimDesktopNotification({ attentionKey: "agent-a-at-time" })).toBe(false);
    expect(claimDesktopNotification({ attentionKey: "agent-b-at-time" })).toBe(true);
  });
  it("scopes replay identity by Host and leaves test notifications repeatable", () => {
    expect(claimDesktopNotification({ hostId: "host-a", eventId: "event" })).toBe(true);
    expect(claimDesktopNotification({ hostId: "host-a", eventId: "event" })).toBe(false);
    expect(claimDesktopNotification({ hostId: "host-b", eventId: "event" })).toBe(true);
    expect(claimDesktopNotification(undefined)).toBe(true);
    expect(claimDesktopNotification(undefined)).toBe(true);
  });
});
