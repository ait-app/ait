import { ProviderSnapshotEntrySchema } from "@ait/protocol/messages";
import { compactProviderSnapshot } from "@ait/protocol/provider-snapshot-codec";
import { sessionEventMethod } from "@ait/protocol/session-event-kinds";
import { object, type Payload } from "./types";

export function session(type: string, payload: Payload): Payload {
  return { type: "session", message: { type, payload } };
}

export function rpcError(
  requestId: string,
  requestType: string,
  code: string,
  error: string,
): Payload {
  return session("rpc_error", { requestId, requestType, code, error });
}

// The SDK uses these flags to choose wire shapes, not just to show UI controls.
// Only advertise behavior supported by this adapter AND the Rust implementation.
export function serverInfo(info: Payload, implemented: Set<string>): Payload {
  const has = (method: string) => implemented.has(method);
  const supportsDirectories = [
    "project.list.request",
    "workspace.list.request",
    "agent.list.request",
  ].every(has);
  const features = new Set(Array.isArray(info.features) ? info.features : []);
  return session("status", {
    status: "server_info",
    serverId: info.server_id,
    hostname: null,
    version: typeof info.version === "string" ? info.version : null,
    desktopManaged: false,
    sessionEventTypes: [
      "status.server_info",
      ...(features.has("checkout-git-events-v1") && has("checkout.status.get.request")
        ? ["checkout.status.update"]
        : []),
      ...(has("daemon.config.get.request") ? ["status.daemon_config_changed"] : []),
      ...(has("provider.snapshot.get.request")
        ? ["provider.snapshot.update", "agent.attention.required"]
        : []),
      ...(features.has("terminal-activity-v1") && has("terminal.list.request")
        ? ["terminal.attention.required"]
        : []),
      ...(features.has("agent-session-events-v1") && has("agent.permission.resolve.request")
        ? [
            "agent.permission.request",
            "agent.permission.resolved",
            "agent.provider_subagents.update",
          ]
        : []),
    ],
    features: {
      ownedSubscriptions: has("subscription.release.request"),
      explicitEventSubscriptions: has("session.events.set_subscription.request"),
      providersSnapshot: has("provider.snapshot.get.request"),
      importSessionWorkspaceTarget: has("agent.import.request"),
      importSessionSearch: has("provider.sessions.recent.list.request"),
      workspaceLabels: has("workspace.label.list.request"),
      agentConfigApply: has("agent.config.apply.request"),
      daemonConfigReload: has("daemon.config.reload.request"),
      daemonStatusRpc: has("daemon.get_status.request"),
      onlineServiceSync: [
        "relay.status.request",
        "relay.start.request",
        "relay.stop.request",
      ].every(has),
      daemonDiagnostics: has("diagnostics.request"),
      skillManagement: has("agent.skills.get_status.request"),
      pushTokenRevocation: has("push.unregister.request"),
      workspaceFileEditing: has("fs.file.write.request"),
      projectAdd: has("project.add.request"),
      projectRemove: has("project.remove.request"),
      workspaceRecovery: has("workspace.recovery.inspect.request"),
      workspaceSetupRun: has("workspace.setup.run.request"),
      workspaceTerminals: has("terminal.list.request"),
      "terminal-restore-modes": has("terminal.subscribe.request"),
      "terminal-input-mode-replay": has("terminal.subscribe.request"),
      "terminal-size-ownership": has("terminal.input"),
      forgeSearch: has("forge.search.request"),
      forgeProviders: features.has("forge-gitlab-v1"),
      forgeCheckDetails: has("checkout.forge.get_check_details.request"),
      checkoutForgeSetAutoMerge: has("checkout.forge.set_auto_merge.request"),
      checkoutRefresh: has("checkout.refresh.request"),
      providerUsageList: has("provider.usage.list.request"),
      agentHistorySearch: has("agent.history.get.request"),
      agentForkContext: has("agent.fork_context.request"),
      agentDetach: has("agent.detach.request"),
      agentTimelinePromptIndex: has("agent.timeline.list_prompts.request"),
      rewind: has("agent.rewind.request"),
      directorySync: supportsDirectories && features.has("directory-sync-v1"),
      directorySubscriptions:
        supportsDirectories &&
        has("subscription.release.request") &&
        features.has("directory-subscriptions-v1"),
      daemonPairing: false,
      providerConfiguration: false,
      projectList: has("project.list.request"),
      stableProjectIdentity: has("project.list.request") && has("project.add.request"),
      workspaceMultiplicity: has("workspace.list.request"),
      projectCreateDirectory: has("project.create_directory.request"),
      projectGithubClone: has("project.github.clone.request"),
      workspaceGithubRepositorySearch: has("workspace.github.search_repositories.request"),
      projectCustomIcon: has("project.icon.get.request") && has("project.icon.set.request"),
      providerRemoval: has("daemon.config.set.request"),
      agentProfiles:
        has("daemon.config.get.request") &&
        has("daemon.config.set.request") &&
        has("agent.config.apply.request"),
      creationLifecycle:
        features.has("creation-lifecycle-v1") &&
        has("agent.create.request") &&
        has("workspace.create.request") &&
        has("creation.subscribe.request"),
      agentRequestReceipts: features.has("creation-lifecycle-v1") && has("agent.create.request"),
      workspaceRequestReceipts:
        features.has("creation-lifecycle-v1") && has("workspace.create.request"),
      projectedSubagentTimeline: has("agent.provider_subagents.timeline.get.request"),
    },
  });
}

function providerSnapshot(payload: Payload): Payload {
  if (payload.notModified === true || payload.compactSnapshot || !Array.isArray(payload.entries))
    return payload;
  const entries = ProviderSnapshotEntrySchema.array().parse(payload.entries);
  // Rust hashes expanded entries. Supply the SDK's content body as well: a hash
  // without compactSnapshot is interpreted as a cache-only announcement.
  return {
    ...payload,
    compactSnapshot: compactProviderSnapshot(
      entries.map(({ fetchedAt: _fetchedAt, ...entry }) => entry),
    ),
    fetchedAt: Object.fromEntries(
      entries.flatMap((entry) => (entry.fetchedAt ? [[entry.provider, entry.fetchedAt]] : [])),
    ),
  };
}

export function eventMessage(method: string, params: unknown): Payload {
  method = sessionEventMethod(method);
  const payload = object(params);
  if (method === "checkout.status.update") {
    // The SDK reuses the correlated status schema for unsolicited updates.
    return session("checkout.status.update", { requestId: "", ...payload });
  }
  if (method === "provider.snapshot.update") {
    return session("provider.snapshot.update", providerSnapshot(payload));
  }
  if (method === "browser.automation.execute.request") {
    return { type: "session", message: { type: method, ...payload } };
  }
  if (method.startsWith("status.")) {
    // Status event payloads retain their SDK discriminator.
    return session("status", {
      ...payload,
      status: method.slice("status.".length),
    });
  }
  return session(method, payload);
}

export function responseMessage(
  response: string,
  requestId: string,
  result: unknown,
  request: Payload,
): Payload {
  const payload: Payload = { ...object(result), requestId };
  if (response === "provider.snapshot.get.response")
    return session(response, providerSnapshot(payload));
  if (response.startsWith("status:")) {
    const status = response.slice("status:".length);
    return session("status", {
      ...payload,
      ...(payload.agentId === undefined && typeof request.agentId === "string"
        ? { agentId: request.agentId }
        : {}),
      status,
    });
  }
  return session(response, payload);
}
