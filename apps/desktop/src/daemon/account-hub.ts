import { HubCallError, HUB_ACCESS_METHODS, type HubLink } from "@ait/client/internal/hub";
import type { AccountTransportOwner } from "./account-transport.js";

const SUBSCRIPTIONS = new Set([
  "session.events.set_subscription.request",
  "server.status.subscribe",
  "creation.subscribe.request",
  "agent.timeline.set_subscription.request",
]);

/** Keep each renderer's account interests owned by its window, including crash/navigation cleanup. */
export class AccountHubBridge {
  private readonly subscriptions = new Map<string, AccountTransportOwner>();
  private readonly owners = new Map<AccountTransportOwner, () => void>();
  private link: string | null = null;
  private readonly presence = new Map<
    AccountTransportOwner,
    { at: number; params: Record<string, unknown> }
  >();

  constructor(private readonly hub: HubLink) {
    hub.subscribe(() => {
      const link = hub.snapshot().linkId;
      if (link !== this.link) {
        this.link = link;
        this.subscriptions.clear();
        this.presence.clear();
      }
    });
    hub.onEvent((event) => {
      const owner = this.subscriptions.get(event.subscriptionId);
      if (owner && !owner.isDestroyed()) {
        const focused = Array.from(this.presence.values()).some(
          ({ at, params }) =>
            Date.now() - at < 60_000 &&
            params.appVisible === true &&
            params.focusedHostId === event.hostId &&
            event.resourceId &&
            (params.focusedAgentId === event.resourceId ||
              params.focusedTerminalId === event.resourceId),
        );
        owner.send("paseo:event:account-hub-event", {
          ...event,
          ...(focused ? { notificationSuppressed: true } : {}),
        });
      }
    });
  }

  async call(owner: AccountTransportOwner, method: unknown, params: unknown): Promise<unknown> {
    try {
      if (!HUB_ACCESS_METHODS.some((value) => value === method))
        throw new HubCallError("unsupported", "Unavailable account service");
      const name = method as string;
      if (name === "session.heartbeat") {
        if (
          !params ||
          typeof params !== "object" ||
          !("appVisible" in params) ||
          typeof params.appVisible !== "boolean"
        )
          throw new HubCallError("invalid_params", "Invalid account presence");
        this.watch(owner);
        this.presence.set(owner, { at: Date.now(), params: params as Record<string, unknown> });
        return { result: await this.hub.call(name, this.heartbeat()) };
      }
      const subscriptionId =
        params && typeof params === "object" && "subscriptionId" in params
          ? params.subscriptionId
          : undefined;
      if (SUBSCRIPTIONS.has(name) || name === "subscription.release.request") {
        if (typeof subscriptionId !== "string" || !/^[0-9a-f-]{36}$/i.test(subscriptionId))
          throw new HubCallError("invalid_params", "Invalid subscription identity");
        const existing = this.subscriptions.get(subscriptionId);
        if (existing && existing !== owner)
          throw new HubCallError("forbidden", "Subscription belongs to another window");
        if (SUBSCRIPTIONS.has(name)) {
          if (!existing && this.subscriptions.size >= 64)
            throw new HubCallError("capacity", "Too many account subscriptions");
          this.watch(owner);
          this.subscriptions.set(subscriptionId, owner);
        }
      }
      const result = await this.hub.call(name, params ?? {});
      if (name === "subscription.release.request" && typeof subscriptionId === "string")
        this.subscriptions.delete(subscriptionId);
      return { result };
    } catch (error) {
      return {
        error: {
          code: error instanceof HubCallError ? error.code : "call_failed",
          message: error instanceof Error ? error.message : "Account service failed",
        },
      };
    }
  }

  private watch(owner: AccountTransportOwner): void {
    if (this.owners.has(owner)) return;
    const navigated = (
      _event: unknown,
      _url: string,
      sameDocument: boolean,
      mainFrame: boolean,
    ) => {
      if (mainFrame && !sameDocument) close();
    };
    const close = () => {
      owner.removeListener("destroyed", close);
      owner.removeListener("did-start-navigation", navigated);
      this.owners.delete(owner);
      if (this.presence.delete(owner))
        void this.hub.call("session.heartbeat", this.heartbeat()).catch(() => {});
      for (const [subscriptionId, current] of this.subscriptions) {
        if (current !== owner) continue;
        this.subscriptions.delete(subscriptionId);
        void this.hub.call("subscription.release.request", { subscriptionId }).catch(() => {});
      }
    };
    this.owners.set(owner, close);
    owner.once("destroyed", close);
    owner.on("did-start-navigation", navigated);
  }

  private heartbeat(): Record<string, unknown> {
    const fresh = Array.from(this.presence.values()).filter(
      (value) => Date.now() - value.at < 60_000,
    );
    const visible = fresh.filter((value) => value.params.appVisible === true);
    return (
      visible.sort((a, b) => b.at - a.at)[0]?.params ?? { appVisible: false, deviceType: "web" }
    );
  }
}
