// @vitest-environment jsdom
import React from "react";
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ToolCallDetail } from "@ait/protocol/agent-types";
import { i18n } from "@/i18n/i18next";
import { ToolCallDetailsContent } from "./tool-call-details";

const worktreeSetup: ToolCallDetail = {
  type: "worktree_setup",
  worktreePath: "/tmp/wt",
  branchName: "feature-x",
  log: "",
  commands: [],
};

const subAgent: ToolCallDetail = {
  type: "sub_agent",
  childSessionId: "child-1",
  log: "",
};

async function renderInLanguage(language: string, detail: ToolCallDetail) {
  await i18n.changeLanguage(language);
  render(<ToolCallDetailsContent toolName="tool" detail={detail} />);
}

describe("ToolCallDetailsContent labels", () => {
  beforeEach(() => vi.stubGlobal("React", React));
  afterEach(async () => {
    cleanup();
    vi.unstubAllGlobals();
    await i18n.changeLanguage("en");
  });

  it("describes a worktree setup with no log yet in the active app language", async () => {
    await renderInLanguage("en", worktreeSetup);
    expect(screen.getByText("Preparing worktree feature-x at /tmp/wt")).toBeTruthy();
    cleanup();

    await renderInLanguage("es", worktreeSetup);
    expect(screen.getByText("Preparando el worktree feature-x en /tmp/wt")).toBeTruthy();
  });

  it("names the sub-agent session in the active app language", async () => {
    await renderInLanguage("en", subAgent);
    expect(screen.getByText("session child-1")).toBeTruthy();
    cleanup();

    await renderInLanguage("es", subAgent);
    expect(screen.getByText("sesión child-1")).toBeTruthy();
  });

  it("lists an Ait agent call's fields in the catalog order", () => {
    render(
      <ToolCallDetailsContent
        toolName="mcp__ait__create_agent"
        detail={{
          type: "unknown",
          input: {
            title: "Greeter",
            settings: { approvals: "never" },
            internal: true,
            notifyOnFinish: true,
          },
          output: null,
        }}
      />,
    );
    const labels = ["Title", "Notify on finish", "Internal", "Settings"].map((label) =>
      screen.getByText(label),
    );
    const renderedOrder = [...labels]
      .sort((left, right) =>
        left.compareDocumentPosition(right) & Node.DOCUMENT_POSITION_FOLLOWING ? -1 : 1,
      )
      .map((element) => element.textContent);
    expect(renderedOrder).toEqual(["Title", "Notify on finish", "Internal", "Settings"]);
  });

  it("renders MiniMax ask-user questionnaires as readable steps instead of JSON", () => {
    render(
      <ToolCallDetailsContent
        toolName="ask_user"
        detail={{
          type: "unknown",
          input: {
            steps: [
              {
                header: "Release 正文",
                question: "是否同步 Release 正文？",
                selectionMode: "single",
                options: [
                  {
                    label: "更新正文",
                    description: "补充稳定版章节",
                    recommended: true,
                  },
                  { label: "保留原文" },
                ],
              },
            ],
          },
          output: {
            content: [
              {
                type: "text",
                text: "Questionnaire ask_123 is waiting for the local user.",
              },
            ],
            details: { waiting_for_user: true },
          },
        }}
      />,
    );

    expect(screen.getByTestId("questionnaire-detail")).toBeTruthy();
    expect(screen.getByText("Release 正文")).toBeTruthy();
    expect(screen.getByText("是否同步 Release 正文？")).toBeTruthy();
    expect(screen.getByText("更新正文")).toBeTruthy();
    expect(screen.getByText("补充稳定版章节")).toBeTruthy();
    expect(screen.getByText("保留原文")).toBeTruthy();
    expect(screen.getByText("Waiting for input")).toBeTruthy();
    expect(screen.queryByText(/Questionnaire ask_123/)).toBeNull();
  });

  it("keeps non-questionnaire unknown tools on the raw detail renderer", () => {
    render(
      <ToolCallDetailsContent
        toolName="custom_tool"
        detail={{
          type: "unknown",
          input: { steps: [{ question: "Opaque", options: [] }] },
          output: null,
        }}
      />,
    );

    expect(screen.queryByTestId("questionnaire-detail")).toBeNull();
    expect(screen.getByText(/"steps"/)).toBeTruthy();
  });
});
