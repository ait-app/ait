import { describe, expect, it } from "vitest";
import { buildPaseoToolDetailSections } from "./index.js";

describe("Paseo tool-call detail presentation", () => {
  it.each([
    "mcp__ait__create_agent",
    "ait.create_agent",
    "ait_remote.create_agent",
    "mcp__ait__agent_create",
    "ait.agent_create",
  ])("shares one create-agent mapping for %s", (toolName) => {
    expect(
      buildPaseoToolDetailSections(
        toolName,
        {
          workspaceId: "wks_123",
          provider: "codex/gpt-5.4",
          title: "Greeter",
          initialPrompt: "Say hello back.\nDo nothing else.",
          notifyOnFinish: true,
        },
        { agentId: "agt_123", status: "idle" },
      ),
    ).toEqual([
      {
        kind: "prose",
        title: "Prompt",
        text: "Say hello back.\nDo nothing else.",
      },
      {
        kind: "fields",
        title: "Details",
        fields: [
          { label: "Title", value: "Greeter" },
          { label: "Provider", value: "codex/gpt-5.4" },
          { label: "Workspace", value: "wks_123" },
          { label: "Notify on finish", value: "Yes" },
        ],
      },
      {
        kind: "fields",
        title: "Result",
        fields: [
          { label: "Agent", value: "agt_123" },
          { label: "Status", value: "idle" },
        ],
      },
    ]);
  });

  it("formats schedule cadence and nested settings without JSON syntax", () => {
    const sections = buildPaseoToolDetailSections(
      "mcp__ait__create_schedule",
      {
        prompt: "Say hello back.",
        cron: "0 9 * * 1",
        timezone: "Europe/Berlin",
        provider: "codex/gpt-5.4",
        maxRuns: 1,
      },
      {
        id: "sch_123",
        status: "active",
        nextRunAt: "2026-09-07T09:00:00.000Z",
        target: { type: "new-agent", mode: "read-only" },
      },
    );

    expect(sections?.slice(0, 2)).toMatchObject([
      { kind: "prose", title: "Prompt", text: "Say hello back." },
      {
        kind: "fields",
        title: "Details",
        fields: [
          { label: "Cron", value: "0 9 * * 1" },
          { label: "Timezone", value: "Europe/Berlin" },
          { label: "Provider", value: "codex/gpt-5.4" },
          { label: "Maximum runs", value: "1" },
        ],
      },
    ]);
    expect(JSON.stringify(sections)).not.toContain('\\"new-agent\\"');
    expect(sections?.at(-1)).toEqual({
      kind: "fields",
      title: "Result",
      fields: [
        { label: "ID", value: "sch_123" },
        { label: "Status", value: "active" },
        { label: "Next run", value: "2026-09-07T09:00:00.000Z" },
      ],
    });
  });

  it.each([
    ["mcp__ait__update_agent", { agentId: "agt_1", name: "Renamed" }, "Name"],
    ["mcp__ait__agent_update", { agentId: "agt_1", title: "Renamed" }, "Title"],
  ])("leads %s with the agent's new title under either field name", (toolName, input, label) => {
    expect(buildPaseoToolDetailSections(toolName, input, null)).toEqual([
      {
        kind: "fields",
        title: "Details",
        fields: [
          { label, value: "Renamed" },
          { label: "Agent", value: "agt_1" },
        ],
      },
    ]);
  });

  it.each([
    ["mcp__ait__update_schedule", { id: "sch_1", mode: "plan", cron: "0 9 * * 1" }, "ID"],
    [
      "mcp__ait__schedule_update",
      { scheduleId: "sch_1", modeId: "plan", cron: "0 9 * * 1" },
      "Schedule",
    ],
  ])("lays out %s the same with old and new field names", (toolName, input, idLabel) => {
    expect(buildPaseoToolDetailSections(toolName, input, null)).toEqual([
      {
        kind: "fields",
        title: "Details",
        fields: [
          { label: idLabel, value: "sch_1" },
          { label: "Cron", value: "0 9 * * 1" },
          { label: "Mode", value: "plan" },
        ],
      },
    ]);
  });

  it.each([
    ["mcp__ait__delete_heartbeat", { id: "hb_1" }, "ID"],
    ["mcp__ait__heartbeat_delete", { heartbeatId: "hb_1" }, "Heartbeat"],
  ])("names the heartbeat that %s removes", (toolName, input, label) => {
    expect(buildPaseoToolDetailSections(toolName, input, null)).toEqual([
      { kind: "fields", title: "Details", fields: [{ label, value: "hb_1" }] },
    ]);
  });

  it("lays out a notepad write and reports the revision that conflicts", () => {
    expect(
      buildPaseoToolDetailSections(
        "mcp__ait__notepad_write",
        {
          text: "Use v2 of the API.",
          expected: "9f86d081884c7d65",
          workspaceId: "wks_1",
          mode: "replace",
        },
        {
          accepted: false,
          workspaceId: "wks_1",
          path: "/home/me/.ait/notepads/wks_1/notepad.md",
          revision: "2c26b46b68ffc68f",
          updatedAt: "2026-11-22T09:00:00.000Z",
          error: "The notepad changed. Read it again before replacing it.",
        },
      ),
    ).toEqual([
      {
        kind: "fields",
        title: "Details",
        fields: [
          { label: "Workspace", value: "wks_1" },
          { label: "Mode", value: "replace" },
          { label: "Expected", value: "9f86d081884c7d65" },
          { label: "Text", value: "Use v2 of the API." },
        ],
      },
      {
        kind: "fields",
        title: "Result",
        fields: [
          { label: "Accepted", value: "No" },
          { label: "Workspace", value: "wks_1" },
          { label: "Path", value: "/home/me/.ait/notepads/wks_1/notepad.md" },
          { label: "Revision", value: "2c26b46b68ffc68f" },
          { label: "Updated at", value: "2026-11-22T09:00:00.000Z" },
          { label: "Error", value: "The notepad changed. Read it again before replacing it." },
        ],
      },
    ]);
  });

  it("shows what a notepad read returned, text last", () => {
    expect(
      buildPaseoToolDetailSections(
        "mcp__ait__notepad_read",
        { limit: 2000 },
        {
          text: "Use v2.",
          chars: 7,
          revision: "2c26b46b68ffc68f",
          workspaceId: "wks_1",
          path: "/home/me/.ait/notepads/wks_1/notepad.md",
          nextOffset: null,
        },
      )?.at(-1),
    ).toEqual({
      kind: "fields",
      title: "Result",
      fields: [
        { label: "Workspace", value: "wks_1" },
        { label: "Path", value: "/home/me/.ait/notepads/wks_1/notepad.md" },
        { label: "Revision", value: "2c26b46b68ffc68f" },
        { label: "Chars", value: "7" },
        { label: "Next offset", value: "None" },
        { label: "Text", value: "Use v2." },
      ],
    });
  });

  it("unwraps MCP result envelopes instead of exposing JSON-encoded text", () => {
    expect(
      buildPaseoToolDetailSections(
        "mcp__ait__send_agent_prompt",
        { prompt: "Say hello back." },
        {
          meta: null,
          content: [
            {
              type: "text",
              text: '{"success":true,"status":"idle","lastMessage":"Hello back."}',
            },
          ],
          structuredContent: {
            success: true,
            status: "idle",
            lastMessage: "Hello back.",
          },
        },
      )?.at(-1),
    ).toEqual({
      kind: "fields",
      title: "Result",
      fields: [
        { label: "Status", value: "idle" },
        { label: "Last message", value: "Hello back." },
      ],
    });
  });

  it("uses readable fallback fields for newly added Paseo tools", () => {
    expect(
      buildPaseoToolDetailSections(
        "mcp__ait__future_tool",
        { opaqueThing: ["one", "two"], enabled: false },
        { success: true },
      ),
    ).toEqual([
      {
        kind: "fields",
        title: "Details",
        fields: [
          { label: "Enabled", value: "No" },
          { label: "Opaque thing", value: "• one\n• two" },
        ],
      },
      {
        kind: "fields",
        title: "Result",
        fields: [{ label: "Success", value: "Yes" }],
      },
    ]);
  });

  it("leaves non-Paseo tools alone", () => {
    expect(buildPaseoToolDetailSections("mcp__github__create_issue", {}, {})).toBeNull();
  });
});
