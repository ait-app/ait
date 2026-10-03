import { ProviderSnapshotEntrySchema } from "@ait/protocol/messages";
import { compactProviderSnapshot } from "@ait/protocol/provider-snapshot-codec";
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
        ? ["checkout_status_update"]
        : []),
      ...(has("daemon.config.get.request") ? ["status.daemon_config_changed"] : []),
      ...(has("provider.snapshot.get.request")
        ? ["providers_snapshot_update", "agent_attention_required"]
        : []),
      ...(features.has("terminal-activity-v1") && has("terminal.list.request")
        ? ["terminal_attention_required"]
        : []),
      ...(features.has("agent-session-events-v1") && has("agent.permission.resolve.request")
        ? [
            "agent_permission_request",
            "agent_permission_resolved",
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
      commitsList: has("checkout.commits.list.request"),
      // Rust list responses always include the required isOnBase classification.
      commitBaseClassification: has("checkout.commits.list.request"),
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

const EVENTS: Readonly<Record<string, string>> = {
  "agent.update": "agent_update",
  "workspace.update": "workspace_update",
  "provider.snapshot.update": "providers_snapshot_update",
  "agent.attention.required": "agent_attention_required",
  "agent.permission.request": "agent_permission_request",
  "agent.permission.resolved": "agent_permission_resolved",
  "agent.stream": "agent_stream",
  "checkout.diff.update": "checkout_diff_update",
  "checkout.status.update": "checkout_status_update",
  "terminal.stream.exit": "terminal_stream_exit",
  "terminal.list.changed": "terminals_changed",
  "terminal.attention.required": "terminal_attention_required",
  "voice.audio.output": "audio_output",
  "voice.input.state": "voice_input_state",
  "voice.transcription.result": "transcription_result",
  "voice.assistant.chunk": "assistant_chunk",
  "voice.error": "error",
  "dictation.stream.ack": "dictation_stream_ack",
  "dictation.stream.partial": "dictation_stream_partial",
  "dictation.stream.final": "dictation_stream_final",
  "dictation.stream.error": "dictation_stream_error",
  "dictation.stream.finish.accepted": "dictation_stream_finish_accepted",
};

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
  const payload = object(params);
  if (method === "checkout.status.update") {
    // The SDK reuses the correlated status schema for unsolicited updates.
    return session("checkout_status_update", { requestId: "", ...payload });
  }
  if (method === "providers_snapshot_update" || method === "provider.snapshot.update") {
    return session("providers_snapshot_update", providerSnapshot(payload));
  }
  if (method === "browser.automation.execute.request") {
    return { type: "session", message: { type: method, ...payload } };
  }
  if (method.startsWith("status.")) {
    // Session server-info events are already Paseo status payloads.
    return session("status", {
      ...payload,
      status: method.slice("status.".length),
    });
  }
  return session(EVENTS[method] ?? method, payload);
}

export function responseMessage(
  response: string,
  requestId: string,
  result: unknown,
  request: Payload,
): Payload {
  const payload: Payload = { ...object(result), requestId };
  if (response === "get_providers_snapshot_response")
    return session(response, providerSnapshot(payload));
  if (response.startsWith("status:")) {
    const status = response.slice("status:".length);
    if (status === "agent_created" && typeof payload.error === "string") {
      return session("status", {
        ...payload,
        status: "agent_create_failed",
      });
    }
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
