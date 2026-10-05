#!/usr/bin/env node
const assert = require("node:assert/strict");
const fs = require("node:fs");
const readline = require("node:readline");

assert.deepEqual(process.argv.slice(2), ["--profile", "acp"]);
const scenario = process.env.ACP_FIXTURE_SCENARIO;
const pro = '["deepseek-official","deepseek-v4-pro"]';
const flash = '["deepseek-official","deepseek-v4-flash"]';
let model = pro;
let effort = "high";
let active;
let permission;
const send = (message) =>
  process.stdout.write(JSON.stringify({ jsonrpc: "2.0", ...message }) + "\n");
const reply = (id, result) => send({ id, result });
const options = () => [
  {
    id: "model",
    name: "Model",
    category: "model",
    type: "select",
    currentValue: model,
    options: [
      {
        group: "deepseek-official",
        name: "DeepSeek",
        options: [
          { value: pro, name: "DeepSeek V4 Pro" },
          { value: flash, name: "DeepSeek V4 Flash" },
        ],
      },
    ],
  },
  {
    id: "reasoning_effort",
    name: "Reasoning effort",
    category: "thought_level",
    type: "select",
    currentValue: effort,
    options: (model === flash ? ["off", "high"] : ["off", "low", "high", "max"]).map((value) => ({
      value,
      name: value,
    })),
  },
];
const update = (update) =>
  send({
    method: "session/update",
    params: {
      sessionId: scenario === "wrong-session" ? "other-session" : "native-one",
      update,
    },
  });
const finish = () => {
  update({
    sessionUpdate: "tool_call_update",
    toolCallId: "tool-1",
    status: "completed",
    content: [{ type: "content", content: { type: "text", text: "done" } }],
  });
  update({
    sessionUpdate: "agent_message_chunk",
    messageId: "assistant-1",
    content: { type: "text", text: "Hello " },
  });
  update({
    sessionUpdate: "agent_message_chunk",
    messageId: "assistant-1",
    content: { type: "text", text: "world" },
  });
  update({ sessionUpdate: "usage_update", used: 123, size: 1000 });
  reply(active, { stopReason: "end_turn" });
  active = undefined;
};
readline.createInterface({ input: process.stdin }).on("line", (line) => {
  const message = JSON.parse(line);
  assert.equal(message.jsonrpc, "2.0");
  if (process.env.ACP_FIXTURE_LOG)
    fs.appendFileSync(
      process.env.ACP_FIXTURE_LOG,
      JSON.stringify({
        ...message,
        hasEnvironment: process.env.ACP_TEST_ENV === "test-only-value",
      }) + "\n",
    );
  if (!message.method) {
    assert.equal(message.id, "permission-one");
    assert.equal(message.result.outcome.outcome, "selected");
    if (permission) {
      permission = false;
      finish();
    }
    return;
  }
  const { id, method, params } = message;
  switch (method) {
    case "initialize":
      assert.equal(params.protocolVersion, 1);
      assert.deepEqual(params.clientCapabilities, {});
      if (scenario === "hung") return;
      if (scenario === "malformed") {
        process.stdout.write("not json\n");
        return;
      }
      if (scenario === "oversized") {
        process.stdout.write("x".repeat(3 * 1024 * 1024) + "\n");
        return;
      }
      reply(id, {
        protocolVersion: scenario === "bad-version" ? 99 : 1,
        agentCapabilities: {
          sessionCapabilities: { resume: {}, close: {}, list: {} },
          promptCapabilities: { image: scenario === "image-enabled" },
        },
      });
      break;
    case "session/new":
    case "session/resume":
      assert.ok(params.cwd.startsWith("/"));
      assert.ok(Array.isArray(params.mcpServers));
      if (method === "session/resume") assert.equal(params.sessionId, "native-one");
      reply(id, {
        ...(method === "session/new" ? { sessionId: "native-one" } : {}),
        configOptions: options(),
      });
      break;
    case "session/set_config_option":
      assert.equal(params.sessionId, "native-one");
      if (params.configId === "model") {
        assert.ok([pro, flash].includes(params.value));
        model = params.value;
      } else {
        assert.equal(params.configId, "reasoning_effort");
        effort = params.value;
      }
      reply(id, { configOptions: options() });
      update({ sessionUpdate: "config_option_update", configOptions: options() });
      break;
    case "session/prompt":
      assert.equal(params.sessionId, "native-one");
      active = id;
      if (params.prompt.some((block) => block.text === "wait")) break;
      update({
        sessionUpdate: "agent_thought_chunk",
        messageId: "thought-1",
        content: { type: "text", text: "Thinking" },
      });
      update({
        sessionUpdate: "tool_call",
        toolCallId: "tool-1",
        title: "shell",
        kind: "other",
        status: "in_progress",
        rawInput: { command: "pwd" },
      });
      permission = true;
      send({
        id: "permission-one",
        method: "session/request_permission",
        params: {
          sessionId: "native-one",
          toolCall: { toolCallId: "tool-1", title: "shell", rawInput: { command: "pwd" } },
          options: [
            { optionId: "once", name: "Allow once", kind: "allow_once" },
            { optionId: "deny", name: "Reject once", kind: "reject_once" },
          ],
        },
      });
      break;
    case "session/cancel":
      assert.equal(params.sessionId, "native-one");
      assert.equal(id, undefined);
      reply(active, { stopReason: "cancelled" });
      active = undefined;
      break;
    case "session/close":
      reply(id, {});
      break;
    default:
      send({ id, error: { code: -32601, message: "Unknown method" } });
  }
});
