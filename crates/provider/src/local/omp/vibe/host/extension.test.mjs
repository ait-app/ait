import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const source =
  (await readFile(new URL("./extension.ts", import.meta.url), "utf8")) +
  (await readFile(new URL("../../btw/branch-extension.js", import.meta.url), "utf8"));
const { default: extension } = await import(
  `data:text/javascript;base64,${Buffer.from(source).toString("base64")}`
);

function fixture(tools = ["read", "todo", "goal"]) {
  let active = ["read", "todo"];
  const commands = new Map();
  const messages = [];
  extension({
    registerCommand: (name, command) => commands.set(name, command),
    getAllTools: () => tools.map((name) => ({ name })),
    getActiveTools: () => active,
    setActiveTools: async (next) => {
      active = next;
    },
    sendMessage: (...message) => messages.push(message),
    on() {},
  });
  return { commands, messages, active: () => active };
}

test("direct Goal preparation enables the native tool without starting model work", async () => {
  const state = fixture();
  await state.commands.get("ait-goal-tools-v1").handler();
  await state.commands.get("ait-goal-tools-v1").handler();
  assert.deepEqual(state.active(), ["read", "todo", "goal"]);
  assert.equal(state.messages.length, 0);
});

test("missing Goal capability fails before starting an interview or model turn", async () => {
  const state = fixture(["read", "todo"]);
  await assert.rejects(state.commands.get("ait-goal-tools-v1").handler(), /goal tool/);
  assert.deepEqual(state.active(), ["read", "todo"]);
  assert.equal(state.messages.length, 0);
});

test("guided Goal shares the same native tool preparation and keeps the confirmation instruction", async () => {
  const state = fixture();
  await state.commands.get("ait-guided-goal-v1").handler("Verify the real handoff page");
  assert.ok(state.active().includes("goal"));
  assert.match(state.messages[0][0].content, /Do not create a goal until the user has confirmed/);
  assert.equal(state.messages[0][1].triggerTurn, true);
});
