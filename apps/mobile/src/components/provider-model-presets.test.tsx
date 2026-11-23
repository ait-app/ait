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
  getAgentModelPresetContext: vi.fn(),
};
vi.mock("@/stores/session-store", () => ({
  useSessionStore: (select: (state: unknown) => unknown) =>
    select({
      sessions: {
        host: { client, serverInfo: { features: { providerPresets: host.supportsPresets } } },
      },
    }),
}));
vi.mock("@/runtime/host-runtime", () => ({
  useHostRuntimeClient: () => client,
  useHostRuntimeIsConnected: () => true,
}));
vi.mock("@/constants/platform", () => ({ isWeb: true }));
vi.mock("@/i18n/i18next", () => ({ i18n: { t: (key: string) => key } }));
vi.mock("react-native-unistyles", () => ({
  StyleSheet: {
    create: (factory: (theme: unknown) => unknown) =>
      factory({
        colors: { foregroundMuted: "#888" },
        spacing: { 1: 4, 2: 8, 3: 12, 6: 24 },
        fontSize: { xs: 11, sm: 12 },
        fontWeight: { medium: "500" },
      }),
  },
}));
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
      options: [
        { id: "native", label: "Inherit", isDefault: true, metadata: { supportsFallbacks: true } },
      ],
    },
    {
      type: "select",
      id: "modelRole.worker",
      label: "Worker",
      value: "old",
      options: [
        { id: "native", label: "Inherit", isDefault: true, metadata: { supportsFallbacks: true } },
      ],
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

it("explains that guest users cannot select presets even when no list is visible", () => {
  render(
    <ProviderModelPresets
      provider="omp"
      choices={{ presets: [], features: [] }}
      controls={{ permissionDenied: true, supported: false, apply: vi.fn() }}
      renderRow={() => <span />}
    />,
  );
  expect(screen.getByText("modelSelector.presetOwnerOnly")).toBeTruthy();
});

it("uses owner context and refreshes after an agent revision without guessing a same-name preset", async () => {
  client.getAgentModelPresetContext.mockResolvedValueOnce({
    agentId: "a",
    provider: "omp",
    presets: [{ name: "Quick", model: "fast", source: "project" }],
    selectedModelPreset: { name: "Quick", source: "project" },
    selectedAvailable: true,
    error: null,
  });
  client.getAgentModelPresetContext.mockResolvedValueOnce({
    agentId: "a",
    provider: "omp",
    presets: [{ name: "Quick", model: "fast", source: "project" }],
    selectedModelPreset: { name: "Quick", source: "project" },
    selectedAvailable: false,
    error: null,
  });
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  function Probe({ revision }: { revision: number }) {
    const choices = useProviderModelPresetChoices({
      serverId: "host",
      provider: "omp",
      cwd: "/worktree",
      contextAgentId: "a",
      contextRevision: revision,
      supportsSelection: true,
      enabled: true,
    });
    return (
      <ProviderModelPresets
        provider="omp"
        choices={choices}
        models={models}
        controls={{
          apply: vi.fn(),
          selected: { provider: "omp", name: "Quick", source: "project" },
        }}
        renderRow={(row) => (
          <button data-testid={row.testID} aria-selected={row.selected}>
            {row.name}
          </button>
        )}
      />
    );
  }
  const view = render(
    <QueryClientProvider client={queryClient}>
      <Probe revision={1} />
    </QueryClientProvider>,
  );
  expect(
    (await screen.findByTestId("provider-model-preset-omp-Quick")).getAttribute("aria-selected"),
  ).toBe("true");
  view.rerender(
    <QueryClientProvider client={queryClient}>
      <Probe revision={2} />
    </QueryClientProvider>,
  );
  await waitFor(() =>
    expect(
      screen.getByTestId("provider-model-preset-omp-Quick").getAttribute("aria-selected"),
    ).toBe("false"),
  );
  expect(screen.getByText("modelSelector.presetContextUnavailable")).toBeTruthy();
  expect(client.listProviderPresets).not.toHaveBeenCalled();
});

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

it("shows vendor-qualified preset rows after shared-cache invalidation and applies roles and fallbacks", async () => {
  const { apply, queryClient } = showPicker();
  await waitFor(() => expect(client.listProviderPresets).toHaveBeenCalledTimes(1));
  expect(screen.queryByText("modelSelector.modelPresets")).toBeNull();
  presets = [
    {
      name: "Quick",
      model: '["test","fast"]',
      source: "ait",
      editable: true,
      featureValues: {
        "modelRole.worker": "unset",
        "modelFallback.default": ["test/fast:off"],
      },
    },
  ];
  await act(async () => {
    await invalidatePresetCaches(queryClient, "host", "omp");
  });
  fireEvent.click(await screen.findByRole("button", { name: "Quick test / fast" }));
  expect(apply).toHaveBeenCalledExactlyOnceWith(
    "omp",
    {
      modelId: '["test","fast"]',
      thinkingOptionId: null,
      featureValues: {
        "modelRole.advisor": "native",
        "modelRole.worker": "unset",
        "modelFallback.advisor": "native",
        "modelFallback.worker": "native",
        "modelFallback.default": ["test/fast:off"],
      },
    },
    expect.objectContaining({ name: "Quick", source: "ait" }),
  );
  expect(client.listProviderFeatures).toHaveBeenCalledWith({ provider: "omp", cwd: "/project" });
});

it("also shows Ait-managed presets for providers without separate role features", async () => {
  presets = [{ name: "Quick", model: "fast", source: "ait", editable: true }];
  features = [];
  const { apply } = showPicker();
  const row = await screen.findByTestId("provider-model-preset-omp-Quick");
  fireEvent.click(row);
  expect(apply).toHaveBeenCalledExactlyOnceWith(
    "omp",
    {
      modelId: '["test","fast"]',
      thinkingOptionId: null,
      featureValues: {},
    },
    expect.objectContaining({ name: "Quick", source: "ait" }),
  );
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
  "canonicalizes a known model alias with its supported/default thinking (%s)",
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
      expect.objectContaining({
        modelId: '["test","fast"]',
        thinkingOptionId: thinkingOptionId ?? null,
      }),
      expect.objectContaining({ name: "Available", source: "global" }),
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
