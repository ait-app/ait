import type { ToolCallDetail } from "@ait/protocol/agent-types";
import { describe, expect, it } from "vitest";

import { buildToolCallPresentation, type ToolCallPresentationIcon } from "./presentation";

const fakeIcons = {
  brain: (() => null) as ToolCallPresentationIcon,
  eye: (() => null) as ToolCallPresentationIcon,
  wrench: (() => null) as ToolCallPresentationIcon,
};

function fakeResolveIcon(
  toolName: string,
  detail: ToolCallDetail | undefined,
): ToolCallPresentationIcon {
  if (detail?.type === "plan") {
    return fakeIcons.brain;
  }
  if (detail?.type === "read") {
    return fakeIcons.eye;
  }
  if (toolName === "exec_command") {
    return fakeIcons.wrench;
  }
  return fakeIcons.wrench;
}

describe("tool-call presentation", () => {
  const emptyDetails: { label: string; detail?: ToolCallDetail; toolName?: string }[] = [
    { label: "missing detail" },
    { label: "empty shell", detail: { type: "shell", command: "", output: "" } },
    { label: "blank shell", detail: { type: "shell", command: " \n", output: "\n" } },
    { label: "read path only", detail: { type: "read", filePath: "src/index.ts" } },
    { label: "empty read", detail: { type: "read", filePath: "src/index.ts", content: "" } },
    { label: "blank read", detail: { type: "read", filePath: "src/index.ts", content: " \n" } },
    { label: "write path only", detail: { type: "write", filePath: "src/index.ts" } },
    { label: "edit path only", detail: { type: "edit", filePath: "src/index.ts" } },
    {
      label: "diff file headers only",
      detail: {
        type: "edit",
        filePath: "src/index.ts",
        unifiedDiff: "--- a/index.ts\n+++ b/index.ts\n",
      },
    },
    { label: "search query only", detail: { type: "search", query: "needle" } },
    {
      label: "empty search results",
      detail: {
        type: "search",
        query: "needle",
        content: " ",
        filePaths: [""],
        webResults: [{ title: "", url: " " }],
        annotations: ["\n"],
        numMatches: 0,
      },
    },
    { label: "fetch status only", detail: { type: "fetch", url: "", codeText: "OK" } },
    {
      label: "empty worktree setup",
      detail: { type: "worktree_setup", branchName: "", worktreePath: "", log: "", commands: [] },
    },
    { label: "empty sub-agent", detail: { type: "sub_agent", log: "" } },
    { label: "text label only", detail: { type: "plain_text", label: "Terminal" } },
    { label: "empty plan", detail: { type: "plan", text: " \n" } },
    { label: "empty unknown", detail: { type: "unknown", input: {}, output: null } },
    {
      label: "nested empty unknown",
      detail: { type: "unknown", input: { values: [null, " ", {}] }, output: [] },
    },
    {
      label: "Paseo result with no rendered fields",
      toolName: "paseo.create_agent",
      detail: { type: "unknown", input: {}, output: { structuredContent: { ignored: "value" } } },
    },
  ];

  it.each(emptyDetails)("does not expand $label in any lifecycle state", ({ detail, toolName }) => {
    const statuses = ["executing", "running", "completed", "failed", "canceled"] as const;
    for (const status of statuses) {
      const presentation = buildToolCallPresentation({
        toolName: toolName ?? "tool",
        status,
        error: null,
        detail,
        resolveIcon: fakeResolveIcon,
      });
      expect(presentation.canOpenDetails).toBe(false);
      expect(presentation.hasDetails).toBe(false);
    }
  });

  const visibleDetails: ToolCallDetail[] = [
    { type: "shell", command: "pwd" },
    { type: "shell", command: "", output: "done" },
    { type: "read", filePath: "src/index.ts", content: "0" },
    { type: "write", filePath: "src/index.ts", content: "hello" },
    { type: "edit", filePath: "src/index.ts", oldString: "old", newString: "" },
    { type: "edit", filePath: "src/index.ts", oldString: "", newString: "\n" },
    { type: "edit", filePath: "src/index.ts", unifiedDiff: "@@ -1 +1 @@\n-old\n+new" },
    { type: "search", query: "needle", content: "0 matches" },
    { type: "search", query: "needle", filePaths: ["src/index.ts"] },
    {
      type: "search",
      query: "needle",
      webResults: [{ title: "Result", url: "https://example.com" }],
    },
    { type: "search", query: "needle", annotations: ["No matches"] },
    { type: "fetch", url: "https://example.com" },
    { type: "fetch", url: "", result: "Result" },
    { type: "worktree_setup", branchName: "", worktreePath: "", log: "Ready", commands: [] },
    { type: "sub_agent", log: "[Read] README.md" },
    { type: "sub_agent", log: "", childSessionId: "child-1" },
    { type: "plain_text", text: "hello" },
    { type: "unknown", input: null, output: 0 },
    { type: "unknown", input: null, output: false },
  ];

  it.each(visibleDetails)("keeps rendered $type content inspectable: %j", (detail) => {
    const presentation = buildToolCallPresentation({
      toolName: "tool",
      status: "completed",
      error: null,
      detail,
      resolveIcon: fakeResolveIcon,
    });
    expect(presentation.canOpenDetails).toBe(true);
  });

  it("only offers error details when a nonempty error is rendered", () => {
    for (const error of [null, "", " \n", { content: "" }]) {
      expect(
        buildToolCallPresentation({
          toolName: "tool",
          status: "failed",
          error,
          resolveIcon: fakeResolveIcon,
        }).canOpenDetails,
      ).toBe(false);
    }
    const failed = buildToolCallPresentation({
      toolName: "tool",
      status: "failed",
      error: { message: "Permission denied" },
      resolveIcon: fakeResolveIcon,
    });
    expect(failed.canOpenDetails).toBe(true);
    expect(failed.errorText).toContain("Permission denied");
    expect(
      buildToolCallPresentation({
        toolName: "tool",
        status: "completed",
        error: "not rendered",
        resolveIcon: fakeResolveIcon,
      }).canOpenDetails,
    ).toBe(false);
  });

  it("updates expansion availability as streaming content arrives and disappears", () => {
    const frames = [
      { status: "running", content: "" },
      { status: "running", content: "file contents" },
      { status: "completed", content: "" },
    ] as const;
    const views = frames.map(({ status, content }) =>
      buildToolCallPresentation({
        toolName: "read_file",
        status,
        error: null,
        cwd: "/repo",
        detail: { type: "read", filePath: "/repo/src/index.ts", content },
        resolveIcon: fakeResolveIcon,
      }),
    );
    expect(views.map((view) => view.canOpenDetails)).toEqual([false, true, false]);
    for (const view of views) {
      expect(view.summary).toBe("src/index.ts");
      expect(view.openFilePath).toBe("/repo/src/index.ts");
    }
  });

  it("builds badge, detail, icon, and file-open policy in one model", () => {
    const presentation = buildToolCallPresentation({
      toolName: "read_file",
      status: "completed",
      error: null,
      cwd: "/tmp/repo",
      detail: {
        type: "read",
        filePath: "/tmp/repo/src/index.ts",
        content: "console.log('hi');",
      },
      resolveIcon: fakeResolveIcon,
    });

    expect(presentation).toMatchObject({
      displayName: "Read",
      summary: "src/index.ts",
      icon: fakeIcons.eye,
      isLoadingDetails: false,
      hasDetails: true,
      canOpenDetails: true,
      openFilePath: "/tmp/repo/src/index.ts",
      isPlan: false,
    });
  });

  it("keeps running calls without content non-expandable while loading", () => {
    const presentation = buildToolCallPresentation({
      toolName: "exec_command",
      status: "running",
      error: null,
      detail: {
        type: "unknown",
        input: {},
        output: null,
      },
      resolveIcon: fakeResolveIcon,
    });

    expect(presentation).toMatchObject({
      displayName: "Exec command",
      icon: fakeIcons.wrench,
      isLoadingDetails: true,
      hasDetails: false,
      canOpenDetails: false,
      openFilePath: null,
      isPlan: false,
    });
  });

  it("keeps plan calls out of the expandable badge path", () => {
    const presentation = buildToolCallPresentation({
      toolName: "ExitPlanMode",
      status: "completed",
      error: null,
      detail: {
        type: "plan",
        text: "1. Do the thing",
      },
      resolveIcon: fakeResolveIcon,
    });

    expect(presentation.isPlan).toBe(true);
    expect(presentation.icon).toBe(fakeIcons.brain);
  });
});
