/**
 * One entry per Paseo MCP tool: the timeline label, the context shown after it,
 * and how the expanded details lay out the call.
 *
 * Labels are fixed text so truncation never hides them. Actions read verb-first
 * ("Send message"); the context goes in the summary.
 *
 * Entries are keyed by the current domain-first names. `FORMER_NAMES` maps the
 * names the tools had before, which stored transcripts and call aliases keep.
 * Renamed input fields are listed under both spellings for the same reason.
 */

type ToolInput = Readonly<Record<string, unknown>>;

/** `targetAgent` is the target agent's title when known, else its ID. */
export type PaseoToolContext = (input: ToolInput, targetAgent: string | undefined) => unknown;

export interface PaseoToolDetailSpec {
  promptField?: string;
  inputOrder?: readonly string[];
  outputFields?: readonly string[];
}

export interface PaseoToolEntry {
  label: string;
  context?: PaseoToolContext;
  detail?: PaseoToolDetailSpec;
}

const WORKSPACE_FIELDS = [
  "title",
  "workspaceId",
  "projectId",
  "isolation",
  "path",
  "mode",
  "worktreeSlug",
  "branchName",
  "baseBranch",
  "branch",
  "prNumber",
  "forge",
] as const;
const AGENT_FIELDS = [
  "title",
  "name",
  "agentId",
  "provider",
  "workspaceId",
  "cwd",
  "sessionMode",
  "modeId",
  "background",
  "notifyOnFinish",
  "internal",
  "settings",
  "labels",
] as const;
const AUTOMATION_FIELDS = [
  "name",
  "scheduleId",
  "heartbeatId",
  "id",
  "cron",
  "timezone",
  "provider",
  "cwd",
  "isolation",
  "maxRuns",
  "expiresIn",
  "clearExpires",
] as const;
const BROWSER_FIELDS = [
  "browserId",
  "url",
  "ref",
  "sourceRef",
  "targetRef",
  "value",
  "text",
  "key",
  "button",
  "doubleClick",
  "modifiers",
  "filePaths",
  "fullPage",
  "maxEntries",
  "timeoutMs",
  "deltaX",
  "deltaY",
  "width",
  "height",
  "function",
] as const;

const AUTOMATION_OUTPUT = [
  "scheduleId",
  "heartbeatId",
  "id",
  "status",
  "nextRunAt",
  "expiresAt",
] as const;
const SCHEDULE_ID = ["scheduleId", "id"] as const;
const HEARTBEAT_ID = ["heartbeatId", "id"] as const;

function field(...keys: string[]): PaseoToolContext {
  return (input) => keys.map((key) => input[key]).find(isPresent);
}

function targetAgent(): PaseoToolContext {
  return (_input, agent) => agent;
}

function fieldOrTargetAgent(...keys: string[]): PaseoToolContext {
  return (input, agent) => keys.map((key) => input[key]).find(isPresent) ?? agent;
}

function workspaceContext(): PaseoToolContext {
  return (input) =>
    [input.title, input.branchName, input.branch, baseName(input.path)].find(isPresent);
}

function viewportContext(): PaseoToolContext {
  return (input) =>
    typeof input.width === "number" && typeof input.height === "number"
      ? `${input.width}×${input.height}`
      : undefined;
}

function isPresent(value: unknown): boolean {
  if (typeof value === "string") return value.trim().length > 0;
  if (Array.isArray(value)) return value.length > 0;
  return typeof value === "number";
}

function baseName(value: unknown): string | undefined {
  if (typeof value !== "string") return undefined;
  return value.split(/[\\/]/).findLast((segment) => segment.length > 0);
}

function browser(label: string, context?: PaseoToolContext): PaseoToolEntry {
  return { label, ...(context ? { context } : {}), detail: { inputOrder: BROWSER_FIELDS } };
}

