#!/usr/bin/env node
// Offline Pi 1.2 RPC peer. Creates or resumes JSONL sessions; never calls a model service.
import assert from "node:assert/strict";
import { randomUUID } from "node:crypto";
import { appendFileSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import readline from "node:readline";

const args = process.argv.slice(2);
if (args.includes("--version")) {
  console.log("1.2.0");
  process.exit(0);
}

const argument = (flag) => {
  const index = args.indexOf(flag);
  return index < 0 ? undefined : args[index + 1];
};
let sessionFile = argument("--session");
const records = () => readFileSync(sessionFile, "utf8").trimEnd().split("\n").map(JSON.parse);
let header = sessionFile ? records()[0] : null;
if (sessionFile) {
  assert.equal(header.type, "session");
  assert.equal(header.cwd, process.cwd());
  assert.equal(argument("--session-dir"), path.dirname(sessionFile));
} else if (!args.includes("--no-session")) {
  const timestamp = new Date().toISOString();
  header = {
    type: "session",
    version: 3,
    id: argument("--session-id") ?? randomUUID(),
    cwd: process.cwd(),
    timestamp,
  };
  const directory =
    argument("--session-dir") ??
    process.env.PI_CODING_AGENT_SESSION_DIR ??
    path.join(
      process.env.PI_CODING_AGENT_DIR ?? path.join(os.homedir(), ".pi", "agent"),
      "sessions",
    );
  mkdirSync(directory, { recursive: true });
  sessionFile = path.join(directory, `${timestamp.replaceAll(":", "-")}_${header.id}.jsonl`);
  writeFileSync(sessionFile, `${JSON.stringify(header)}\n`);
}

const log = (entry) => appendFileSync(process.env.PI_IMPORT_RPC_LOG, `${JSON.stringify(entry)}\n`);
log({ type: "open", sessionFile: sessionFile ?? null, sessionId: header?.id ?? null });

const model = {
  provider: "e2e",
  id: "pi-import-model",
  name: "Pi import model",
  input: ["text", "image"],
};
const emit = (event) => process.stdout.write(`${JSON.stringify(event)}\n`);
const respond = (command, data = {}, success = true) =>
  emit({ type: "response", id: command.id, command: command.type, success, data });

for await (const line of readline.createInterface({ input: process.stdin })) {
  const command = JSON.parse(line);
  switch (command.type) {
    case "get_available_models":
      respond(command, { models: [model] });
      break;
    case "get_state":
      respond(command, {
        sessionId: header?.id,
        sessionFile,
        model,
        isStreaming: false,
        isCompacting: false,
        messageCount: sessionFile
          ? records().filter((record) => record.type === "message").length
          : 0,
        pendingMessageCount: 0,
      });
      break;
    case "set_model":
      assert.equal(command.provider, model.provider);
      assert.equal(command.modelId, model.id);
      respond(command, model);
      break;
    case "prompt": {
      assert.ok(sessionFile, "Catalog discovery must never submit input");
      const parent = records().slice(1).at(-1)?.id ?? null;
      const userId = randomUUID();
      const assistant = {
        role: "assistant",
        provider: model.provider,
        model: model.id,
        content: [{ type: "text", text: `Pi continued: ${command.message}` }],
        stopReason: "stop",
      };
      const timestamp = new Date().toISOString();
      appendFileSync(
        sessionFile,
        [
          {
            type: "message",
            id: userId,
            parentId: parent,
            timestamp,
            message: {
              role: "user",
              content: [{ type: "text", text: command.message }, ...(command.images ?? [])],
            },
          },
          { type: "message", id: randomUUID(), parentId: userId, timestamp, message: assistant },
        ]
          .map((record) => `${JSON.stringify(record)}\n`)
          .join(""),
      );
      log({
        type: "prompt",
        sessionFile,
        sessionId: header.id,
        text: command.message,
        images: command.images ?? [],
      });
      emit({ type: "agent_start" });
      respond(command, { disposition: "started" });
      emit({ type: "message_start", message: { role: "assistant" } });
      emit({
        type: "message_update",
        assistantMessageEvent: {
          type: "text_delta",
          contentIndex: 0,
          delta: assistant.content[0].text,
        },
      });
      emit({ type: "message_end", message: assistant });
      emit({ type: "agent_end", messages: [assistant] });
      emit({ type: "agent_settled", aborted: false });
      break;
    }
    default:
      respond(command, {}, false);
  }
}
