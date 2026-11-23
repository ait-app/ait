import {
  BOT_FEISHU_SETUP_METHODS,
  BOT_LARK_SETUP_METHODS,
  BOT_IMESSAGE_SETUP_METHODS,
  BOT_WEBHOOK_SETUP_METHODS,
  BOT_METHODS,
  BOT_NOTIFICATION_METHODS,
  BOT_PERMISSION_METHODS,
  BOT_QQ_SETUP_METHODS,
  BOT_TELEGRAM_SETUP_METHODS,
  BOT_TICKET_ANSWER_METHODS,
  BOT_TICKET_METHODS,
  BOT_WEIXIN_SETUP_METHODS,
} from "@ait/protocol/bot/rpc-schemas";
import { ProviderSnapshotEntrySchema, type ServerInfoStatusPayload } from "@ait/protocol/messages";
import { compactProviderSnapshot } from "@ait/protocol/provider-snapshot-codec";
import { sessionEventMethod } from "@ait/protocol/session-event-kinds";
import { WORKSPACE_NOTEPAD_METHODS } from "@ait/protocol/workspace-notepad";
import { object, type Payload } from "./types.js";
import type { HubShareResourceKind } from "../hub/index.js";

export function session(type: string, payload: Payload): Payload {
  return { type, payload };
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
export function serverInfo(
  info: Payload,
  implemented: Set<string>,
  guestScope?: HubShareResourceKind,
): Payload {
  // Scoped guests can read their directories, but whole-host subscriptions and
  // synchronization cursors are rejected by the daemon's guest policy.
  const scopedGuest = guestScope === "workspace" || guestScope === "project";
  const has = (method: string) => implemented.has(method);
  const supports = (methods: readonly string[]) => methods.every(has);
  const supportsDirectories = [
    "project.list.request",
    "workspace.list.request",
    "agent.list.request",
  ].every(has);
  const supportsProviderSubagents = [
    "agent.provider_subagents.list.request",
    "agent.provider_subagents.timeline.get.request",
  ].every(has);
  const serverFeatures = new Set(Array.isArray(info.features) ? info.features : []);
  // Keep this map required so adding a feature to ServerInfoStatusPayload forces
  // the producer to decide how the current daemon advertises it.
  const featureFlags: Required<NonNullable<ServerInfoStatusPayload["features"]>> = {
    agentRequestReceipts:
      serverFeatures.has("creation-lifecycle-v1") && has("agent.create.request"),
    workspaceRequestReceipts:
      serverFeatures.has("creation-lifecycle-v1") && has("workspace.create.request"),
    creationLifecycle:
      serverFeatures.has("creation-lifecycle-v1") &&
      supports(["agent.create.request", "workspace.create.request", "creation.subscribe.request"]),
    workspaceBots: serverFeatures.has("workspace-bots-v2") && BOT_METHODS.every(has),
    workspaceBotPermissions:
      serverFeatures.has("workspace-bots-v2") &&
      BOT_METHODS.every(has) &&
      BOT_PERMISSION_METHODS.every(has),
    workspaceBotNotifications:
      serverFeatures.has("workspace-bots-v2") &&
      BOT_METHODS.every(has) &&
      BOT_NOTIFICATION_METHODS.every(has),
    workspaceBotTickets:
      serverFeatures.has("workspace-bots-v2") &&
      BOT_METHODS.every(has) &&
      BOT_TICKET_METHODS.every(has),
    workspaceBotTicketAnswers:
      serverFeatures.has("workspace-bots-v2") &&
      BOT_METHODS.every(has) &&
      BOT_TICKET_METHODS.every(has) &&
      BOT_TICKET_ANSWER_METHODS.every(has),
    workspaceNotepad: supports(WORKSPACE_NOTEPAD_METHODS),
    workspaceBotFeishuSetup:
      serverFeatures.has("workspace-bots-v2") &&
      BOT_METHODS.every(has) &&
      BOT_FEISHU_SETUP_METHODS.every(has),
    workspaceBotLarkSetup:
      serverFeatures.has("workspace-bots-v2") &&
      BOT_METHODS.every(has) &&
      BOT_LARK_SETUP_METHODS.every(has),
    workspaceBotTelegramSetup:
      serverFeatures.has("workspace-bots-v2") &&
      BOT_METHODS.every(has) &&
      BOT_TELEGRAM_SETUP_METHODS.every(has),
    workspaceBotWeixinSetup:
      serverFeatures.has("workspace-bots-v2") &&
      BOT_METHODS.every(has) &&
      BOT_WEIXIN_SETUP_METHODS.every(has),
    workspaceBotQqSetup:
      serverFeatures.has("workspace-bots-v2") &&
      BOT_METHODS.every(has) &&
      BOT_QQ_SETUP_METHODS.every(has),
    workspaceBotImessageSetup:
      serverFeatures.has("workspace-bots-v2") &&
      BOT_METHODS.every(has) &&
      BOT_IMESSAGE_SETUP_METHODS.every(has),
    workspaceBotWebhookSetup:
      serverFeatures.has("workspace-bots-v2") &&
      BOT_METHODS.every(has) &&
      BOT_WEBHOOK_SETUP_METHODS.every(has),
    providersSnapshot: has("provider.snapshot.get.request"),
    providersSnapshotCwd: has("provider.snapshot.get.request"),
    directorySync: !scopedGuest && supportsDirectories && serverFeatures.has("directory-sync-v1"),
    directorySubscriptions:
      !scopedGuest &&
      supportsDirectories &&
      has("subscription.release.request") &&
      serverFeatures.has("directory-subscriptions-v1"),
    daemonPairing: false,
    providerConfiguration: false,
    workspaceLabels: !scopedGuest && has("workspace.label.list.request"),
    workspaceSetupRun: has("workspace.setup.run.request"),
    workspaceSetupObservation:
      has("workspace.setup.subscribe.request") && has("workspace.setup.summary.request"),
    workspaceTerminals: has("terminal.list.request"),
    checkoutForgeSetAutoMerge: has("checkout.forge.set_auto_merge.request"),
    // Keep the legacy GitHub flags populated for clients that still select the
    // GitHub-specific RPCs while the forge-neutral aliases roll out.
    checkoutGithubSetAutoMerge: has("checkout.github.set_auto_merge.request"),
    githubCheckDetails: has("checkout.github.get_check_details.request"),
    forgeCheckDetails: has("checkout.forge.get_check_details.request"),
    forgeSearch: has("forge.search.request"),
    daemonStatusRpc: has("daemon.get_status.request"),
    hostStats: !scopedGuest && has("daemon.host_stats.request"),
    onlineServiceSync: supports([
      "relay.status.request",
      "relay.start.request",
      "relay.stop.request",
    ]),
    daemonConfigReload: has("daemon.config.reload.request"),
    relayConfig: supports(["daemon.config.get.request", "daemon.config.set.request"]),
    pushTokenRevocation: has("push.unregister.request"),
    // Plugin RPCs are retired from the Rust daemon; keep the compatibility flag
    // explicit so it cannot disappear from the producer silently.
    pluginTimelineItems: false,
    skillManagement: has("agent.skills.get_status.request"),
    "terminal-restore-modes": has("terminal.subscribe.request"),
    "terminal-input-mode-replay": has("terminal.subscribe.request"),
    "terminal-size-ownership": has("terminal.input"),
    rewind: has("agent.rewind.request"),
    agentTimelinePromptIndex: has("agent.timeline.list_prompts.request"),
    agentHistorySearch: has("agent.history.get.request"),
    checkoutRefresh: has("checkout.refresh.request"),
    workspaceMultiplicity: has("workspace.list.request"),
    workspaceTargetRead: has("workspace.get.request"),
    projectRemove: has("project.remove.request"),
    projectAdd: has("project.add.request"),
    worktreeRestore: has("workspace.recovery.restore.request"),
    workspaceRecovery: has("workspace.recovery.inspect.request"),
    workspaceFileEditing: has("fs.file.write.request"),
    providerUsageList: has("provider.usage.list.request"),
    providerUsageLedger: has("provider.usage.ledger.request"),
    agentDetach: has("agent.detach.request"),
    agentTimelineExport:
      has("agent.timeline.export.request") && has("agent.timeline.export.cancel.request"),
    agentThinkingUpdate: has("agent.thinking.set.request"),
    daemonDiagnostics: has("diagnostics.request"),
    daemonSelfUpdate: has("daemon.update.request"),
    agentForkContext: has("agent.fork_context.request"),
    agentForkContextCursor: has("agent.fork_context.request"),
    unifiedSubagentAgents: has("agent.subagents.list.request"),
    providerSubagents: supportsProviderSubagents,
    projectedSubagentTimeline: has("agent.provider_subagents.timeline.get.request"),
    providerSubagentNesting: supportsProviderSubagents,
    subagentControls: has("agent.subagents.guide.request") && has("agent.subagents.stop.request"),
    workspacePinning: has("workspace.pin.set.request"),
    workspaceMarkUnread: has("workspace.mark_unread.request"),
    projectGithubClone: has("project.github.clone.request"),
    workspaceGithubRepositorySearch: has("workspace.github.search_repositories.request"),
    projectCreateDirectory: has("project.create_directory.request"),
    projectList: guestScope !== "workspace" && has("project.list.request"),
    commitsList: has("checkout.commits.list.request"),
    commitBaseClassification: has("checkout.commits.list.request"),
    providerRemoval: has("daemon.config.set.request"),
    importSessionWorkspaceTarget: has("agent.import.request"),
    importSessionSearch: has("provider.sessions.recent.list.request"),
    importSessionPreview: has("provider.sessions.preview.get.request"),
    forgeProviders: serverFeatures.has("forge-gitlab-v1"),
    workspaceSandbox: serverFeatures.has("workspace-sandbox-v1") && has("workspace.create.request"),
    selectiveAgentTimeline: has("agent.timeline.set_subscription.request"),
    agentTimelineCachedReads:
      has("agent.timeline.get.request") && serverFeatures.has("agent-timeline-cached-reads-v1"),
    explicitEventSubscriptions: !scopedGuest && has("session.events.set_subscription.request"),
    ownedSubscriptions: has("subscription.release.request"),
    canonicalSubmittedPrompts: has("agent.message.send.request"),
    agentTurnIdentity: has("agent.list.request"),
    stableProjectIdentity: has("project.list.request") && has("project.add.request"),
    workspaceScriptManagement: supports([
      "workspace.script.list.request",
      "workspace.script.start.request",
      "workspace.script.stop.request",
      "workspace.script.run.get.request",
      "workspace.script.run.wait.request",
    ]),
    projectCustomIcon: has("project.icon.get.request") && has("project.icon.set.request"),
    fsEntryOps: supports([
      "fs.entry.create.request",
      "fs.entry.rename.request",
      "fs.entry.delete.request",
    ]),
    fsEntryDuplicate: has("fs.entry.duplicate.request"),
    checkoutDiscardChanges: has("checkout.discard_changes.request"),
    agentProfiles:
      has("daemon.config.get.request") &&
      has("daemon.config.set.request") &&
      has("agent.config.apply.request"),
    agentConfigApply: has("agent.config.apply.request"),
    providerPresets:
      !scopedGuest &&
      supports([
        "provider.presets.describe.request",
        "provider.presets.list.request",
        "provider.presets.save.request",
        "provider.presets.delete.request",
      ]),
    sideQuestions: ["ask", "cancel", "history", "transfer"].every((operation) =>
      has(`agent.side_question.${operation}.request`),
    ),
  };
  return session("status", {
    status: "server_info",
    serverId: info.server_id,
    hostname: null,
    version: typeof info.version === "string" ? info.version : null,
    desktopManaged: false,
    sessionEventTypes: scopedGuest
      ? []
      : [
          "status.server_info",
          ...(serverFeatures.has("directory-subscriptions-v1") && has("project.list.request")
            ? ["project.update"]
            : []),
          ...(has("workspace.script.list.request") ? ["script_status_update"] : []),
          ...(has("workspace.setup.subscribe.request") ? ["workspace_setup_status"] : []),
          ...(serverFeatures.has("workspace-setup-events-v1") &&
          has("workspace.setup.status.request")
            ? ["workspace.setup.progress"]
            : []),
          ...(serverFeatures.has("activity-log-events-v1") && has("agent.list.request")
            ? ["activity.log"]
            : []),
          ...(serverFeatures.has("checkout-git-events-v1") && has("checkout.status.get.request")
            ? ["checkout.status.update"]
            : []),
          ...(has("daemon.config.get.request") ? ["status.daemon_config_changed"] : []),
          ...(has("provider.snapshot.get.request")
            ? ["provider.snapshot.update", "agent.attention.required"]
            : []),
          ...(serverFeatures.has("terminal-activity-v1") && has("terminal.list.request")
            ? ["terminal.attention.required"]
            : []),
          ...(serverFeatures.has("agent-session-events-v1") &&
          has("agent.permission.resolve.request")
            ? [
                "agent.permission.request",
                "agent.permission.resolved",
                "agent.provider_subagents.update",
              ]
            : []),
        ],
    features: featureFlags,
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
  if (method === "agent.stream") {
    // 0.0.27 omitted timestamps on native control and usage notifications.
    // Use receipt time for those legacy events while preserving source timestamps.
    const event = object(payload.event);
    if (
      payload.timestamp === undefined &&
      ["turn_started", "usage_updated", "permission_requested", "permission_resolved"].includes(
        String(event.type),
      )
    ) {
      return session(method, { ...payload, timestamp: new Date().toISOString() });
    }
  }
  if (method === "checkout.status.update") {
    // The SDK reuses the correlated status schema for unsolicited updates.
    return session("checkout.status.update", { requestId: "", ...payload });
  }
  if (method === "provider.snapshot.update") {
    return session("provider.snapshot.update", providerSnapshot(payload));
  }
  if (method === "browser.automation.execute.request") {
    return { type: method, ...payload };
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
