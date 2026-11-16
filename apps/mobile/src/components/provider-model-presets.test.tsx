// @vitest-environment jsdom
import React, { type ReactNode } from "react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { AgentFeature, AgentModelDefinition } from "@ait/protocol/agent-types";
import type { ProviderPresetEntry } from "@ait/protocol/messages";
import { ProviderModelPresets, useProviderModelPresetChoices } from "./provider-model-presets";
import { invalidatePresetCaches } from "@/data/provider-presets";

const host = vi.hoisted(() => ({ supportsPresets: true }));
const client = {
  describeProviderPresets: vi.fn(),
  listProviderPresets: vi.fn(),
  listProviderFeatures: vi.fn(),
};
vi.mock("@/stores/session-store", () => ({
  useSessionStore: (select: (state: unknown) => unknown) =>
    select({
      sessions: { host: { serverInfo: { features: { providerPresets: host.supportsPresets } } } },
    }),
}));
vi.mock("@/runtime/host-runtime", () => ({
  useHostRuntimeClient: () => client,
  useHostRuntimeIsConnected: () => true,
}));
vi.mock("react-native-unistyles", () => ({ StyleSheet: { create: () => ({}) } }));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock("react-native", () => ({
  Text: ({ children }: { children: ReactNode }) => <span>{children}</span>,
  View: ({ children, testID }: { children: ReactNode; testID?: string }) => (
    <div data-testid={testID}>{children}</div>
  ),
  Pressable: ({
    children,
    onPress,
    testID,
  }: {
    children: ReactNode;
    onPress: () => void;
    testID: string;
  }) => (
    <button data-testid={testID} onClick={onPress}>
      {children}
    </button>
  ),
}));
let presets: ProviderPresetEntry[];
let features: AgentFeature[];
let models: AgentModelDefinition[];
beforeEach(() => {
  host.supportsPresets = true;
  vi.clearAllMocks();
  presets = [];
  models = [
    {
      provider: "omp",
      id: '["test","fast"]',
      aliases: ["fast"],
      label: "fast",
      defaultThinkingOptionId: "high",
      thinkingOptions: [
        { id: "off", label: "Off" },
        { id: "high", label: "High" },
      ],
    },
  ];
  features = [
    {
      type: "select",
      id: "modelRole.advisor",
      label: "Advisor",
      value: "old",
      options: [{ id: "native", label: "Inherit", isDefault: true }],
    },
    {
      type: "select",
      id: "modelRole.worker",
      label: "Worker",
      value: "old",
      options: [{ id: "native", label: "Inherit", isDefault: true }],
    },
  ];
  client.describeProviderPresets.mockResolvedValue({
    descriptor: {
      backend: "ait",
      canCreate: true,
      canEdit: true,
      canDelete: true,
      canRename: true,
    },
  });
  client.listProviderPresets.mockImplementation(async () => ({ presets, error: null }));
  client.listProviderFeatures.mockImplementation(async () => ({ features, error: null }));
});
afterEach(cleanup);

function showPicker(
  queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } }),
) {
  const apply = vi.fn();
  function Picker() {
    const choices = useProviderModelPresetChoices({
      serverId: "host",
      provider: "omp",
      cwd: "/project",
      enabled: true,
    });
    return (
      <ProviderModelPresets
        choices={choices}
        provider="omp"
        models={models}
        controls={{ cwd: "/project", apply }}
        renderRow={(row) => (
          <button data-testid={row.testID} onClick={row.onPress} disabled={row.disabled}>
            {row.name} {row.modelLabel}
          </button>
        )}
      />
    );
  }
  const view = render(
    <QueryClientProvider client={queryClient}>
      <Picker />
    </QueryClientProvider>,
  );
  return { ...view, apply, queryClient };
}

it("shows a preset saved in settings after shared-cache invalidation and applies the full role snapshot", async () => {
  const { apply, queryClient } = showPicker();
  await waitFor(() => expect(client.listProviderPresets).toHaveBeenCalledTimes(1));
  expect(screen.queryByText("modelSelector.modelPresets")).toBeNull();
  presets = [
    {
      name: "Quick",
      model: '["test","fast"]',
      source: "ait",
      editable: true,
      featureValues: { "modelRole.worker": "unset" },
    },
  ];
  await act(async () => {
    await invalidatePresetCaches(queryClient, "host", "omp");
  });
  fireEvent.click(await screen.findByRole("button", { name: "Quick fast" }));
  expect(apply).toHaveBeenCalledExactlyOnceWith("omp", {
    modelId: '["test","fast"]',
    thinkingOptionId: null,
    featureValues: { "modelRole.advisor": "native", "modelRole.worker": "unset" },
  });
  expect(client.listProviderFeatures).toHaveBeenCalledWith({ provider: "omp", cwd: "/project" });
});

it("keeps the preset group hidden for a provider that has no role capability", async () => {
  presets = [{ name: "Quick", model: "fast", source: "ait", editable: true }];
  features = [];
  showPicker();
  await waitFor(() => expect(client.listProviderFeatures).toHaveBeenCalledTimes(1));
  expect(screen.queryByText("modelSelector.modelPresets")).toBeNull();
});

it("does not advertise stale cached presets when the store fails", async () => {
  presets = [{ name: "Quick", model: "fast", source: "ait", editable: true }];
  client.listProviderPresets.mockResolvedValue({ presets, error: "Store unavailable" });
  showPicker();
  await waitFor(() => expect(client.listProviderFeatures).toHaveBeenCalledTimes(1));
  expect(screen.queryByText("Quick")).toBeNull();
});

it("does not request presets or show a group on a host without the preset capability", () => {
  host.supportsPresets = false;
  showPicker();
  expect(client.listProviderPresets).not.toHaveBeenCalled();
  expect(client.listProviderFeatures).not.toHaveBeenCalled();
  expect(screen.queryByText("modelSelector.modelPresets")).toBeNull();
});

it.each([
  { model: "retired", thinkingOptionId: undefined },
  { model: "fast", thinkingOptionId: "future-effort" },
])("retains but disables an unresolved preset without applying it (%j)", async (selection) => {
  presets = [{ name: "Unresolved", source: "global", editable: true, ...selection }];
  const { apply } = showPicker();
  const row = await screen.findByTestId("provider-model-preset-omp-Unresolved");
  expect(row.hasAttribute("disabled")).toBe(true);
  expect(row.textContent).toContain("providerSelection.unavailable");
  fireEvent.click(row);
  expect(apply).not.toHaveBeenCalled();
});

it.each([undefined, "off", "high"])(
  "applies a known model alias with its supported/default thinking (%s)",
  async (thinkingOptionId) => {
    presets = [
      { name: "Available", model: "fast", source: "global", editable: true, thinkingOptionId },
    ];
    const { apply } = showPicker();
    const row = await screen.findByTestId("provider-model-preset-omp-Available");
    expect(row.hasAttribute("disabled")).toBe(false);
    fireEvent.click(row);
    expect(apply).toHaveBeenCalledWith(
      "omp",
      expect.objectContaining({ modelId: "fast", thinkingOptionId: thinkingOptionId ?? null }),
    );
  },
);

it("does not treat off as universally supported on a model with no thinking controls", async () => {
  models = [{ provider: "omp", id: "plain", label: "Plain" }];
  presets = [
    { name: "Plain", model: "plain", thinkingOptionId: "off", source: "global", editable: true },
  ];
  const { apply } = showPicker();
  const row = await screen.findByTestId("provider-model-preset-omp-Plain");
  expect(row.hasAttribute("disabled")).toBe(true);
  fireEvent.click(row);
  expect(apply).not.toHaveBeenCalled();
});
