import { describe, expect, it, vi } from "vitest";
import type {
  HubAccountConnection,
  HubAccountEvent,
  HubDirectoryItem,
} from "@ait/client/internal/hub";
import { emptyHubDirectory, hubAttentionNotification, loadHubDirectory } from "./hub-services";

const item = (hostId: string): HubDirectoryItem => ({
  hostId,
  id: "same-id",
  data: { title: "Agent", workspaceId: "workspace", requiresAttention: true },
  revision: 1,
  updatedAt: "now",
  online: false,
  stale: true,
});
const attention: HubAccountEvent = {
  subscriptionId: "sub",
  cursor: 10,
  hostId: "host-b",
  eventId: "event",
  method: "agent_attention_required",
  params: { agentId: "same-id", reason: "finished", timestamp: "2026-10-22T00:00:00Z" },
};
describe("account directory and attention routing", () => {
  it("loads all pages and preserves Host-scoped identities and offline summaries", async () => {
    const call = vi.fn(async (method: string, params: { after: unknown }) => {
      if (method !== "agent.list.request") return { items: [], nextCursor: null };
      return params.after
        ? { items: [item("host-b")], nextCursor: null }
        : { items: [item("host-a")], nextCursor: { hostId: "host-a", id: "same-id" } };
    });
    const value = await loadHubDirectory({ call } as unknown as HubAccountConnection);
    expect(value.agent.map((row) => row.hostId)).toEqual(["host-a", "host-b"]);
    expect(value.agent.every((row) => row.stale && !row.online)).toBe(true);
    expect(call).toHaveBeenCalledWith("agent.list.request", {
      after: { hostId: "host-a", id: "same-id" },
      limit: 100,
    });
  });
  it("uses authoritative Host routing even when two Hosts share a resource ID", () => {
    const directory = emptyHubDirectory();
    directory.agent = [
      item("host-a"),
      { ...item("host-b"), data: { title: "B", workspaceId: "workspace-b" } },
    ];
    const notification = hubAttentionNotification(attention, "server-b", directory);
    expect(notification?.data).toMatchObject({
      serverId: "server-b",
      hostId: "host-b",
      agentId: "same-id",
      workspaceId: "workspace-b",
      eventId: "event",
    });
    expect(notification?.body).toBe("B");
  });
  it("drops cleared or superseded attention and ignores timeline markers for notifications", () => {
    const directory = emptyHubDirectory();
    directory.agent = [
      { ...item("host-b"), data: { requiresAttention: false, updatedAt: "2026-10-22T01:00:00Z" } },
    ];
    expect(hubAttentionNotification(attention, "server-b", directory)).toBeNull();
    directory.agent = [{ ...item("host-b"), data: { attentionTimestamp: "2026-10-22T01:00:00Z" } }];
    expect(hubAttentionNotification(attention, "server-b", directory)).toBeNull();
    expect(
      hubAttentionNotification(
        { ...attention, method: "agent.timeline.update" },
        "server-b",
        directory,
      ),
    ).toBeNull();
  });
  it("accepts attention that arrives before its newer directory summary", () => {
    const directory = emptyHubDirectory();
    directory.agent = [
      { ...item("host-b"), data: { requiresAttention: false, updatedAt: "2026-10-21T23:00:00Z" } },
    ];
    expect(hubAttentionNotification(attention, "server-b", directory)).not.toBeNull();
  });
});
