import { EventEmitter } from "node:events";
import { describe, expect, it, vi } from "vitest";
import type { HubAccountEvent, HubLink } from "@ait/client/internal/hub";
import { AccountHubBridge } from "./account-hub.js";
import type { AccountTransportOwner } from "./account-transport.js";

const SUB = "11111111-1111-4111-8111-111111111111";
function fixture() {
  let event = (_event: HubAccountEvent) => {};
  const call = vi.fn(async () => ({}));
  const reportPresence = vi.fn(async () => {});
  const releaseShares = vi.fn();
  const subscribeShares = vi.fn(() => releaseShares);
  const hub = {
    call,
    reportPresence,
    subscribeShares,
    subscribe: vi.fn(),
    onEvent: (handler: typeof event) => {
      event = handler;
    },
  };
  const bridge = new AccountHubBridge(hub as unknown as HubLink);
  const owner = () =>
    Object.assign(new EventEmitter(), {
      isDestroyed: () => false,
      send: vi.fn(),
    }) satisfies AccountTransportOwner;
  return {
    bridge,
    call,
    reportPresence,
    owner,
    releaseShares,
    subscribeShares,
    event: (value: HubAccountEvent) => event(value),
  };
}
describe("desktop account Hub boundary", () => {
  it("rejects publication and window attempts to release each other's interests", async () => {
    const h = fixture();
    const owner = h.owner();
    const other = h.owner();
    await expect(h.bridge.call(owner, "hub.events.publish", {})).resolves.toMatchObject({
      error: { code: "unsupported" },
    });
    expect(h.call).not.toHaveBeenCalled();
    await h.bridge.call(owner, "session.events.set_subscription.request", { subscriptionId: SUB });
    await expect(
      h.bridge.call(other, "subscription.release.request", { subscriptionId: SUB }),
    ).resolves.toMatchObject({ error: { code: "forbidden" } });
  });
  it("routes events only to the owning window and releases interests on navigation", async () => {
    const h = fixture();
    const owner = h.owner();
    await h.bridge.call(owner, "session.events.set_subscription.request", { subscriptionId: SUB });
    const event = {
      subscriptionId: SUB,
      cursor: 1,
      hostId: "host",
      eventId: "event",
      method: "agent.update",
      params: {},
    };
    h.event(event);
    expect(owner.send).toHaveBeenCalledExactlyOnceWith("paseo:event:account-hub-event", event);
    owner.emit("did-start-navigation", {}, "about:blank", false, true);
    expect(h.call).toHaveBeenLastCalledWith("subscription.release.request", {
      subscriptionId: SUB,
    });
    h.event({ ...event, cursor: 2 });
    expect(owner.send).toHaveBeenCalledOnce();
  });
  it("keeps a hidden window from hiding a visible window on the shared access link", async () => {
    const h = fixture();
    const visible = h.owner();
    const hidden = h.owner();
    await h.bridge.reportPresence(visible, { appVisible: true, focusedHostId: "host" });
    await h.bridge.reportPresence(hidden, { appVisible: false });
    expect(h.reportPresence).toHaveBeenLastCalledWith({
      appVisible: true,
      deviceType: "web",
      focusedHostId: "host",
      focusedAgentId: null,
      focusedTerminalId: null,
    });
    visible.emit("destroyed");
    expect(h.reportPresence).toHaveBeenLastCalledWith({ appVisible: false, deviceType: "web" });
    // Presence is not a center call a renderer may make.
    await expect(
      h.bridge.call(hidden, "session.heartbeat", { appVisible: true }),
    ).resolves.toMatchObject({ error: { code: "unsupported" } });
    await expect(h.bridge.reportPresence(hidden, { appVisible: "yes" })).rejects.toMatchObject({
      code: "invalid_params",
    });
    expect(h.call).not.toHaveBeenCalled();
  });
  it("merges windows into the link's state frames", async () => {
    vi.useFakeTimers();
    try {
      const h = fixture();
      const first = h.owner();
      const second = h.owner();
      await h.bridge.reportPresence(first, {
        appVisible: true,
        deviceType: "web",
        focusedHostId: "host-a",
        focusedAgentId: "agent-a",
      });
      vi.advanceTimersByTime(1000);
      await h.bridge.reportPresence(second, {
        appVisible: true,
        deviceType: "web",
        focusedHostId: "host-b",
      });
      expect(h.reportPresence).toHaveBeenLastCalledWith({
        appVisible: true,
        deviceType: "web",
        focusedHostId: "host-b",
        focusedAgentId: null,
        focusedTerminalId: null,
      });
      // A repeated heartbeat does not move focus; refocusing the first window does.
      vi.advanceTimersByTime(1000);
      await h.bridge.reportPresence(first, {
        appVisible: true,
        deviceType: "web",
        focusedHostId: "host-a",
        focusedAgentId: "agent-a",
      });
      expect(h.reportPresence).toHaveBeenLastCalledWith(
        expect.objectContaining({ focusedHostId: "host-b" }),
      );
      await h.bridge.reportPresence(first, { appVisible: false, deviceType: "web" });
      vi.advanceTimersByTime(1000);
      await h.bridge.reportPresence(first, {
        appVisible: true,
        deviceType: "web",
        focusedHostId: "host-a",
        focusedAgentId: "agent-a",
      });
      expect(h.reportPresence).toHaveBeenLastCalledWith(
        expect.objectContaining({ focusedHostId: "host-a", focusedAgentId: "agent-a" }),
      );
      // Any visible window keeps the account visible; none hides it.
      await h.bridge.reportPresence(first, { appVisible: false, deviceType: "web" });
      expect(h.reportPresence).toHaveBeenLastCalledWith(
        expect.objectContaining({ appVisible: true, focusedHostId: "host-b" }),
      );
      second.emit("destroyed");
      expect(h.reportPresence).toHaveBeenLastCalledWith({ appVisible: false, deviceType: "web" });
      expect(h.call).not.toHaveBeenCalled();
    } finally {
      vi.useRealTimers();
    }
  });
  it("owns share presence subscriptions by renderer lifetime", () => {
    const h = fixture();
    const owner = h.owner();
    h.bridge.setShareSubscription(owner, true);
    h.bridge.setShareSubscription(owner, true);
    expect(h.subscribeShares).toHaveBeenCalledOnce();
    owner.emit("did-start-navigation", {}, "about:blank", false, true);
    expect(h.releaseShares).toHaveBeenCalledOnce();
  });
});
