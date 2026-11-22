import { describe, expect, it } from "vitest";
import {
  buildPaseoToolDisplay,
  getPaseoToolTargetAgentId,
  MAX_SUMMARY_CHARS,
  PASEO_TOOL_NAMES,
} from "./index.js";

// Every renamed tool: [former name, current name, a call's input].
const RENAMED_TOOLS: ReadonlyArray<[string, string, Record<string, unknown>]> = [
  ["create_agent", "agent_create", { title: "Reviewer", initialPrompt: "Review it" }],
  ["send_agent_prompt", "agent_prompt", { agentId: "agt_1", prompt: "Fix the test" }],
  ["get_agent_status", "agent_get", { agentId: "agt_1" }],
  ["get_agent_activity", "agent_activity_read", { agentId: "agt_1", limit: 5 }],
  ["list_agents", "agent_list", {}],
  ["wait_for_agent", "agent_wait", { agentId: "agt_1" }],
  ["update_agent", "agent_update", { agentId: "agt_1", title: "Renamed" }],
  ["set_agent_mode", "agent_mode_set", { agentId: "agt_1", modeId: "plan" }],
  ["cancel_agent", "agent_cancel", { agentId: "agt_1" }],
  ["archive_agent", "agent_archive", { agentId: "agt_1" }],
  ["create_workspace", "workspace_create", { title: "Docs" }],
  ["list_workspaces", "workspace_list", {}],
  ["rename_workspace", "workspace_rename", { workspaceId: "wks_1", title: "Docs" }],
  ["archive_workspace", "workspace_archive", { workspaceId: "wks_1" }],
  ["list_workspace_scripts", "script_list", { workspaceId: "wks_1" }],
  ["start_workspace_script", "script_start", { scriptName: "web" }],
  ["stop_workspace_script", "script_stop", { scriptName: "web" }],
  ["get_workspace_script_run", "script_run_get", { runId: "run_1" }],
  ["wait_for_workspace_script", "script_run_wait", { runId: "run_1" }],
  ["list_terminals", "terminal_list", {}],
  ["create_terminal", "terminal_create", { name: "dev" }],
  ["kill_terminal", "terminal_kill", { terminalId: "t1" }],
  ["capture_terminal", "terminal_capture", { terminalId: "t1" }],
  ["send_terminal_keys", "terminal_send", { terminalId: "t1", keys: "npm test" }],
  ["create_schedule", "schedule_create", { name: "Nightly", cron: "0 9 * * 1" }],
  ["list_schedules", "schedule_list", {}],
  ["inspect_schedule", "schedule_get", { scheduleId: "sch_1" }],
  ["update_schedule", "schedule_update", { scheduleId: "sch_1", modeId: "plan" }],
  ["pause_schedule", "schedule_pause", { scheduleId: "sch_1" }],
  ["resume_schedule", "schedule_resume", { scheduleId: "sch_1" }],
  ["run_schedule_once", "schedule_run", { scheduleId: "sch_1" }],
  ["schedule_logs", "schedule_log_read", { scheduleId: "sch_1" }],
  ["delete_schedule", "schedule_delete", { scheduleId: "sch_1" }],
  ["create_heartbeat", "heartbeat_create", { cron: "*/5 * * * *" }],
  ["delete_heartbeat", "heartbeat_delete", { heartbeatId: "hb_1" }],
  ["list_providers", "provider_list", {}],
  ["list_models", "provider_model_list", { provider: "codex" }],
  ["inspect_provider", "provider_get", { provider: "codex" }],
  ["list_profiles", "profile_list", {}],
  ["list_pending_permissions", "permission_list", {}],
  ["respond_to_permission", "permission_respond", { agentId: "agt_1", requestId: "req_1" }],
];

function display(toolName: string, input: unknown, targetAgentTitle?: string) {
  return buildPaseoToolDisplay({
    toolName,
    detail: { type: "unknown", input, output: null },
    ...(targetAgentTitle ? { targetAgentTitle } : {}),
  });
}

