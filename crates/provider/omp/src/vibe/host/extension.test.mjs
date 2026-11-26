import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const source = await readFile(new URL("./extension.ts", import.meta.url), "utf8");
const { default: extension } = await import(
  `data:text/javascript;base64,${Buffer.from(source).toString("base64")}`
);
const names = ["vibe_spawn", "vibe_send", "vibe_wait", "vibe_kill", "vibe_list"];
function fixture(tools = [...names, "read", "todo", "bash", "edit", "mcp__ait__agent_create"]) {
  let active = ["read", "todo", "bash", "edit", "mcp__ait__agent_create"];
  const commands = new Map();
  const hooks = new Map();
  const messages = [];
  const receipts = [];
  extension({
    registerCommand: (name, command) => commands.set(name, command),
    getAllTools: () => tools.map((name) => ({ name })),
    getActiveTools: () => [...active],
    setActiveTools: async (next) => {
      active = next;
    },
    sendMessage: (...message) => messages.push(message),
    on: (name, fn) => hooks.set(name, fn),
  });
  const ctx = {
    models: { resolve: (role) => ({ provider: "fixture", id: role }) },
    model: { provider: "fixture", id: "main" },
    ui: { notify: (message) => receipts.push(message) },
  };
  return {
    commands,
    hooks,
    messages,
    receipts,
    ctx,
    active: () => active,
    mode: (value) => commands.get("ait-vibe-mode-v1").handler(value, ctx),
  };
}

test("director limits writes, retains Ait delegation, and restores prior tools exactly", async () => {
  const state = fixture();
  const original = state.active();
  await state.mode("on");
  await state.mode("on");
  assert.deepEqual(state.active(), ["read", "todo", "mcp__ait__agent_create", ...names]);
  assert.deepEqual(JSON.parse(state.receipts[0].split("AIT_VIBE_V1:")[1]), {
    fast: ["fixture", "@smol"],
    good: ["fixture", "@task"],
  });
  const prompt = state.hooks.get("before_agent_start")({ systemPrompt: ["original"] });
  assert.equal(prompt.systemPrompt[0], "original");
  assert.match(prompt.systemPrompt[1], /delegate edits/);
  await state.mode("off");
  assert.deepEqual(state.active(), original);
  assert.equal(state.hooks.get("before_agent_start")({ systemPrompt: [] }), undefined);
});

test("missing host tools reject activation without changing tools or acknowledging mode", async () => {
  const state = fixture(["read"]);
  const original = state.active();
  await assert.rejects(state.mode("on"), /not registered/);
  assert.deepEqual(state.active(), original);
  assert.equal(state.receipts.length, 0);
  await assert.rejects(state.mode("invalid"), /on or off/);
});

test("worker results trigger a follow-up only while enabled", async () => {
  const state = fixture();
  const result = { workers: [{ id: "worker", status: "completed" }] };
  const deliver = () => state.commands.get("ait-vibe-result-v1").handler(JSON.stringify(result));
  await deliver();
  assert.equal(state.messages.length, 0);
  await state.mode("on");
  await deliver();
  assert.deepEqual(state.messages[0], [
    { customType: "ait-vibe-result", content: JSON.stringify(result), display: true },
    { deliverAs: "followUp", triggerTurn: true },
  ]);
  await state.mode("off");
  await deliver();
  assert.equal(state.messages.length, 1);
});
