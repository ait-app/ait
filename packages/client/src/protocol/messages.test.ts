import { describe, expect, it } from "vitest";
import {
  AgentTimelineSearchResponseMessageSchema,
  parseServerInfoStatusPayload,
  SessionOutboundMessageSchema,
} from "@ait/protocol/messages";
import { METHODS } from "./methods.js";
import { eventMessage, responseMessage, serverInfo } from "./messages.js";
import { object } from "./types.js";
import { validateWSOutboundMessage } from "@ait/protocol/validation/ws-outbound";
it("retains full directory capabilities for whole-node shares", () => {
  const info = {
    server_id: "shared-node",
    features: ["directory-sync-v1", "directory-subscriptions-v1"],
  };
  const methods = new Set(Object.values(METHODS).map((spec) => spec.method));
  expect(serverInfo(info, methods, "node")).toEqual(serverInfo(info, methods));
});
it.each(["workspace", "project"] as const)(
  "uses scoped snapshots for a %s guest even when the host advertises full directories",
  (scope) => {
    const methods = new Set([
      "project.list.request",
      "workspace.list.request",
      "agent.list.request",
      "subscription.release.request",
      "session.events.set_subscription.request",
      "workspace.label.list.request",
      "daemon.host_stats.request",
    ]);
    const info = serverInfo(
      {
        server_id: "shared-host",
        features: ["directory-sync-v1", "directory-subscriptions-v1"],
      },
      methods,
      scope,
    );
    const payload = object(object(info).payload);
    expect(payload.features).toMatchObject({
      directorySubscriptions: false,
      directorySync: false,
      explicitEventSubscriptions: false,
      workspaceLabels: false,
      hostStats: false,
      projectList: scope === "project",
      workspaceMultiplicity: true,
    });
    expect(payload.sessionEventTypes).toEqual([]);
  },
);
describe("native agent stream control events", () => {
  it.each([
    { type: "turn_started", provider: "codex", turnId: "turn" },
    { type: "usage_updated", provider: "codex", turnId: null, usage: { inputTokens: 100 } },
    {
      type: "permission_requested",
      provider: "codex",
      request: { id: "permission", provider: "codex", name: "Shell", kind: "tool" },
    },
    {
      type: "permission_resolved",
      provider: "codex",
      requestId: "permission",
      resolution: { behavior: "allow" },
    },
  ])("accepts 0.0.27 $type notifications without dropping the stream", (event) => {
    const envelope = eventMessage("agent_stream", {
      subscriptionId: "timeline-lease",
      agentId: "agent",
      event,
    });
    const parsed = validateWSOutboundMessage({ type: "session", message: envelope });
    expect(parsed.success).toBe(true);
    const payload = object(object(envelope).payload);
    expect(Number.isFinite(Date.parse(String(payload.timestamp)))).toBe(true);
    expect(payload.subscriptionId).toBe("timeline-lease");
  });
  it("preserves source timestamps on new host usage events", () => {
    const payload = {
      agentId: "agent",
      timestamp: "2026-10-11T00:00:00Z",
      event: { type: "usage_updated", provider: "codex", usage: { inputTokens: 100 } },
    };
    const envelope = eventMessage("agent_stream", payload);
    expect(validateWSOutboundMessage({ type: "session", message: envelope }).success).toBe(true);
    expect(object(envelope).payload).toEqual(payload);
  });
  it.each([
    { type: "usage_updated", provider: "codex", usage: { inputTokens: "invalid" } },
    { type: "timeline", provider: "codex", item: { type: "assistant_message", text: "hello" } },
    { type: "unknown_event", provider: "codex" },
  ])("keeps malformed $type events subject to protocol validation", (event) => {
    const envelope = eventMessage("agent_stream", { agentId: "agent", event });
    expect(validateWSOutboundMessage({ type: "session", message: envelope }).success).toBe(false);
  });
});
it.each([
  ["agent.update", "agent.update", { agentId: "agent" }],
  ["workspace.update", "workspace.update", { id: "workspace", removedProjectId: "project" }],
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
    expect(SessionOutboundMessageSchema.parse(envelope)).toEqual({ type, payload });
  },
);
function info(methods: string[]) {
  const message = serverInfo({ server_id: "ait" }, new Set(methods));
  return parseServerInfoStatusPayload(object(object(message).payload));
}
describe("Rust daemon software version", () => {
  it.each(["0.0.10", "v0.0.11"])("preserves the connected server's version %s", (version) => {
    const envelope = serverInfo({ server_id: "ait", version }, new Set());
    const value = parseServerInfoStatusPayload(object(object(envelope).payload));
    expect(value?.version).toBe(version);
  });
  it.each([undefined, null, 11, { major: 1, minor: 0 }])(
    "keeps an unknown software version when the server sends %j",
    (version) => {
      const envelope = serverInfo({ server_id: "ait", version }, new Set());
      const value = parseServerInfoStatusPayload(object(object(envelope).payload));
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
  expect(AgentTimelineSearchResponseMessageSchema.parse(envelope).payload).toEqual({
    ...result,
    requestId: "search",
  });
});
describe("Ait host capabilities", () => {
  it.each([
    { list: true, timeline: true },
    { list: true, timeline: false },
    { list: false, timeline: true },
    { list: false, timeline: false },
  ])(
    "advertises provider children with list=$list and timeline=$timeline",
    ({ list, timeline }) => {
      const methods = [
        ...(list ? ["agent.provider_subagents.list.request"] : []),
        ...(timeline ? ["agent.provider_subagents.timeline.get.request"] : []),
      ];
      expect(info(methods)?.features).toMatchObject({
        providerSubagents: list && timeline,
        providerSubagentNesting: list && timeline,
        projectedSubagentTimeline: timeline,
      });
    },
  );
  it.each([
    ["workspace.setup.progress", "workspace-setup-events-v1", "workspace.setup.status.request"],
    ["activity.log", "activity-log-events-v1", "agent.list.request"],
  ])(
    "advertises %s only with its canonical producer feature and method",
    (event, feature, method) => {
      for (const supported of [false, true]) {
        for (const implemented of [false, true]) {
          const envelope = serverInfo(
            { server_id: "ait", features: supported ? [feature] : [] },
            new Set(implemented ? [method] : []),
          );
          const catalog = parseServerInfoStatusPayload(
            object(object(envelope).payload),
          )?.sessionEventTypes;
          expect(catalog?.includes(event)).toBe(supported && implemented);
        }
      }
    },
  );
  it("enables the usage ledger only when the host implements its method", () => {
    expect(info(["provider.usage.ledger.request"])?.features?.providerUsageLedger).toBe(true);
    expect(info(["provider.usage.list.request"])?.features).toMatchObject({
      providerUsageList: true,
      providerUsageLedger: false,
    });
  });
  it("enables cached timeline reads only with the versioned feature and timeline method", () => {
    for (const supported of [false, true]) {
      for (const implemented of [false, true]) {
        const envelope = serverInfo(
          {
            server_id: "ait",
            features: supported ? ["agent-timeline-cached-reads-v1"] : [],
          },
          new Set(implemented ? ["agent.timeline.get.request"] : []),
        );
        expect(
          parseServerInfoStatusPayload(object(object(envelope).payload))?.features
            ?.agentTimelineCachedReads,
        ).toBe(supported && implemented);
      }
    }
  });
  it("advertises directory and script events only with their installed producers", () => {
    const read = (features: string[], methods: string[]) =>
      parseServerInfoStatusPayload(
        object(object(serverInfo({ server_id: "ait", features }, new Set(methods))).payload),
      )?.sessionEventTypes;
    const methods = ["project.list.request", "workspace.script.list.request"];
    expect(read(["directory-subscriptions-v1"], methods)).toEqual(
      expect.arrayContaining(["project.update", "script_status_update"]),
    );
    expect(read([], methods)).not.toContain("project.update");
    expect(read(["directory-subscriptions-v1"], [])).not.toContain("project.update");
    expect(read(["directory-subscriptions-v1"], [])).not.toContain("script_status_update");
  });
  it("advertises checkout status events only with the Git producer and checkout method", () => {
    const read = (features: string[], methods: string[]) =>
      parseServerInfoStatusPayload(
        object(object(serverInfo({ server_id: "ait", features }, new Set(methods))).payload),
      )?.sessionEventTypes;
    const method = "checkout.status.get.request";
    expect(read(["checkout-git-events-v1"], [method])).toContain("checkout.status.update");
    expect(read([], [method])).not.toContain("checkout.status.update");
    expect(read(["checkout-git-events-v1"], [])).not.toContain("checkout.status.update");
  });
  it("enables composite creation only with the lifecycle producer and its methods", () => {
    const methods = new Set([
      "agent.create.request",
      "workspace.create.request",
      "creation.subscribe.request",
    ]);
    const read = (features: string[]) =>
      parseServerInfoStatusPayload(
        object(object(serverInfo({ server_id: "ait", features }, methods)).payload),
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
  it("offers sandboxed worktrees only when the host advertises confinement", () => {
    const read = (features: string[], methods: string[]) =>
      parseServerInfoStatusPayload(
        object(object(serverInfo({ server_id: "ait", features }, new Set(methods))).payload),
      )?.features?.workspaceSandbox;
    expect(read(["workspace-sandbox-v1"], ["workspace.create.request"])).toBe(true);
    expect(read([], ["workspace.create.request"])).toBe(false);
    expect(read(["workspace-sandbox-v1"], [])).toBe(false);
  });
  it("advertises native approval and child events only with the producer feature", () => {
    const read = (features: string[], methods: string[]) =>
      parseServerInfoStatusPayload(
        object(object(serverInfo({ server_id: "ait", features }, new Set(methods))).payload),
      )?.sessionEventTypes;
    const method = "agent.permission.resolve.request";
    expect(read(["agent-session-events-v1"], [method])).toEqual(
      expect.arrayContaining([
        "agent.permission.request",
        "agent.permission.resolved",
        "agent.provider_subagents.update",
      ]),
    );
    expect(read([], [method])).not.toContain("agent.permission.request");
    expect(read(["agent-session-events-v1"], [])).not.toContain("agent.permission.resolved");
  });
  it("advertises terminal attention only for hosts with terminal activity events", () => {
    const read = (features: string[], methods: string[]) =>
      parseServerInfoStatusPayload(
        object(object(serverInfo({ server_id: "ait", features }, new Set(methods))).payload),
      )?.sessionEventTypes;
    expect(read(["terminal-activity-v1"], ["terminal.list.request"])).toContain(
      "terminal.attention.required",
    );
    expect(read([], ["terminal.list.request"])).not.toContain("terminal.attention.required");
    expect(read(["terminal-activity-v1"], [])).not.toContain("terminal.attention.required");
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
        object(object(serverInfo({ server_id: "ait", features: advertised }, methods)).payload),
      )?.features;
    expect(read(features)).toMatchObject({ directorySync: true, directorySubscriptions: true });
    expect(read([])).toMatchObject({ directorySync: false, directorySubscriptions: false });
    methods.delete("agent.list.request");
    expect(read(features)).toMatchObject({
      directorySync: false,
      directorySubscriptions: false,
    });
  });

  it("enables workspace pinning only when the host implements the method", () => {
    for (const supported of [false, true]) {
      const info = serverInfo(
        { server_id: "ait" },
        new Set(supported ? ["workspace.pin.set.request"] : []),
      );
      expect(
        parseServerInfoStatusPayload(object(object(info).payload))?.features?.workspacePinning,
      ).toBe(supported);
    }
  });

  it("enables Workspace target reads only when the host implements the method", () => {
    for (const supported of [false, true]) {
      const info = serverInfo(
        { server_id: "ait" },
        new Set(supported ? ["workspace.get.request"] : []),
      );
      expect(
        parseServerInfoStatusPayload(object(object(info).payload))?.features?.workspaceTargetRead,
      ).toBe(supported);
    }
  });
  it("enables GitLab panels only when the server advertises its installed adapter", () => {
    const methods = new Set([
      "checkout.pr.status.request",
      "checkout.forge.get_check_details.request",
      "checkout.forge.set_auto_merge.request",
    ]);
    const envelope = serverInfo({ server_id: "ait", features: ["forge-gitlab-v1"] }, methods);
    const value = parseServerInfoStatusPayload(object(object(envelope).payload));
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
      providerPresets: true,
      sideQuestions: true,
      providerRemoval: true,
      projectCustomIcon: true,
      importSessionWorkspaceTarget: true,
      importSessionSearch: true,
      directorySubscriptions: false,
      workspaceSetupObservation: true,
      daemonPairing: false,
    });
    expect(value?.features).not.toHaveProperty("plugins");
    expect(value?.sessionEventTypes).toEqual([
      "status.server_info",
      "script_status_update",
      "workspace_setup_status",
      "status.daemon_config_changed",
      "provider.snapshot.update",
      "agent.attention.required",
    ]);
  });
  it("does not enable controls without all their required methods", () => {
    expect(info(["project.list.request", "daemon.config.get.request"])?.features).toMatchObject({
      projectList: true,
      projectAdd: false,
      stableProjectIdentity: false,
      agentProfiles: false,
      providerPresets: false,
      sideQuestions: false,
      providerRemoval: false,
      projectCustomIcon: false,
      importSessionWorkspaceTarget: false,
      importSessionSearch: false,
    });
  });
  it("exposes preset management only with every required provider method", () => {
    const methods = [
      "provider.presets.describe.request",
      "provider.presets.list.request",
      "provider.presets.save.request",
      "provider.presets.delete.request",
    ];
    expect(info(methods)?.features?.providerPresets).toBe(true);
    for (const missing of methods) {
      expect(info(methods.filter((method) => method !== missing))?.features?.providerPresets).toBe(
        false,
      );
    }
    for (const scope of ["workspace", "project"] as const) {
      const status = serverInfo({ server_id: "shared-host" }, new Set(methods), scope);
      expect(parseServerInfoStatusPayload(object(status).payload)?.features?.providerPresets).toBe(
        false,
      );
    }
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
  const parsed = SessionOutboundMessageSchema.parse(envelope);
  expect(parsed).toMatchObject({
    type: "checkout.status.update",
    payload: { ...payload, requestId: "" },
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
      "provider.snapshot.get.response",
      "catalog",
      { entries, snapshotHash: "hash", notModified: false },
      {},
    );
    expect(object(message).payload).toMatchObject({
      requestId: "catalog",
      snapshotHash: "hash",
      compactSnapshot: { entries: [{ provider: "codex", models: [{ id: "model-1" }] }] },
      fetchedAt: { codex: "2026-09-27T00:00:00Z" },
    });
  });
  it("normalizes pushed catalogs as full snapshots too", () => {
    const message = eventMessage("provider.snapshot.update", {
      entries,
      snapshotHash: "hash",
      subscriptionId: "feed",
    });
    expect(object(message).payload).toMatchObject({
      subscriptionId: "feed",
      compactSnapshot: { entries: [{ provider: "codex" }] },
    });
  });
  it("does not replace a not-modified reply with an empty catalog", () => {
    const message = responseMessage(
      "provider.snapshot.get.response",
      "cached",
      { entries: [], snapshotHash: "hash", notModified: true },
      {},
    );
    expect(object(message).payload).not.toHaveProperty("compactSnapshot");
  });
  it("keeps a genuinely empty full catalog cacheable", () => {
    const message = responseMessage(
      "provider.snapshot.get.response",
      "empty",
      { entries: [], snapshotHash: "empty", notModified: false },
      {},
    );
    expect(object(message).payload).toMatchObject({
      compactSnapshot: { entries: [], thinkingSets: [] },
    });
  });
});
it("uses daemon voice readiness without upstream SDP and recognizes terminal realtime events", () => {
  const response = responseMessage(
    "voice.mode.set.response",
    "voice",
    { accepted: true, enabled: true, agentId: "agent", error: null },
    {},
  );
  expect(validateWSOutboundMessage({ type: "session", message: response }).success).toBe(true);
  expect(object(object(response).payload)).not.toHaveProperty("sdp");
  for (const [method, params] of [
    ["voice.session.closed", {}],
    [
      "voice.error",
      { error: "native voice unavailable", reasonCode: "native_realtime_failed", retryable: false },
    ],
  ] as const) {
    expect(
      validateWSOutboundMessage({ type: "session", message: eventMessage(method, params) }).success,
    ).toBe(true);
  }
});

it("advertises host monitoring only when the daemon declares its method", () => {
  expect(info([])?.features?.hostStats).toBe(false);
  expect(info(["daemon.host_stats.request"])?.features?.hostStats).toBe(true);
});
