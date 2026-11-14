import { describe, expect, it } from "vitest";
import {
  buildPaseoToolDisplay,
  getPaseoToolTargetAgentId,
  MAX_SUMMARY_CHARS,
  PASEO_TOOL_NAMES,
} from "./index.js";

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
