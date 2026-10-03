import { describe, expect, it } from "vitest";
import {
  AgentTimelineSearchResponseMessageSchema,
  parseServerInfoStatusPayload,
  WSOutboundMessageSchema,
} from "@ait/protocol/messages";
import { METHODS } from "./methods";
import { eventMessage, responseMessage, serverInfo } from "./messages";
import { object } from "./types";

it.each([
  ["agent.update", "agent_update", { agentId: "agent" }],
  ["workspace.update", "workspace_update", { id: "workspace", removedProjectId: "project" }],
] as const)(
  "adapts %s directory events without losing subscription or sync metadata",
  (method, type, identity) => {
    const payload = {
      kind: "remove",
      ...identity,
      subscriptionId: "directory-lease",
      generation: "generation",
      seq: 3,
    };
    const envelope = eventMessage(method, payload);
    expect(WSOutboundMessageSchema.parse(envelope)).toEqual({
      type: "session",
      message: { type, payload },
    });
  },
);

function info(methods: string[]) {
  const message = serverInfo({ server_id: "ait" }, new Set(methods));
  return parseServerInfoStatusPayload(object(object(message.message).payload));
}

describe("Rust daemon software version", () => {
  it.each(["0.0.10", "v0.0.11"])("preserves the connected server's version %s", (version) => {
    const envelope = serverInfo({ server_id: "ait", version }, new Set());
    const value = parseServerInfoStatusPayload(object(object(envelope.message).payload));
    expect(value?.version).toBe(version);
  });

  it.each([undefined, null, 11, { major: 1, minor: 0 }])(
    "keeps an unknown software version when the server sends %j",
    (version) => {
      const envelope = serverInfo({ server_id: "ait", version }, new Set());
      const value = parseServerInfoStatusPayload(object(object(envelope.message).payload));
      expect(value?.version).toBeNull();
    },
  );
});

it("preserves Rust timeline search counts in the SDK response envelope", () => {
  const result = {
    agentId: "agent",
    epoch: "epoch",
    locations: [{ seq: 1, role: "assistant", count: 3 }],
    nextCursor: null,
    error: null,
  };
  const envelope = responseMessage("agent.timeline.search.response", "search", result, {});
  expect(AgentTimelineSearchResponseMessageSchema.parse(envelope.message).payload).toEqual({
    ...result,
    requestId: "search",
  });
});

describe("Ait host capabilities", () => {
  it("enables classified commit history only when Rust advertises its list method", () => {
    expect(info([])?.features).toMatchObject({ commitsList: false, commitBaseClassification: false });
    expect(info(["checkout.commits.list.request"])?.features).toMatchObject({ commitsList: true, commitBaseClassification: true });
  });

  it("advertises checkout status events only with the Git producer and checkout method", () => {
    const read = (features: string[], methods: string[]) =>
      parseServerInfoStatusPayload(
        object(
          object(serverInfo({ server_id: "ait", features }, new Set(methods)).message).payload,
        ),
      )?.sessionEventTypes;
    const method = "checkout.status.get.request";
    expect(read(["checkout-git-events-v1"], [method])).toContain("checkout_status_update");
    expect(read([], [method])).not.toContain("checkout_status_update");
    expect(read(["checkout-git-events-v1"], [])).not.toContain("checkout_status_update");
  });

  it("enables composite creation only with the lifecycle producer and its methods", () => {
    const methods = new Set([
      "agent.create.request",
      "workspace.create.request",
      "creation.subscribe.request",
    ]);
    const read = (features: string[]) =>
      parseServerInfoStatusPayload(
        object(object(serverInfo({ server_id: "ait", features }, methods).message).payload),
      )?.features;
    expect(read(["creation-lifecycle-v1"])).toMatchObject({
      creationLifecycle: true,
      agentRequestReceipts: true,
      workspaceRequestReceipts: true,
    });
    expect(read([])?.creationLifecycle).toBe(false);
    methods.delete("creation.subscribe.request");
    expect(read(["creation-lifecycle-v1"])?.creationLifecycle).toBe(false);
  });
  it("advertises native approval and child events only with the producer feature", () => {
    const read = (features: string[], methods: string[]) =>
      parseServerInfoStatusPayload(
        object(
          object(serverInfo({ server_id: "ait", features }, new Set(methods)).message).payload,
        ),
      )?.sessionEventTypes;
    const method = "agent.permission.resolve.request";
    expect(read(["agent-session-events-v1"], [method])).toEqual(
      expect.arrayContaining([
        "agent_permission_request",
        "agent_permission_resolved",
        "agent.provider_subagents.update",
      ]),
    );
    expect(read([], [method])).not.toContain("agent_permission_request");
    expect(read(["agent-session-events-v1"], [])).not.toContain("agent_permission_resolved");
  });
  it("advertises terminal attention only for hosts with terminal activity events", () => {
    const read = (features: string[], methods: string[]) =>
      parseServerInfoStatusPayload(
        object(
          object(serverInfo({ server_id: "ait", features }, new Set(methods)).message).payload,
        ),
      )?.sessionEventTypes;
    expect(read(["terminal-activity-v1"], ["terminal.list.request"])).toContain(
      "terminal_attention_required",
    );
    expect(read([], ["terminal.list.request"])).not.toContain("terminal_attention_required");
    expect(read(["terminal-activity-v1"], [])).not.toContain("terminal_attention_required");
  });

  it("enables directory sync and streams only when the connected host advertises them", () => {
    const methods = new Set([
      "project.list.request",
      "workspace.list.request",
      "agent.list.request",
      "subscription.release.request",
    ]);
    const features = ["directory-sync-v1", "directory-subscriptions-v1"];
    const read = (advertised: string[]) =>
      parseServerInfoStatusPayload(
        object(
          object(serverInfo({ server_id: "ait", features: advertised }, methods).message).payload,
        ),
      )?.features;
    expect(read(features)).toMatchObject({ directorySync: true, directorySubscriptions: true });
    expect(read([])).toMatchObject({ directorySync: false, directorySubscriptions: false });
    methods.delete("agent.list.request");
    expect(read(features)).toMatchObject({ directorySync: false, directorySubscriptions: false });
  });

  it("enables GitLab panels only when the server advertises its installed adapter", () => {
    const methods = new Set([
      "checkout.pr.status.request",
      "checkout.forge.get_check_details.request",
      "checkout.forge.set_auto_merge.request",
    ]);
    const envelope = serverInfo({ server_id: "ait", features: ["forge-gitlab-v1"] }, methods);
    const value = parseServerInfoStatusPayload(object(object(envelope.message).payload));
    expect(value?.features).toMatchObject({
      forgeProviders: true,
      forgeCheckDetails: true,
      checkoutForgeSetAutoMerge: true,
    });
    expect(info([...methods])?.features?.forgeProviders).toBe(false);
    expect(info([])?.features?.forgeCheckDetails).toBe(false);
  });

  it("exposes working settings and directory features without unsupported transports", () => {
    const value = info(Object.values(METHODS).map((method) => method.method));
    expect(value?.features).toMatchObject({
      projectList: true,
      projectAdd: true,
      stableProjectIdentity: true,
      workspaceMultiplicity: true,
      projectCreateDirectory: true,
      projectGithubClone: true,
      workspaceGithubRepositorySearch: true,
      agentProfiles: true,
      providerRemoval: true,
      projectCustomIcon: true,
      importSessionWorkspaceTarget: true,
      importSessionSearch: true,
      directorySubscriptions: false,
      daemonPairing: false,
    });
    expect(value?.features?.plugins).not.toBe(true);
    expect(value?.sessionEventTypes).toEqual([
      "status.server_info",
      "status.daemon_config_changed",
      "providers_snapshot_update",
      "agent_attention_required",
    ]);
  });

  it("does not enable controls without all their required methods", () => {
    expect(info(["project.list.request", "daemon.config.get.request"])?.features).toMatchObject({
      projectList: true,
      projectAdd: false,
      stableProjectIdentity: false,
      agentProfiles: false,
      providerRemoval: false,
      projectCustomIcon: false,
      importSessionWorkspaceTarget: false,
      importSessionSearch: false,
    });
  });
});

