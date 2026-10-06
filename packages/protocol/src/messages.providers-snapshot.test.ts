import { describe, expect, test } from "vitest";
import { validateWSOutboundMessage } from "./validation/ws-outbound.js";
import {
  CompactProviderSnapshotModelSchema,
  ListProviderModelsResponseMessageSchema,
  GetProvidersSnapshotResponseMessageSchema,
  ProviderSnapshotEntrySchema,
  ProvidersSnapshotUpdateMessageSchema,
} from "./messages.js";

describe("provider snapshot message schemas", () => {
  test("defaults missing provider snapshot entry enabled state to true", () => {
    const parsed = ProviderSnapshotEntrySchema.parse({
      provider: "codex",
      status: "ready",
      label: "Codex",
    });

    expect(parsed.enabled).toBe(true);
  });

  test("preserves disabled provider snapshot entries", () => {
    const parsed = ProviderSnapshotEntrySchema.parse({
      provider: "claude",
      status: "unavailable",
      enabled: false,
      label: "Claude",
    });

    expect(parsed.enabled).toBe(false);
  });

  test("preserves enabled provider snapshot entries", () => {
    const parsed = ProviderSnapshotEntrySchema.parse({
      provider: "opencode",
      status: "loading",
      enabled: true,
      label: "OpenCode",
    });

    expect(parsed.enabled).toBe(true);
  });

  test("preserves provider snapshot entry source", () => {
    const parsed = ProviderSnapshotEntrySchema.parse({
      provider: "gemini",
      status: "ready",
      enabled: true,
      source: "custom",
      label: "Gemini",
    });

    expect(parsed.source).toBe("custom");
  });

  test("defaults missing enabled state in providers snapshot response entries", () => {
    const parsed = GetProvidersSnapshotResponseMessageSchema.parse({
      type: "get_providers_snapshot_response",
      payload: {
        entries: [
          {
            provider: "codex",
            status: "ready",
            label: "Codex",
          },
          {
            provider: "claude",
            status: "unavailable",
            enabled: false,
            label: "Claude",
          },
        ],
        generatedAt: "2026-04-24T00:00:00.000Z",
        requestId: "req-providers",
      },
    });

    expect(parsed.payload.entries.map((entry) => entry.enabled)).toEqual([true, false]);
  });

  test("defaults missing enabled state in providers snapshot update entries", () => {
    const parsed = ProvidersSnapshotUpdateMessageSchema.parse({
      type: "providers_snapshot_update",
      payload: {
        cwd: "/tmp/repo",
        entries: [
          {
            provider: "codex",
            status: "ready",
            label: "Codex",
          },
        ],
        generatedAt: "2026-04-24T00:00:00.000Z",
      },
    });

    expect(parsed.payload.entries[0]?.enabled).toBe(true);
  });
});

test("accepts a bodyless announcement with separate discovery freshness", () => {
  const message = {
    type: "providers_snapshot_update",
    payload: {
      cwd: "/project",
      entries: [],
      snapshotHash: "content-hash",
      fetchedAt: { codex: "2026-09-06T12:00:00.000Z" },
      generatedAt: "2026-09-06T13:00:00.000Z",
    },
  };
  expect(ProvidersSnapshotUpdateMessageSchema.parse(message)).toEqual(message);
  const result = validateWSOutboundMessage({ type: "session", message });
  expect(result.success).toBe(true);
});

test("preserves models without an optional description", () => {
  const model = { provider: "deepseek", id: "model", label: "Model" };
  const response = ListProviderModelsResponseMessageSchema.parse({
    type: "list_provider_models_response",
    payload: {
      provider: "deepseek",
      models: [model],
      requestId: "models",
      fetchedAt: "2026-10-02T02:00:00.000Z",
    },
  });

  expect(response.payload.models?.[0]).toStrictEqual(model);
});

test("normalizes null model descriptions from native providers", () => {
  const model = { provider: "opencode", id: "model", label: "Model", description: null };
  const entry = ProviderSnapshotEntrySchema.parse({
    provider: "opencode",
    status: "ready",
    models: [model],
  });
  expect(entry.models?.[0]?.description).toBeUndefined();
  expect(CompactProviderSnapshotModelSchema.parse(model).description).toBeUndefined();
  const response = ListProviderModelsResponseMessageSchema.parse({
    type: "list_provider_models_response",
    payload: {
      provider: "opencode",
      models: [model],
      requestId: "models",
      fetchedAt: "2026-10-02T02:00:00.000Z",
    },
  });
  expect(response.payload.models?.[0]?.description).toBeUndefined();
  const validated = validateWSOutboundMessage({
    type: "session",
    message: { ...response, payload: { ...response.payload, models: [model] } },
  });
  expect(validated.success).toBe(true);
  expect(
    ProviderSnapshotEntrySchema.safeParse({
      provider: "opencode",
      status: "ready",
      models: [{ ...model, description: 42 }],
    }).success,
  ).toBe(false);
});
