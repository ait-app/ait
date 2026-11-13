// @vitest-environment jsdom
import React, { type ReactNode } from "react";
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, expect, it, vi } from "vitest";
import { useDraftAgentFeatures } from "./use-draft-agent-features";

const client = {
  listProviderFeatures: vi.fn(async () => ({
    features: [
      { type: "select", id: "modelRole.worker", label: "Worker", value: "native", options: [] },
    ],
  })),
};
vi.mock("@/runtime/host-runtime", () => ({
  useHostRuntimeClient: () => client,
  useHostRuntimeIsConnected: () => true,
}));
vi.mock("./use-form-preferences", () => ({
  useFormPreferences: () => ({ preferences: {}, updatePreferences: vi.fn() }),
}));
afterEach(cleanup);

it("retains the new role snapshot when the preset switches the draft's provider", async () => {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );
  const { result, rerender } = renderHook(
    ({ provider }) =>
      useDraftAgentFeatures({
        serverId: "host",
        provider,
        cwd: "/project",
        modeId: "",
        modelId: "model",
        thinkingOptionId: "",
      }),
    { initialProps: { provider: "codex" }, wrapper },
  );
  await waitFor(() => expect(result.current.isLoading).toBe(false));
  act(() => {
    result.current.applyProfileFeatureValues({ "modelRole.worker": "fast" }, "omp");
    rerender({ provider: "omp" });
  });
  await waitFor(() => expect(result.current.featureValues).toEqual({ "modelRole.worker": "fast" }));
});

it("keeps the full explicit snapshot available for immediate submission while the new model catalog is pending", async () => {
  let finishCatalog!: (value: Awaited<ReturnType<typeof client.listProviderFeatures>>) => void;
  const pending = new Promise<Awaited<ReturnType<typeof client.listProviderFeatures>>>(
    (resolve) => {
      finishCatalog = resolve;
    },
  );
  client.listProviderFeatures.mockImplementationOnce(async () => ({
    features: [
      { type: "select", id: "modelRole.worker", label: "Worker", value: "native", options: [] },
    ],
  }));
  client.listProviderFeatures.mockImplementationOnce(() => pending);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );
  const { result, rerender } = renderHook(
    ({ modelId }) =>
      useDraftAgentFeatures({
        serverId: "host",
        provider: "omp",
        cwd: "/project",
        modeId: "",
        modelId,
        thinkingOptionId: "",
      }),
    { initialProps: { modelId: "old" }, wrapper },
  );
  await waitFor(() => expect(result.current.isLoading).toBe(false));
  const snapshot = { "modelRole.worker": "fast", "modelRole.obsolete": "native", autoRetry: true };
  act(() => {
    result.current.applyProfileFeatureValues(snapshot, "omp");
    rerender({ modelId: "fast" });
  });
  expect(result.current.isLoading).toBe(true);
  expect(result.current.featureValues).toEqual(snapshot);
  await act(async () => {
    finishCatalog({
      features: [
        { type: "select", id: "modelRole.worker", label: "Worker", value: "native", options: [] },
      ],
    });
  });
  await waitFor(() => expect(result.current.featureValues).toEqual({ "modelRole.worker": "fast" }));
});
