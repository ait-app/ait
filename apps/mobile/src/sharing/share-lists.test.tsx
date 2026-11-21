// @vitest-environment jsdom
import type { ReactNode } from "react";
import { QueryClientProvider } from "@tanstack/react-query";
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { DaemonClient } from "@ait/client/internal/daemon-client";
import type { HubAccountSnapshot, HubShare } from "@ait/client/internal/hub";
import type { ShareSetting } from "@ait/protocol/messages";
import { queryClient } from "@/data/query-client";
import { useHubServices } from "@/runtime/hub-service-store";
import { refreshOutgoingShares, selectIncomingShares, useHasAnyShares } from "./share-lists";

type ShareListing = Awaited<ReturnType<DaemonClient["listShares"]>>;

const state = vi.hoisted(() => ({
  hosts: [
    { serverId: "owned", label: "Workstation", connections: [{ type: "directTcp" }] },
    {
      serverId: "shared:grant",
      label: "Someone else's Host",
      connections: [{ type: "accountRelay", shareId: "grant" }],
    },
  ],
  clients: new Map<string, Pick<DaemonClient, "listShares">>(),
  offline: new Set<string>(),
}));

vi.mock("@/runtime/host-runtime", () => ({
  useHosts: () => state.hosts,
  isSharedResourceHost: (host: { connections: { shareId?: string }[] }) =>
    host.connections.some((connection) => connection.shareId !== undefined),
  useHostRuntimeConnectionStatuses: (serverIds: readonly string[]) =>
    new Map(
      serverIds.map((serverId) => [serverId, state.offline.has(serverId) ? "offline" : "online"]),
    ),
  getHostRuntimeStore: () => ({ getClient: (serverId: string) => state.clients.get(serverId) }),
}));

function setting(shareState: ShareSetting["state"] = "accepted"): ShareSetting {
  return {
    resourceKind: "project",
    resourceKey: "project-1",
    groupId: "group-1",
    permission: "operate",
    audience: "group",
    recipients: [],
    settingId: "setting-1",
    state: shareState,
    missingRecipients: [],
  };
}

function listing(shares: ShareSetting[]): ShareListing {
  return {
    requestId: "list-shares",
    groups: [{ groupId: "group-1", name: "Team", status: "active", myRole: "member" }],
    shares,
  };
}

function ownedHostLists(shares: ShareSetting[]) {
  const listShares = vi.fn<DaemonClient["listShares"]>().mockResolvedValue(listing(shares));
  state.clients.set("owned", { listShares });
  return listShares;
}

function incoming(shares: HubShare[]) {
  useHubServices.setState({
    snapshot: {
      state: "online",
      linkId: "link",
      centerUrl: "https://center.test/api",
      methods: [],
      shares,
    },
  });
}

function wrapper({ children }: { children: ReactNode }) {
  return <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>;
}

beforeEach(() => {
  queryClient.clear();
  useHubServices.setState({ snapshot: null });
  state.clients.clear();
  state.offline.clear();
});
afterEach(cleanup);

describe("selectIncomingShares", () => {
  it("keeps an empty fallback reference stable for Zustand selectors", () => {
    expect(selectIncomingShares(null)).toBe(selectIncomingShares(null));
    expect(selectIncomingShares({ shares: null })).toBe(selectIncomingShares({}));
  });

  it("returns the Hub share snapshot unchanged", () => {
    const shares = [{ shareId: "share-1" }] as HubShare[];
    const snapshot = { shares } as Pick<HubAccountSnapshot, "shares">;
    expect(selectIncomingShares(snapshot)).toBe(shares);
  });
});

describe("useHasAnyShares", () => {
  it("is false when nothing is shared with the user or by the user", async () => {
    const listShares = ownedHostLists([]);
    const { result } = renderHook(useHasAnyShares, { wrapper });

    await waitFor(() => expect(listShares).toHaveBeenCalledOnce());
    expect(result.current).toBe(false);
    // A Host reached through someone else's share cannot list shares of its own.
    expect(state.clients.has("shared:grant")).toBe(false);
  });

  it("is true when only the shared-with-me list has an entry", () => {
    ownedHostLists([]);
    incoming([{ shareId: "grant", hostId: "host" } as HubShare]);
    const { result } = renderHook(useHasAnyShares, { wrapper });

    expect(result.current).toBe(true);
  });

  it("is true when only the shared-by-me list has an entry", async () => {
    ownedHostLists([setting()]);
    const { result } = renderHook(useHasAnyShares, { wrapper });

    expect(result.current).toBe(false);
    await waitFor(() => expect(result.current).toBe(true));
  });

  it("does not count revoked shares, daemons without sharing, or failed requests", async () => {
    const listShares = ownedHostLists([setting("revoked")]);
    const { result, rerender } = renderHook(useHasAnyShares, { wrapper });
    await waitFor(() => expect(listShares).toHaveBeenCalledOnce());
    expect(result.current).toBe(false);

    for (const error of [
      Object.assign(new Error("not implemented"), { code: "not_implemented" }),
      new Error("Timed out"),
    ]) {
      listShares.mockClear().mockRejectedValue(error);
      act(() => refreshOutgoingShares("owned"));
      await waitFor(() => expect(listShares).toHaveBeenCalledOnce());
      rerender();
      expect(result.current).toBe(false);
    }
  });

  it("follows the Host connection and later changes to the Host's shares", async () => {
    const listShares = ownedHostLists([setting()]);
    const { result, rerender } = renderHook(useHasAnyShares, { wrapper });
    await waitFor(() => expect(result.current).toBe(true));

    state.offline.add("owned");
    rerender();
    expect(result.current).toBe(false);

    state.offline.clear();
    rerender();
    await waitFor(() => expect(result.current).toBe(true));

    listShares.mockResolvedValue(listing([]));
    act(() => refreshOutgoingShares("owned"));
    await waitFor(() => expect(result.current).toBe(false));
  });
});
