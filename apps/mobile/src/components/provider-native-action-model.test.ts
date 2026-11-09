import { expect, it } from "vitest";
import {
  nativeActionArguments,
  nativeActionFields,
  nativeActionGroup,
  nativeActionValid,
  nativeObjectList,
  type NativeAction,
} from "./provider-native-action-model";
const fields = (ids: string[]) =>
  ids.map((id) => ({
    id,
    label: id,
    options:
      id === "operation"
        ? ["list", "spawn", "send", "wait", "kill", "toggle"]
        : id === "op"
          ? ["get", "create", "resume", "pause", "drop"]
          : id === "cli"
            ? ["fast", "good"]
            : [],
    required: id === "operation" || id === "op",
  }));
const vibe: NativeAction = {
  id: "vibe_control",
  label: "Vibe",
  fields: fields([
    "operation",
    "enabled",
    "cli",
    "prompt",
    "name",
    "sessionId",
    "message",
    "sessions",
    "timeout",
  ]),
};
const goal: NativeAction = {
  id: "goal",
  label: "Goal",
  fields: fields(["op", "objective", "token_budget"]),
};
it("keeps preset writes out of every GUI group and places account/configuration in settings", () => {
  expect(nativeActionGroup("model_preset_save")).toBeNull();
  expect(nativeActionGroup("model_preset_delete")).toBeNull();
  expect(nativeActionGroup("logout")).toBe("accounts");
  expect(nativeActionGroup("set_cache_warming")).toBe("runtime");
  expect(nativeActionGroup("branch")).toBe("history");
});
it.each(["list", "spawn", "send", "wait", "kill", "toggle"])(
  "bounds Vibe %s to its own fields",
  (operation) => {
    const values = {
      operation,
      cli: "fast",
      prompt: "Task",
      sessionId: "worker",
      message: "Guide",
      sessions: "one,two",
      timeout: "30",
      enabled: "false",
    };
    const result = nativeActionArguments(vibe, values);
    const allowed: string[] = {
      list: [],
      spawn: ["cli", "prompt", "name"],
      send: ["sessionId", "message"],
      wait: ["sessions", "timeout"],
      kill: ["sessionId"],
      toggle: ["enabled"],
    }[operation]!;
    expect(Object.keys(result).every((key) => key === "operation" || allowed.includes(key))).toBe(
      true,
    );
  },
);
it("requires worker class/task, guidance target/text and an explicit toggle target", () => {
  for (const values of [
    { operation: "spawn" },
    { operation: "spawn", cli: "fast", prompt: "  " },
    { operation: "send", sessionId: "worker" },
    { operation: "kill" },
    { operation: "toggle" },
  ] as Record<string, string>[])
    expect(nativeActionValid(vibe, values)).toBe(false);
  expect(nativeActionValid(vibe, { operation: "spawn", cli: "fast", prompt: "Task" })).toBe(true);
  expect(nativeActionValid(vibe, { operation: "toggle", enabled: "false" })).toBe(true);
});
it("Goal creates require an objective; other operations discard create fields", () => {
  expect(nativeActionValid(goal, { op: "create" })).toBe(false);
  expect(nativeActionArguments(goal, { op: "get", objective: "old", token_budget: "100" })).toEqual(
    { op: "get" },
  );
  expect(nativeActionFields(goal, { op: "pause" }).map((field) => field.id)).toEqual(["op"]);
});
it("rejects invalid budgets and timeouts before sending native RPC", () => {
  for (const token_budget of ["-1", "0", "1.5", "NaN", "9007199254740992"])
    expect(nativeActionValid(goal, { op: "create", objective: "Task", token_budget })).toBe(false);
  for (const timeout of ["-1", "0", "Infinity", "601"])
    expect(nativeActionValid(vibe, { operation: "wait", timeout })).toBe(false);
});
it("reads human entry/provider/account labels and keeps identifiers only in values", () => {
  expect(
    nativeObjectList(
      JSON.stringify({
        entries: [
          {
            id: "native-id",
            type: "message",
            message: { content: [{ type: "text", text: "Build a task board" }] },
          },
        ],
      }),
      "entries",
    ).options,
  ).toEqual([{ id: "native-id", label: "Build a task board" }]);
  expect(
    nativeObjectList(
      '{"providers":[{"id":"openai","name":"OpenAI","available":true}]}',
      "providers",
    ).options,
  ).toEqual([{ id: "openai", label: "OpenAI" }]);
  expect(
    nativeObjectList('{"accounts":[{"credentialId":42,"label":"Work account"}]}', "accounts")
      .options,
  ).toEqual([{ id: "42", label: "Work account" }]);
  expect(() => nativeObjectList("invalid", "entries")).toThrow("Incomplete native object list");
});

it("keeps subagent and topic mutations on their object rows, and only asks for a new side question", () => {
  for (const id of ["get_subagents", "steer_subagent", "cancel_subagent", "btw_cancel"])
    expect(nativeActionGroup(id)).toBeNull();
  const action: NativeAction = {
    id: "btw",
    label: "Ask",
    fields: fields(["question", "recordId"]),
  };
  expect(nativeActionFields(action, {}).map((field) => field.id)).toEqual(["question"]);
  expect(
    nativeActionArguments(action, { question: "New question", recordId: "old topic" }),
  ).toEqual({ question: "New question" });
});

it("preserves role identity and omitted counts for the task-specific history picker", () => {
  expect(
    nativeObjectList(
      '{"entries":[{"id":"user","label":"Request","role":"user"}],"omitted":42}',
      "entries",
    ),
  ).toEqual({ options: [{ id: "user", label: "Request", role: "user" }], omitted: 42 });
});
