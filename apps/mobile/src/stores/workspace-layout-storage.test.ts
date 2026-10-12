import { describe, expect, it } from "vitest";
import { WorkspaceLayoutPersistedStateSchema } from "./workspace-layout-storage";

describe("Bot layout persistence", () => {
  it.each([
    { kind: "bot", view: "list" },
    { kind: "bot", view: "create" },
    { kind: "bot", botId: "bot-a" },
    { kind: "bot", botId: "bot-a", view: "settings" },
  ])("restores %j alongside existing tabs", (target) => {
    const saved = {
      layoutByWorkspace: {
        "server:workspace": {
          focusedPaneId: "main",
          root: {
            kind: "pane",
            pane: {
              id: "main",
              tabIds: ["agent-tab", "bot-tab"],
              focusedTabId: "bot-tab",
              tabs: [
                { tabId: "agent-tab", target: { kind: "agent", agentId: "agent-a" }, createdAt: 1 },
                { tabId: "bot-tab", target, createdAt: 2 },
              ],
            },
          },
        },
      },
    };
    expect(WorkspaceLayoutPersistedStateSchema.parse(JSON.parse(JSON.stringify(saved)))).toEqual(
      saved,
    );
  });
});
