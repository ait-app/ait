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
