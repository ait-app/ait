import { readFileSync } from "node:fs";
import { expect, test } from "vitest";
import {
  ListCommandsResponseSchema,
  CompleteCommandsRequestSchema,
  CompleteCommandsResponseSchema,
} from "./messages.js";
import { validateWSOutboundMessage } from "./validation/ws-outbound.js";

const native = JSON.parse(
  readFileSync(
    new URL("../../../crates/provider/tests/fixtures/omp_commands.json", import.meta.url),
    "utf8",
  ),
);

test("OMP aliases and subcommands survive schema and generated wire validation", () => {
  const commands = native.commands.map(
    ({ input, ...command }: { input?: { hint?: string }; source: string }) => ({
      ...command,
      argumentHint: input?.hint ?? "",
      kind: command.source === "skill" ? "skill" : "command",
    }),
  );
  const message = {
    type: "agent.commands.list.response",
    payload: { agentId: "omp-session", commands, error: null, requestId: "commands" },
  };
  expect(ListCommandsResponseSchema.parse(message)).toEqual(message);
  const envelope = { type: "session", message };
  expect(validateWSOutboundMessage(envelope)).toEqual({ success: true, data: envelope });
});

test("command responses from providers without argument metadata remain compatible", () => {
  const message = {
    type: "agent.commands.list.response",
    payload: {
      agentId: "other-session",
      commands: [{ name: "compact", description: "Compact context", argumentHint: "" }],
      error: null,
      requestId: "legacy",
    },
  };
  expect(ListCommandsResponseSchema.parse(message)).toEqual(message);
});

test("native multi-level edits survive generated outbound validation", () => {
  const message = {
    type: "agent.commands.complete.response",
    payload: {
      agentId: "omp-session",
      requestId: "completion",
      supported: true,
      items: [
        {
          id: "third",
          label: "third",
          description: "Native dynamic choice",
          hint: "remaining",
          insertText: "third ",
          replaceRange: { start: 14, end: 15 },
          cursor: 20,
          triggerCompletion: true,
        },
      ],
    },
  };
  expect(CompleteCommandsResponseSchema.parse(message)).toEqual(message);
  const envelope = { type: "session", message };
  expect(validateWSOutboundMessage(envelope)).toEqual({ success: true, data: envelope });
});

test("completion input rejects negative and fractional cursor offsets", () => {
  const request = {
    type: "agent.commands.complete.request",
    agentId: "omp",
    text: "/nested ",
    cursor: 8,
    requestId: "complete",
  };
  expect(CompleteCommandsRequestSchema.safeParse(request).success).toBe(true);
  for (const cursor of [-1, 1.5])
    expect(CompleteCommandsRequestSchema.safeParse({ ...request, cursor }).success).toBe(false);
});
