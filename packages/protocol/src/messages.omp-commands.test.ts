import { describe, expect, it } from "vitest";
import {
  CompleteCommandsRequestSchema,
  CompleteCommandsResponseSchema,
  ListCommandsResponseSchema,
} from "./messages.js";

describe("command provenance wire compatibility", () => {
  it("keeps old catalog entries without provenance readable", () => {
    const parsed = ListCommandsResponseSchema.parse({
      type: "agent.commands.list.response",
      payload: {
        agentId: "agent-1",
        commands: [{ name: "review", description: "Review", argumentHint: "" }],
        error: null,
        requestId: "req-1",
      },
    });
    expect(parsed.payload.commands[0]?.source).toBeUndefined();
    expect(parsed.payload.commands[0]?.aliases).toBeUndefined();
  });

  it("carries real native provenance, aliases and metadata hints when present", () => {
    const parsed = ListCommandsResponseSchema.parse({
      type: "agent.commands.list.response",
      payload: {
        agentId: "agent-1",
        commands: [
          {
            name: "deploy",
            description: "Deploy",
            argumentHint: "[env]",
            kind: "command",
            source: "plugin",
            aliases: ["deploy:"],
          },
        ],
        error: null,
        requestId: "req-1",
      },
    });
    expect(parsed.payload.commands[0]?.source).toBe("plugin");
    expect(parsed.payload.commands[0]?.aliases).toEqual(["deploy:"]);
  });
});

describe("native command completion wire", () => {
  it("validates UTF-16 editor state requests", () => {
    const parsed = CompleteCommandsRequestSchema.parse({
      type: "agent.commands.complete.request",
      agentId: "draft",
      draftConfig: { provider: "omp", cwd: "/tmp/project" },
      text: "/revi",
      cursor: 5,
      requestId: "req-2",
    });
    expect(parsed.cursor).toBe(5);
    expect(() =>
      CompleteCommandsRequestSchema.parse({
        type: "agent.commands.complete.request",
        agentId: "draft",
        text: "/revi",
        cursor: -1,
        requestId: "req-2",
      }),
    ).toThrow();
  });

  it("keeps unsupported downgrades explicit and bounded", () => {
    const parsed = CompleteCommandsResponseSchema.parse({
      type: "agent.commands.complete.response",
      payload: { agentId: "draft", supported: false, items: [], requestId: "req-2" },
    });
    expect(parsed.payload.supported).toBe(false);
    const supported = CompleteCommandsResponseSchema.parse({
      type: "agent.commands.complete.response",
      payload: {
        agentId: "draft",
        supported: true,
        items: [
          {
            id: "review",
            label: "/review",
            insertText: "/review ",
            replaceRange: { start: 0, end: 5 },
          },
        ],
        requestId: "req-2",
      },
    });
    expect(supported.payload.items[0]?.triggerCompletion).toBe(false);
  });
});
