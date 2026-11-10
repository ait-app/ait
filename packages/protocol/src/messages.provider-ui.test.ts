import { expect, test } from "vitest";
import { NativeActionRequestSchema, ProviderUiStateSchema } from "./messages.js";
import { validateWSOutboundMessage } from "./validation/ws-outbound.js";

test("native UI validates ephemeral editor and browser actions", () => {
  const state = {
    statuses: [],
    widgets: [],
    title: null,
    editor: { id: "native-1", text: "" },
    link: {
      url: "https://example.com/login",
      launchUrl: "https://example.com/login",
      instructions: null,
    },
  };
  expect(ProviderUiStateSchema.parse(state)).toEqual(state);
  expect(
    ProviderUiStateSchema.safeParse({
      ...state,
      link: { ...state.link, launchUrl: "javascript:alert(1)" },
    }).success,
  ).toBe(false);
  expect(
    ProviderUiStateSchema.safeParse({
      ...state,
      widgets: [{ key: "widget", lines: [42], placement: "aboveEditor" }],
    }).success,
  ).toBe(false);
});

test("semantic operation requests and detached receipts retain their wire fields", () => {
  expect(
    NativeActionRequestSchema.safeParse({
      type: "agent.native_action.request",
      agentId: "agent",
      action: "login",
      arguments: { providerId: "example" },
      requestId: "request",
    }).success,
  ).toBe(true);
  const message = {
    type: "session",
    message: {
      type: "agent.native_action.response",
      payload: { agentId: "agent", requestId: "request", text: "Done", file: null, error: null },
    },
  };
  expect(validateWSOutboundMessage(message)).toEqual({ success: true, data: message });
});

test("optional side questions preserve legacy snapshots and bound native thread previews", () => {
  const base = { statuses: [], widgets: [], title: null, editor: null, link: null };
  const topic = {
    id: "topic",
    question: "Question",
    createdAt: 1,
    truncated: false,
    turns: [
      {
        question: "Question",
        answer: "Partial",
        status: "running",
        createdAt: 1,
        updatedAt: 2,
        error: null,
      },
    ],
  };
  const state = {
    ...base,
    sideQuestions: {
      records: [topic],
      omitted: 0,
      error: null,
      runningId: "topic",
      reveal: "topic:1",
    },
  };
  expect(ProviderUiStateSchema.parse(base)).toEqual(base);
  expect(ProviderUiStateSchema.parse(state)).toEqual(state);
  expect(
    ProviderUiStateSchema.safeParse({
      ...state,
      sideQuestions: { ...state.sideQuestions, records: Array.from({ length: 9 }, () => topic) },
    }).success,
  ).toBe(false);
  expect(
    ProviderUiStateSchema.safeParse({
      ...state,
      sideQuestions: {
        ...state.sideQuestions,
        records: [{ ...topic, turns: [{ ...topic.turns[0], answer: "x".repeat(48 * 1024 + 1) }] }],
      },
    }).success,
  ).toBe(false);
});

test("optional vibe state preserves older snapshots and bounds native workers", () => {
  const base = { statuses: [], widgets: [], title: null, editor: null, link: null };
  const worker = {
    id: "id",
    parentId: "root",
    status: "running" as const,
    description: "Inspect the parser",
    model: "gpt-5",
  };
  const state = { ...base, vibe: { enabled: true, workers: [worker] } };
  expect(ProviderUiStateSchema.parse(base)).toEqual(base);
  expect(ProviderUiStateSchema.parse(state)).toEqual(state);
  expect(
    ProviderUiStateSchema.safeParse({
      ...state,
      vibe: { enabled: true, workers: Array.from({ length: 257 }, () => worker) },
    }).success,
  ).toBe(false);
  expect(
    ProviderUiStateSchema.safeParse({
      ...state,
      vibe: { enabled: true, workers: [{ ...worker, status: "paused" }] },
    }).success,
  ).toBe(false);
  expect(
    ProviderUiStateSchema.safeParse({
      ...state,
      vibe: { enabled: true, workers: [{ ...worker, description: "x".repeat(4097) }] },
    }).success,
  ).toBe(false);
});

test("current goal controls are optional and preserve native state independently of history", () => {
  const base = { statuses: [], widgets: [], title: null, editor: null, link: null };
  const goal = {
    type: "provider_command",
    id: "current-goal",
    command: "goal",
    status: "success",
    text: "Goal paused",
    goal: {
      provider: "omp",
      id: "native-goal",
      phase: "paused",
      objective: "Verify the release",
      tokensUsed: 12,
    },
    actions: [
      { id: "resume", command: "/goal resume" },
      { id: "clear", command: "/goal drop" },
    ],
  };
  expect(ProviderUiStateSchema.parse(base)).toEqual(base);
  expect(ProviderUiStateSchema.parse({ ...base, goal }).goal).toEqual(goal);
  expect(
    ProviderUiStateSchema.safeParse({
      ...base,
      goal: { ...goal, goal: { ...goal.goal, phase: "unknown" } },
    }).success,
  ).toBe(false);
});
