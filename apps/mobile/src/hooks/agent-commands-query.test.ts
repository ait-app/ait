import { describe, expect, it } from "vitest";
import {
  agentCommandsQueryKey,
  draftAgentCommandsQueryKey,
  normalizeAgentCommandsCwd,
  sessionAgentCommandsQueryKey,
} from "./agent-commands-query";

describe("agent command query keys", () => {
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
    expect(sessionAgentCommandsQueryKey({ serverId: "server-1", agentId: "agent-1" })).not.toEqual(
      draftAgentCommandsQueryKey({
        serverId: "server-1",
        draftConfig: { provider: "codex", cwd: "/repo" },
      }),
    );
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

  it("does not reuse ownership across a provider, cwd, or native session switch", () => {
    const context = { provider: "omp", cwd: "/repo", nativeSessionId: "native-1" };
    const key = sessionAgentCommandsQueryKey({ serverId: "server-1", agentId: "agent-1", context });
    for (const changed of [
      { ...context, provider: "codex" },
      { ...context, cwd: "/other" },
      { ...context, nativeSessionId: "native-2" },
    ]) {
      expect(
        sessionAgentCommandsQueryKey({
          serverId: "server-1",
          agentId: "agent-1",
          context: changed,
        }),
      ).not.toEqual(key);
    }
  });
});