const TOOLS = {
  // Agents
  agent_create: {
    label: "Create agent",
    context: field("title", "initialPrompt"),
    detail: {
      promptField: "initialPrompt",
      inputOrder: AGENT_FIELDS,
      outputFields: ["agentId", "status", "currentModeId", "cwd"],
    },
  },
  agent_prompt: {
    label: "Send message",
    context: field("prompt"),
    detail: {
      promptField: "prompt",
      inputOrder: AGENT_FIELDS,
      outputFields: ["status", "lastMessage", "permission"],
    },
  },
  agent_get: {
    label: "Check agent",
    context: targetAgent(),
    detail: { inputOrder: ["agentId"] },
  },
  agent_activity_read: {
    label: "Read agent activity",
    context: targetAgent(),
    detail: { inputOrder: ["agentId", "limit"] },
  },
  agent_wait: {
    label: "Wait for agent",
    context: targetAgent(),
    detail: { inputOrder: ["agentId", "timeoutMs"] },
  },
  agent_list: {
    label: "List agents",
    detail: { inputOrder: ["cwd", "statuses", "sinceHours", "limit", "includeArchived"] },
  },
  agent_update: {
    label: "Update agent",
    context: fieldOrTargetAgent("title", "name"),
    detail: { inputOrder: AGENT_FIELDS },
  },
  agent_mode_set: {
    label: "Set agent mode",
    context: field("modeId"),
    detail: { inputOrder: ["agentId", "modeId"] },
  },
  agent_cancel: {
    label: "Cancel agent",
    context: targetAgent(),
    detail: { inputOrder: ["agentId"] },
  },
  agent_archive: {
    label: "Archive agent",
    context: targetAgent(),
    detail: { inputOrder: ["agentId"] },
  },
  // Now an alias of `agent_archive`; transcripts keep the label of the tool that was called.
  kill_agent: { label: "Kill agent", context: targetAgent(), detail: { inputOrder: ["agentId"] } },
  permission_list: { label: "List permissions" },
  permission_respond: {
    label: "Respond to permission",
    context: targetAgent(),
    detail: { inputOrder: ["agentId", "requestId", "response"] },
  },

  // Hosts
  host_list: { label: "List hosts" },

  // Workspaces and scripts
  workspace_create: {
    label: "Create workspace",
    context: workspaceContext(),
    detail: { inputOrder: WORKSPACE_FIELDS, outputFields: ["workspaceId", "projectId"] },
  },
  workspace_list: { label: "List workspaces" },
  workspace_rename: {
    label: "Rename workspace",
    context: field("title"),
    detail: { inputOrder: ["title", "workspaceId"] },
  },
  workspace_archive: {
    label: "Archive workspace",
    context: field("workspaceId"),
    detail: {
      inputOrder: ["workspaceId"],
      outputFields: ["workspaceId", "archivedAgentIds", "removedDirectory"],
    },
  },
  script_list: { label: "List scripts", detail: { inputOrder: ["workspaceId"] } },
  script_start: {
    label: "Start script",
    context: field("scriptName"),
    detail: { inputOrder: ["workspaceId", "scriptName"] },
  },
  script_stop: {
    label: "Stop script",
    context: field("scriptName"),
    detail: { inputOrder: ["workspaceId", "scriptName"] },
  },
  script_run_get: {
    label: "Check script run",
    context: field("scriptName", "runId"),
    detail: { inputOrder: ["workspaceId", "scriptName", "runId"] },
  },
  script_run_wait: {
    label: "Wait for script",
    context: field("runId"),
    detail: { inputOrder: ["workspaceId", "runId", "timeoutMs"] },
  },

  // Notepad
  notepad_read: {
    label: "Read notepad",
    detail: {
      inputOrder: ["workspaceId", "offset", "limit"],
      outputFields: ["workspaceId", "path", "revision", "updatedAt", "chars", "nextOffset", "text"],
    },
  },
  notepad_write: {
    label: "Write notepad",
    context: field("text"),
    detail: {
      inputOrder: ["workspaceId", "mode", "expected", "text"],
      outputFields: ["accepted", "workspaceId", "path", "revision", "updatedAt", "error"],
    },
  },

  // Terminals
  terminal_create: {
    label: "Create terminal",
    context: field("name", "command"),
    detail: { inputOrder: ["cwd", "workspaceId", "name", "command"] },
  },
  terminal_list: { label: "List terminals", detail: { inputOrder: ["cwd", "all"] } },
  terminal_capture: {
    label: "Read terminal",
    context: field("terminalId"),
    detail: { inputOrder: ["terminalId", "lines"] },
  },
  terminal_send: {
    label: "Send keys",
    context: field("keys"),
    detail: { inputOrder: ["terminalId", "keys", "literal"] },
  },
  terminal_kill: {
    label: "Kill terminal",
    context: field("terminalId"),
    detail: { inputOrder: ["terminalId"] },
  },

  // Schedules and heartbeats
  schedule_create: {
    label: "Create schedule",
    context: field("name", "cron"),
    detail: {
      promptField: "prompt",
      inputOrder: AUTOMATION_FIELDS,
      outputFields: AUTOMATION_OUTPUT,
    },
  },
  heartbeat_create: {
    label: "Create heartbeat",
    context: field("name", "cron"),
    detail: {
      promptField: "prompt",
      inputOrder: AUTOMATION_FIELDS,
      outputFields: AUTOMATION_OUTPUT,
    },
  },
  schedule_update: {
    label: "Update schedule",
    context: field("name", ...SCHEDULE_ID),
    detail: { promptField: "prompt", inputOrder: AUTOMATION_FIELDS },
  },
  schedule_list: { label: "List schedules" },
  schedule_get: {
    label: "Inspect schedule",
    context: field(...SCHEDULE_ID),
    detail: { inputOrder: SCHEDULE_ID },
  },
  schedule_log_read: {
    label: "Read schedule logs",
    context: field(...SCHEDULE_ID),
    detail: { inputOrder: [...SCHEDULE_ID, "limit"] },
  },
  schedule_run: {
    label: "Run schedule",
    context: field(...SCHEDULE_ID),
    detail: { inputOrder: SCHEDULE_ID },
  },
  schedule_pause: {
    label: "Pause schedule",
    context: field(...SCHEDULE_ID),
    detail: { inputOrder: SCHEDULE_ID },
  },
  schedule_resume: {
    label: "Resume schedule",
    context: field(...SCHEDULE_ID),
    detail: { inputOrder: SCHEDULE_ID },
  },
  schedule_delete: {
    label: "Delete schedule",
    context: field(...SCHEDULE_ID),
    detail: { inputOrder: SCHEDULE_ID },
  },
  heartbeat_list: { label: "List heartbeats" },
  heartbeat_delete: {
    label: "Delete heartbeat",
    context: field(...HEARTBEAT_ID),
    detail: { inputOrder: HEARTBEAT_ID },
  },

  // Providers
  provider_list: { label: "List providers" },
  provider_model_list: {
    label: "List models",
    context: field("provider"),
    detail: { inputOrder: ["provider"] },
  },
  profile_list: { label: "List profiles" },
  provider_get: {
    label: "Inspect provider",
    context: field("provider"),
    detail: { inputOrder: ["provider", "cwd", "settings"] },
  },

  // Browser
  browser_list_tabs: browser("List browser tabs"),
  browser_new_tab: browser("Open browser tab", field("url")),
  browser_close_tab: browser("Close browser tab", field("browserId")),
  browser_navigate: browser("Navigate", field("url")),
  browser_back: browser("Go back"),
  browser_forward: browser("Go forward"),
  browser_reload: browser("Reload page"),
  browser_snapshot: browser("Snapshot page"),
  browser_screenshot: browser("Take screenshot"),
  browser_click: browser("Click element", field("ref")),
  browser_hover: browser("Hover element", field("ref")),
  browser_drag: browser("Drag element", field("sourceRef")),
  browser_fill: browser("Fill field", field("ref")),
  browser_select: browser("Select option", field("value", "ref")),
  browser_type: browser("Type text", field("text")),
  browser_keypress: browser("Press key", field("key")),
  browser_upload: browser("Upload files", field("filePaths")),
  browser_wait: browser("Wait for page", field("text", "url")),
  browser_scroll: browser("Scroll page"),
  browser_resize: browser("Resize browser", viewportContext()),
  browser_logs: browser("Read browser logs"),
  browser_evaluate: browser("Run JavaScript", field("function")),
} satisfies Record<string, PaseoToolEntry>;

