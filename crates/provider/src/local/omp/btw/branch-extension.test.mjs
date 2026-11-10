import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
const source = await readFile(new URL("./branch-extension.js", import.meta.url), "utf8");
const { registerBtwBranch } = await import(
  `data:text/javascript;base64,${Buffer.from(source).toString("base64")}`
);
function fixture() {
  let command;
  const acknowledgements = [];
  let id = "parent",
    path = "/parent.jsonl",
    leaf = "leaf";
  const entries = [
    { id: "leaf", parentId: null, type: "message", message: { role: "user", content: "parent" } },
  ];
  const copied = [],
    messages = [],
    switches = [];
  const manager = {
    getSessionId: () => id,
    getSessionFile: () => path,
    getLeafId: () => leaf,
    getBranch: () => entries,
    getLabel: () => undefined,
  };
  const ctx = {
    sessionManager: manager,
    isIdle: () => true,
    hasPendingMessages: () => false,
    model: { api: "openai-completions", provider: "fixture", id: "model" },
    ui: {
      notify: (message) =>
        acknowledgements.push(JSON.parse(message.split("AIT_BTW_BRANCH_V1:")[1])),
    },
    newSession: async ({ parentSession, setup }) => {
      assert.equal(parentSession, "/parent.jsonl");
      id = "branch";
      path = "/branch.jsonl";
      leaf = null;
      await setup({
        ingestReplicatedEntry: (entry) => copied.push(entry),
        appendLabelChange() {},
        appendMessage: (message) => messages.push(message),
        flush: async () => {},
      });
      return { cancelled: false };
    },
    switchSession: async (next) => {
      switches.push(next);
      path = next;
      return { cancelled: false };
    },
  };
  registerBtwBranch({
    registerCommand: (_, value) => {
      command = value;
    },
  });
  const run = (change = {}) =>
    command.handler(
      JSON.stringify({
        requestId: "r",
        sessionId: "parent",
        leafId: "leaf",
        question: "question",
        answer: "answer",
        ...change,
      }),
      ctx,
    );
  return { ctx, run, entries, copied, messages, switches, acknowledgements };
}
test("Branch carries native entries and exact Q&A, then reloads the new session without inference", async () => {
  const f = fixture();
  await f.run();
  assert.deepEqual(f.copied, f.entries);
  assert.notEqual(f.copied[0], f.entries[0]);
  assert.deepEqual(
    f.messages.map((m) => [m.role, m.content[0].text]),
    [
      ["user", "question"],
      ["assistant", "answer"],
    ],
  );
  assert.deepEqual(f.switches, ["/branch.jsonl"]);
  assert.deepEqual(f.acknowledgements, [{ requestId: "r", success: true }]);
});
test("Busy and stale context reject before creating or switching a session", async () => {
  for (const change of [{ sessionId: "foreign" }, { leafId: "stale" }]) {
    const f = fixture();
    await f.run(change);
    assert.equal(f.messages.length, 0);
    assert.equal(f.acknowledgements[0].success, false);
  }
  const f = fixture();
  f.ctx.isIdle = () => false;
  await f.run();
  assert.equal(f.messages.length, 0);
  assert.equal(f.switches.length, 0);
});
test("A failed open restores the original session and reports failure", async () => {
  const f = fixture();
  f.ctx.switchSession = async (path) => {
    f.switches.push(path);
    return { cancelled: path === "/branch.jsonl" };
  };
  await f.run();
  assert.deepEqual(f.switches, ["/branch.jsonl", "/parent.jsonl"]);
  assert.equal(f.acknowledgements[0].success, false);
});
test("Native cancellation leaves the original conversation untouched", async () => {
  const f = fixture();
  f.ctx.newSession = async () => ({ cancelled: true });
  await f.run();
  assert.equal(f.messages.length, 0);
  assert.equal(f.switches.length, 0);
  assert.equal(f.acknowledgements[0].success, false);
});
