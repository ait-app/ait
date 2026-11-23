import { describe, expect, it } from "vitest";
import { QueryClient } from "@tanstack/react-query";
import {
  agentCommandRevisionKey,
  invalidateAgentCommands,
  agentCommandsQueryKey,
  agentCommandsQueryRoot,
  draftAgentCommandsQueryKey,
  normalizeAgentCommandsCwd,
  sessionAgentCommandsQueryKey,
} from "./agent-commands-query";

describe("agent command query keys", () => {
  it("invalidates only the changed session and advances its in-flight safety revision", () => {
    const client = new QueryClient();
    const affected = [
      ...sessionAgentCommandsQueryKey({ serverId: "server-1", agentId: "agent-1" }),
      "connection",
      1,
    ];
    const unrelated = [
      ...sessionAgentCommandsQueryKey({ serverId: "server-1", agentId: "agent-2" }),
      "connection",
      1,
    ];
    client.setQueryData(affected, ["old"]);
    client.setQueryData(unrelated, ["keep"]);
    invalidateAgentCommands(client, "server-1", "agent-1");
    expect(client.getQueryData(agentCommandRevisionKey("server-1", "agent-1"))).toBe(1);
    expect(client.getQueryState(affected)?.isInvalidated).toBe(true);
    expect(client.getQueryState(unrelated)?.isInvalidated).toBe(false);
  });
  it("shares draft commands across draft tabs with the same provider setup", () => {
    const draftConfig = {
      provider: "codex" as const,
      cwd: "/repo",
      modeId: "auto",
      model: "gpt-5.5",
      thinkingOptionId: "medium",
    };

    expect(
      agentCommandsQueryKey({ serverId: "server-1", agentId: "draft-a", draftConfig }),
    ).toEqual(agentCommandsQueryKey({ serverId: "server-1", agentId: "draft-b", draftConfig }));
  });

  it("keeps draft commands separate from running agent session commands", () => {
    expect(sessionAgentCommandsQueryKey({ serverId: "server-1", agentId: "agent-1" })).toEqual([
      "agentCommands",
      "server-1",
      "session",
      "agent-1",
    ]);
    expect(
      draftAgentCommandsQueryKey({
        serverId: "server-1",
        draftConfig: { provider: "codex", cwd: "/repo" },
      }),
    ).toEqual([
      "agentCommands",
      "server-1",
      "draft",
      "codex",
      "cwd",
      "/repo",
      "mode",
      null,
      "model",
      null,
      "thinking",
      null,
      "features",
      null,
    ]);
  });

  it("normalizes cwd values so equivalent workspace paths share one draft scope", () => {
    expect(normalizeAgentCommandsCwd("C:\\Users\\Ezekiel Bulver\\project")).toBe(
      "C:/Users/Ezekiel Bulver/project",
    );
    expect(
      draftAgentCommandsQueryKey({
        serverId: "server-1",
        draftConfig: { provider: "codex", cwd: "C:\\Users\\Ezekiel Bulver\\project" },
      }),
    ).toEqual(
      draftAgentCommandsQueryKey({
        serverId: "server-1",
        draftConfig: { provider: "codex", cwd: "C:/Users/Ezekiel Bulver/project" },
      }),
    );
  });

  it("exposes a server-level root for refresh invalidation", () => {
    expect(agentCommandsQueryRoot("server-1")).toEqual(["agentCommands", "server-1"]);
  });
});
