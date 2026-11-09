import { expect, test } from "vitest";
import { ProviderPresetsListResponseMessageSchema } from "./messages.js";
import { validateWSOutboundMessage } from "./validation/ws-outbound.js";

test("preset list ownership survives schema and generated envelope validation", () => {
  const message = {
    type: "provider.presets.list.response",
    payload: {
      provider: "omp",
      requestId: "scope",
      presets: [
        { name: "shared", model: "fixture", source: "project", editable: false },
        { name: "global", model: "fixture", source: "global", editable: true },
      ],
      error: null,
    },
  };
  expect(ProviderPresetsListResponseMessageSchema.parse(message)).toEqual(message);
  const envelope = { type: "session", message };
  expect(validateWSOutboundMessage(envelope)).toEqual({ success: true, data: envelope });
});

test("older preset lists preserve missing ownership without inventing write access", () => {
  const result = ProviderPresetsListResponseMessageSchema.parse({
    type: "provider.presets.list.response",
    payload: {
      provider: "omp",
      requestId: "old",
      presets: [{ name: "unknown", model: "fixture" }],
    },
  });
  expect(result.payload.presets[0].editable).toBeUndefined();
  expect(result.payload.presets[0].source).toBeUndefined();
});
