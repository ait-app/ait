import { expect, test } from "vitest";
import { ListCommandsResponseSchema } from "./messages.js";
import { validateWSOutboundMessage } from "./validation/ws-outbound.js";

test("DSH's plugin identity and native attachment declaration survive command wire validation", () => {
  const message = {
    type: "agent.commands.list.response",
    payload: {
      agentId: "dsh-session",
      commands: [
        {
          name: "goal",
          description: "Manage a native goal",
          argumentHint: "<objective>",
          kind: "command",
          source: "extension",
          definitionId: "@deepseek-ai/dsh-command-goal:goal",
          acceptsAttachments: true,
        },
      ],
      error: null,
      requestId: "commands",
    },
  };
  expect(ListCommandsResponseSchema.parse(message)).toEqual(message);
  const envelope = { type: "session", message };
  expect(validateWSOutboundMessage(envelope)).toEqual({ success: true, data: envelope });
});