describe("Paseo tool display", () => {
  it.each([
    [
      "send_agent_prompt",
      { agentId: "agt_1", prompt: "Fix the failing test" },
      "Send message",
      "Fix the failing test",
    ],
    ["create_agent", { title: "Reviewer", initialPrompt: "Review it" }, "Create agent", "Reviewer"],
    ["create_agent", { initialPrompt: "Review it" }, "Create agent", "Review it"],
    ["get_agent_status", { agentId: "agt_1" }, "Check agent", "agt_1"],
    ["wait_for_agent", { agentId: "agt_1", timeoutMs: 5000 }, "Wait for agent", "agt_1"],
    ["update_agent", { agentId: "agt_1", name: "Renamed" }, "Update agent", "Renamed"],
    ["create_workspace", { path: "/tmp/repo/feature-x" }, "Create workspace", "feature-x"],
    ["create_workspace", { title: "Docs", branchName: "docs" }, "Create workspace", "Docs"],
    ["start_workspace_script", { scriptName: "web" }, "Start script", "web"],
    ["send_terminal_keys", { terminalId: "t1", keys: "npm test" }, "Send keys", "npm test"],
    ["create_schedule", { prompt: "Check CI", cron: "0 9 * * 1" }, "Create schedule", "0 9 * * 1"],
    ["browser_navigate", { url: "https://paseo.sh" }, "Navigate", "https://paseo.sh"],
    [
      "browser_upload",
      { ref: "@e1", filePaths: ["/a.png", "/b.png"] },
      "Upload files",
      "/a.png +1",
    ],
    ["browser_resize", { width: 800, height: 600 }, "Resize browser", "800×600"],
  ])("presents %s with a fixed label and its context", (tool, input, label, summary) => {
    expect(display(`mcp__ait__${tool}`, input)).toEqual({ displayName: label, summary });
  });

  it.each(RENAMED_TOOLS)("presents %s and %s the same way", (former, current, input) => {
    expect(PASEO_TOOL_NAMES).toEqual(expect.arrayContaining([former, current]));
    expect(display(`mcp__ait__${current}`, input)).toEqual(display(`mcp__ait__${former}`, input));
  });

  it.each([
    ["agent_update", { agentId: "agt_1", title: "Renamed" }, "Renamed"],
    ["agent_update", { agentId: "agt_1", name: "Renamed" }, "Renamed"],
    ["update_agent", { agentId: "agt_1", title: "Renamed" }, "Renamed"],
    ["agent_update", { agentId: "agt_1", labels: { team: "docs" } }, "agt_1"],
    ["schedule_get", { scheduleId: "sch_1" }, "sch_1"],
    ["schedule_get", { id: "sch_1" }, "sch_1"],
    ["inspect_schedule", { id: "sch_1" }, "sch_1"],
    ["inspect_schedule", { scheduleId: "sch_1" }, "sch_1"],
    ["schedule_update", { scheduleId: "sch_1", modeId: "plan" }, "sch_1"],
    ["update_schedule", { id: "sch_1", mode: "plan" }, "sch_1"],
    ["heartbeat_delete", { heartbeatId: "hb_1" }, "hb_1"],
    ["heartbeat_delete", { id: "hb_1" }, "hb_1"],
    ["delete_heartbeat", { id: "hb_1" }, "hb_1"],
  ])("reads the renamed input field of %s under either spelling", (tool, input, summary) => {
    expect(display(`mcp__ait__${tool}`, input)?.summary).toBe(summary);
  });

  it("keeps the label of the retired kill_agent, which agent_archive replaces", () => {
    expect(display("mcp__ait__kill_agent", { agentId: "agt_1" })?.displayName).toBe("Kill agent");
    expect(display("mcp__ait__agent_archive", { agentId: "agt_1" })?.displayName).toBe(
      "Archive agent",
    );
  });

  it.each([
    ["heartbeat_list", {}, "List heartbeats", undefined],
    ["notepad_read", { workspaceId: "wks_1", offset: 0 }, "Read notepad", undefined],
    [
      "notepad_write",
      { text: "Who owns the schema?", mode: "append" },
      "Write notepad",
      "Who owns the schema?",
    ],
  ])("presents the new tool %s", (tool, input, label, summary) => {
    expect(display(`mcp__ait__${tool}`, input)).toEqual({
      displayName: label,
      ...(summary ? { summary } : {}),
    });
  });

  it("omits the summary when a call has no context", () => {
    expect(display("mcp__ait__list_agents", {})).toEqual({ displayName: "List agents" });
  });

  it("recognizes every ait namespace format", () => {
    for (const name of [
      "mcp__ait__kill_agent",
      "mcp__ait_remote__kill_agent",
      "ait.kill_agent",
      "ait_remote.kill_agent",
    ]) {
      expect(display(name, { agentId: "agt_1" })?.displayName).toBe("Kill agent");
    }
  });

  it("no longer treats the retired paseo namespace as an Ait tool", () => {
    for (const name of ["mcp__paseo__kill_agent", "paseo.kill_agent", "paseo_remote.kill_agent"]) {
      expect(display(name, { agentId: "agt_1" })).toBeNull();
    }
  });

  it("returns null outside the Paseo namespace, including speak", () => {
    expect(display("Bash", {})).toBeNull();
    expect(display("mcp__ait__speak", { text: "hi" })).toBeNull();
    expect(display("mcp__paseo__speak", { text: "hi" })).toBeNull();
    expect(display("mcp__aitx__kill_agent", {})).toBeNull();
  });

  it("humanizes a Paseo tool that has no catalog entry", () => {
    expect(display("mcp__ait__brand_new_tool", {})).toEqual({ displayName: "Brand new tool" });
  });

  it("prefers the known agent title over the agent ID", () => {
    expect(display("mcp__ait__cancel_agent", { agentId: "agt_1" }, "Reviewer")).toEqual({
      displayName: "Cancel agent",
      summary: "Reviewer",
    });
    expect(
      getPaseoToolTargetAgentId("mcp__ait__cancel_agent", {
        type: "unknown",
        input: { agentId: "agt_1" },
        output: null,
      }),
    ).toBe("agt_1");
    expect(
      getPaseoToolTargetAgentId("Bash", { type: "unknown", input: { agentId: "x" }, output: null }),
    ).toBeNull();
  });

  it("truncates a massive single-line prompt and collapses whitespace", () => {
    const prompt = `Line one\n\n   line two ${"x".repeat(1_000_000)}`;
    const summary = display("mcp__ait__send_agent_prompt", { agentId: "a", prompt })?.summary;
    expect(summary).toHaveLength(MAX_SUMMARY_CHARS);
    expect(summary?.startsWith("Line one line two xxx")).toBe(true);
    expect(summary?.endsWith("…")).toBe(true);
  });

  it("bounds the scan even when the prompt opens with a wall of whitespace", () => {
    const prompt = `${" ".repeat(10_000)}hidden`;
    const summary = display("mcp__ait__send_agent_prompt", { agentId: "a", prompt })?.summary;
    expect(summary).toBeUndefined();
  });

  it("keeps a short prompt intact", () => {
    expect(
      display("mcp__ait__send_agent_prompt", { agentId: "a", prompt: " hi\nthere " })?.summary,
    ).toBe("hi there");
  });

  it("catalogs tool names as lowercase leaf names", () => {
    for (const name of PASEO_TOOL_NAMES) {
      expect(name).toMatch(/^[a-z_]+$/);
    }
  });
});