/** Former name to current name. Entries stay for as long as old transcripts can be opened. */
const FORMER_NAMES = {
  create_agent: "agent_create",
  send_agent_prompt: "agent_prompt",
  get_agent_status: "agent_get",
  get_agent_activity: "agent_activity_read",
  wait_for_agent: "agent_wait",
  list_agents: "agent_list",
  update_agent: "agent_update",
  set_agent_mode: "agent_mode_set",
  cancel_agent: "agent_cancel",
  archive_agent: "agent_archive",
  list_pending_permissions: "permission_list",
  respond_to_permission: "permission_respond",
  create_workspace: "workspace_create",
  list_workspaces: "workspace_list",
  rename_workspace: "workspace_rename",
  archive_workspace: "workspace_archive",
  list_workspace_scripts: "script_list",
  start_workspace_script: "script_start",
  stop_workspace_script: "script_stop",
  get_workspace_script_run: "script_run_get",
  wait_for_workspace_script: "script_run_wait",
  create_terminal: "terminal_create",
  list_terminals: "terminal_list",
  capture_terminal: "terminal_capture",
  send_terminal_keys: "terminal_send",
  kill_terminal: "terminal_kill",
  create_schedule: "schedule_create",
  create_heartbeat: "heartbeat_create",
  update_schedule: "schedule_update",
  list_schedules: "schedule_list",
  inspect_schedule: "schedule_get",
  schedule_logs: "schedule_log_read",
  run_schedule_once: "schedule_run",
  pause_schedule: "schedule_pause",
  resume_schedule: "schedule_resume",
  delete_schedule: "schedule_delete",
  delete_heartbeat: "heartbeat_delete",
  list_providers: "provider_list",
  list_models: "provider_model_list",
  list_profiles: "profile_list",
  inspect_provider: "provider_get",
} satisfies Record<string, keyof typeof TOOLS>;

type FormerPaseoToolName = keyof typeof FORMER_NAMES;

export const PASEO_TOOL_CATALOG = {
  ...TOOLS,
  ...(Object.fromEntries(
    Object.entries(FORMER_NAMES).map(([former, current]) => [former, TOOLS[current]]),
  ) as Record<FormerPaseoToolName, PaseoToolEntry>),
};

export type PaseoToolName = keyof typeof PASEO_TOOL_CATALOG;

export function getPaseoToolEntry(leafName: string): PaseoToolEntry | undefined {
  return Object.prototype.hasOwnProperty.call(PASEO_TOOL_CATALOG, leafName)
    ? PASEO_TOOL_CATALOG[leafName as PaseoToolName]
    : undefined;
}
