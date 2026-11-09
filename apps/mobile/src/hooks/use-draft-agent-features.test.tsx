// @vitest-environment jsdom
import React from "react";
import { afterEach, expect, it, vi } from "vitest";
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { useDraftAgentFeatures } from "./use-draft-agent-features";

const fixture = vi.hoisted(() => ({ features: vi.fn(), apply: vi.fn(), toast: vi.fn() }));
vi.mock("@/runtime/host-runtime", () => ({
  useHostRuntimeClient: () => ({ listProviderFeatures: fixture.features }),
  useHostRuntimeIsConnected: () => true,
}));
vi.mock("./use-providers-snapshot", () => ({
  useProvidersSnapshot: () => ({ entries: [{ provider: "omp", fetchedAt: "1" }] }),
}));
vi.mock("./use-form-preferences", () => ({
  useFormPreferences: () => ({ preferences: {}, updatePreferences: vi.fn().mockResolvedValue({}) }),
  mergeProviderPreferences: () => ({}),
}));
vi.mock("@/contexts/toast-context", () => ({ useToast: () => ({ error: fixture.toast }) }));
afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

it("keeps scoped role choices and drafts visible through a slow or failed model refresh", async () => {
  const config = {
    modelId: '["other","model"]',
    thinkingOptionId: "high",
    featureValues: {
      modelPreset: null,
      modelPresetThinking: "high",
      modelPresetRoles: '{"default":"other/model"}',
      "modelRole.default": "other/model",
      "modelRole.advisor": "unset",
    },
  };
  const features = [
    {
      type: "select",
      id: "modelPreset",
      label: "Preset",
      value: "__custom__",
      options: [{ id: "work", label: "Work", metadata: { configuration: config } }],
    },
    {
      type: "select",
      id: "modelRole.default",
      label: "Default",
      value: "native",
      options: [
        { id: "native", label: "Native" },
        { id: "other/model", label: "Other" },
      ],
    },
    {
      type: "select",
      id: "modelRole.advisor",
      label: "Advisor",
      value: "native",
      options: [
        { id: "native", label: "Native" },
        { id: "unset", label: "Unassigned" },
      ],
    },
  ];
  let reject!: (error: Error) => void;
  fixture.features.mockResolvedValueOnce({ features }).mockImplementationOnce(
    () =>
      new Promise((_, fail) => {
        reject = fail;
      }),
  );
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const wrapper = ({ children }: { children: React.ReactNode }) => (
    <QueryClientProvider client={client}>{children}</QueryClientProvider>
  );
  const { result, rerender } = renderHook(
    ({ modelId, serverId }) =>
      useDraftAgentFeatures({
        serverId,
        provider: "omp",
        cwd: "/project",
        modelId,
        modeId: "ask",
        thinkingOptionId: "high",
        onApplyModelPreset: fixture.apply,
      }),
    { wrapper, initialProps: { modelId: '["test","model"]', serverId: "host" } },
  );
  await waitFor(() => expect(result.current.features).toHaveLength(3));
  rerender({ modelId: '["other","model"]', serverId: "host" });
  await waitFor(() => expect(fixture.features).toHaveBeenCalledTimes(2));
  expect(result.current.features).toHaveLength(3);
  act(() => result.current.setFeatureValue("modelPreset", "work"));
  expect(fixture.apply).toHaveBeenCalledExactlyOnceWith(config);
  expect(result.current.featureValues?.["modelRole.advisor"]).toBe("unset");
  await act(async () => reject(new Error("native reader failed")));
  expect(result.current.features).toHaveLength(3);
  expect(result.current.featureValues?.["modelRole.default"]).toBe("other/model");
  fixture.features.mockImplementation(() => new Promise(() => {}));
  rerender({ modelId: '["other","model"]', serverId: "another-host" });
  expect(result.current.features).toHaveLength(0);
  client.clear();
});

it("never stores or applies a synthetic custom placeholder in a draft", async () => {
  fixture.features.mockResolvedValue({
    features: [
      {
        type: "select",
        id: "modelPreset",
        label: "Preset",
        value: "__custom__",
        options: [
          {
            id: "work",
            label: "Work",
            metadata: { source: "ait", configuration: { modelId: "opus" } },
          },
        ],
      },
    ],
  });
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const wrapper = ({ children }: { children: React.ReactNode }) => (
    <QueryClientProvider client={client}>{children}</QueryClientProvider>
  );
  const { result } = renderHook(
    () =>
      useDraftAgentFeatures({
        serverId: "host",
        provider: "claude",
        cwd: "/project",
        modelId: "opus",
        modeId: null,
        thinkingOptionId: null,
        onApplyModelPreset: fixture.apply,
      }),
    { wrapper },
  );
  await waitFor(() => expect(result.current.features).toHaveLength(1));
  act(() => result.current.setFeatureValue("modelPreset", "__custom__"));
  expect(result.current.featureValues).toBeUndefined();
  expect(fixture.apply).not.toHaveBeenCalled();
  client.clear();
});