it("adapts post-fetch checkout status events into the SDK schema", () => {
  const payload = {
    cwd: "/repo",
    isGit: true,
    repoRoot: "/repo",
    mainRepoRoot: "/repo",
    currentBranch: "feature",
    isDirty: false,
    baseRef: "main",
    aheadBehind: { ahead: 1, behind: 2 },
    upstreamRef: "origin/feature",
    aheadOfOrigin: 0,
    behindOfOrigin: 1,
    hasRemote: true,
    remoteUrl: "https://example.test/repo.git",
    isPaseoOwnedWorktree: false,
    error: null,
    subscriptionId: "checkout-events",
  };
  const envelope = eventMessage("checkout.status.update", payload);
  const parsed = WSOutboundMessageSchema.parse(envelope);
  expect(parsed).toMatchObject({
    type: "session",
    message: { type: "checkout_status_update", payload: { ...payload, requestId: "" } },
  });
});

describe("Rust provider snapshots", () => {
  const entries = [
    {
      provider: "codex",
      status: "ready",
      enabled: true,
      fetchedAt: "2026-09-27T00:00:00Z",
      models: [{ provider: "codex", id: "model-1", label: "Model 1", thinkingOptions: [] }],
    },
  ];
  it("supplies a cacheable compact body for a full hashed response", () => {
    const message = responseMessage(
      "get_providers_snapshot_response",
      "catalog",
      { entries, snapshotHash: "hash", notModified: false },
      {},
    );
    expect(object(message.message).payload).toMatchObject({
      requestId: "catalog",
      snapshotHash: "hash",
      compactSnapshot: { entries: [{ provider: "codex", models: [{ id: "model-1" }] }] },
      fetchedAt: { codex: "2026-09-27T00:00:00Z" },
    });
  });
  it("normalizes pushed catalogs as full snapshots too", () => {
    const message = eventMessage("providers_snapshot_update", {
      entries,
      snapshotHash: "hash",
      subscriptionId: "feed",
    });
    expect(object(message.message).payload).toMatchObject({
      subscriptionId: "feed",
      compactSnapshot: { entries: [{ provider: "codex" }] },
    });
  });
  it("does not replace a not-modified reply with an empty catalog", () => {
    const message = responseMessage(
      "get_providers_snapshot_response",
      "cached",
      { entries: [], snapshotHash: "hash", notModified: true },
      {},
    );
    expect(object(message.message).payload).not.toHaveProperty("compactSnapshot");
  });
  it("keeps a genuinely empty full catalog cacheable", () => {
    const message = responseMessage(
      "get_providers_snapshot_response",
      "empty",
      { entries: [], snapshotHash: "empty", notModified: false },
      {},
    );
    expect(object(message.message).payload).toMatchObject({
      compactSnapshot: { entries: [], thinkingSets: [] },
    });
  });
});
