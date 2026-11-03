import { EventEmitter } from "node:events";
import { describe, expect, it, vi } from "vitest";
import type { HubAccountEvent, HubLink } from "@ait/client/internal/hub";
import { AccountHubBridge } from "./account-hub.js";
import type { AccountTransportOwner } from "./account-transport.js";

const SUB = "11111111-1111-4111-8111-111111111111";
function fixture() {
  let event = (_event: HubAccountEvent) => {};
  const call = vi.fn(async () => ({}));
  const releaseShares = vi.fn();
  const subscribeShares = vi.fn(() => releaseShares);
  const hub = {
    call,
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
    await h.bridge.call(visible, "session.heartbeat", { appVisible: true, focusedHostId: "host" });
    await h.bridge.call(hidden, "session.heartbeat", { appVisible: false });
    expect(h.call).toHaveBeenLastCalledWith("session.heartbeat", {
      appVisible: true,
      focusedHostId: "host",
    });
    visible.emit("destroyed");
    expect(h.call).toHaveBeenLastCalledWith("session.heartbeat", {
      appVisible: false,
      deviceType: "web",
    });
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
