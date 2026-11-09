// @vitest-environment jsdom
import React from "react";
import { beforeEach, afterEach, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { ProviderDiagnosticSheet } from "./provider-diagnostic-sheet";
import {
  providerPresetsQueryKey,
  useProviderPresets,
  useProviderPresetFeatures,
} from "@/data/provider-presets";

const fixture = vi.hoisted(() => ({
  list: vi.fn(),
  describe: vi.fn(),
  features: vi.fn(),
  refresh: vi.fn(),
  t: (key: string) => key,
}));
vi.mock("expo-clipboard", () => ({ setStringAsync: vi.fn() }));
vi.mock("@/runtime/host-runtime", () => ({
  useHostRuntimeClient: () => ({
    listProviderPresets: fixture.list,
    describeProviderPresets: fixture.describe,
    listProviderFeatures: fixture.features,
  }),
  useHostRuntimeIsConnected: () => true,
}));
vi.mock("@/hooks/use-providers-snapshot", () => ({
  providersSnapshotQueryKey: (server: string, cwd: string) => ["snapshot", server, cwd],
  useProvidersSnapshot: () => ({
    entries: [{ provider: "omp", status: "ready", models: [] }],
    refresh: fixture.refresh,
    refetchIfStale: vi.fn(),
    isRefreshing: false,
  }),
}));
vi.mock("@/hooks/use-daemon-config", () => ({
  useDaemonConfig: () => ({ config: null, patchConfig: vi.fn() }),
}));
vi.mock("@/stores/session-store", () => ({
  useSessionStore: (fn: (value: unknown) => unknown) =>
    fn({ sessions: { host: { serverInfo: { features: { providerConfiguration: false } } } } }),
}));
vi.mock("@/contexts/toast-context", () => ({ useToast: () => ({ error: vi.fn() }) }));
vi.mock("@/constants/layout", () => ({ useIsCompactFormFactor: () => false }));
vi.mock("@/constants/platform", () => ({ isWeb: true }));
vi.mock("@/styles/settings", () => ({ settingsStyles: {} }));
vi.mock("@/utils/provider-definitions", () => ({ resolveProviderLabel: () => "OMP" }));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: fixture.t }) }));
vi.mock("react-native", () => ({
  Text: ({ children, testID }: { children: React.ReactNode; testID?: string }) => (
    <span data-testid={testID}>{children}</span>
  ),
  View: ({ children, testID }: { children: React.ReactNode; testID?: string }) => (
    <div data-testid={testID}>{children}</div>
  ),
  Pressable: () => null,
}));
vi.mock("react-native-unistyles", () => ({
  StyleSheet: {
    create: (fn: (theme: unknown) => unknown) =>
      fn({
        colors: {},
        fontFamily: {},
        fontWeight: {},
        fontSize: {},
        spacing: {},
        borderRadius: {},
      }),
  },
  useUnistyles: () => ({ theme: { colors: {}, iconSize: { sm: 16 } } }),
}));
vi.mock("@/components/ui/scrollable-code-surface", () => ({
  SurfaceCard: () => null,
  ScrollableCodeSurface: () => null,
}));
vi.mock("@/components/ui/loading-spinner", () => ({ LoadingSpinner: () => null }));
vi.mock("@/components/ui/button", () => ({
  Button: ({
    children,
    onPress,
    disabled,
  }: {
    children: React.ReactNode;
    onPress: () => void;
    disabled?: boolean;
  }) => (
    <button disabled={disabled} onClick={onPress}>
      {children}
    </button>
  ),
}));
vi.mock("@/components/adaptive-modal-sheet", () => ({
  AdaptiveModalSheet: ({
    children,
    footer,
    visible,
  }: {
    children: React.ReactNode;
    footer?: React.ReactNode;
    visible: boolean;
  }) =>
    visible ? (
      <div>
        {children}
        {footer}
      </div>
    ) : null,
  AdaptiveTextInput: () => null,
}));
vi.mock("./provider-presets-section", () => ({
  ProviderPresetsSection: (props: { provider: string; serverId: string; cwd?: string }) => {
    const state = useProviderPresets({ ...props, cwd: props.cwd });
    useProviderPresetFeatures(props.serverId, props.provider, props.cwd);
    return (
      <div>
        {state.error}
        <span data-testid="refreshed-presets">
          {state.snapshot?.presets.map((entry) => entry.name).join(",")}
        </span>
      </div>
    );
  },
}));
let queryClient: QueryClient;
beforeEach(() => {
  queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  fixture.list.mockReset();
  fixture.describe.mockReset();
  fixture.features.mockReset();
  fixture.refresh.mockReset();
  fixture.describe.mockResolvedValue({
    descriptor: {
      backend: "native",
      canCreate: true,
      canEdit: true,
      canDelete: true,
      canRename: false,
    },
  });
  fixture.features.mockResolvedValue({ features: [] });
  fixture.refresh.mockResolvedValue(undefined);
});
afterEach(() => {
  cleanup();
  queryClient.clear();
});
function mount() {
  return render(
    <QueryClientProvider client={queryClient}>
      <ProviderDiagnosticSheet
        provider="omp"
        serverId="host"
        cwd="/project"
        visible
        onClose={() => {}}
      />
    </QueryClientProvider>,
  );
}

it("refresh retries a failed preset read and refreshes the exact project plus its role choices", async () => {
  fixture.list.mockRejectedValueOnce(new Error("Store offline")).mockResolvedValue({
    presets: [{ name: "recovered", model: "model", source: "global", editable: true }],
  });
  const foreign = { marker: "other project" };
  queryClient.setQueryData(providerPresetsQueryKey("host", "omp", "/other"), foreign);
  mount();
  await screen.findByText("Store offline");
  fireEvent.click(screen.getByText("settings.providers.diagnostic.refresh"));
  await waitFor(() =>
    expect(screen.getByTestId("refreshed-presets").textContent).toBe("recovered"),
  );
  expect(fixture.refresh).toHaveBeenCalledWith(["omp"]);
  expect(fixture.list).toHaveBeenLastCalledWith("omp", "/project");
  expect(fixture.features).toHaveBeenCalledTimes(2);
  expect(queryClient.getQueryData(providerPresetsQueryKey("host", "omp", "/other"))).toBe(foreign);
  expect(screen.getByText("providerControl.presetsRefreshed")).toBeTruthy();
});

it("shows a preset-specific failure even when the model refresh succeeds", async () => {
  fixture.list
    .mockResolvedValueOnce({ presets: [] })
    .mockResolvedValue({ presets: [], error: "Project unreadable" });
  mount();
  await waitFor(() => expect(fixture.list).toHaveBeenCalledOnce());
  fireEvent.click(screen.getByText("settings.providers.diagnostic.refresh"));
  await screen.findByText("providerControl.modelsRefreshed");
  expect(screen.getByTestId("provider-settings-refresh-result").textContent).toContain(
    "providerControl.presetsRefreshFailed",
  );
  expect(screen.queryByText("providerControl.presetsRefreshed")).toBeNull();
});

it("picks up externally changed presets and remains busy until the list read finishes", async () => {
  let resolve!: (value: unknown) => void;
  fixture.list.mockResolvedValueOnce({ presets: [] }).mockImplementationOnce(
    () =>
      new Promise((done) => {
        resolve = done;
      }),
  );
  mount();
  await waitFor(() => expect(fixture.list).toHaveBeenCalledOnce());
  fireEvent.click(screen.getByText("settings.providers.diagnostic.refresh"));
  await waitFor(() => expect(fixture.list).toHaveBeenCalledTimes(2));
  expect(
    (screen.getByText("settings.providers.diagnostic.refreshing") as HTMLButtonElement).disabled,
  ).toBe(true);
  resolve({ presets: [{ name: "external", model: "model", source: "project", editable: false }] });
  await waitFor(() => expect(screen.getByTestId("refreshed-presets").textContent).toBe("external"));
});

it("refreshes role candidates only after the model catalog acknowledgement", async () => {
  let resolveModels!: () => void;
  fixture.refresh.mockImplementationOnce(() => new Promise<void>((done) => { resolveModels = done; }));
  fixture.list.mockResolvedValue({ presets: [] });
  mount(); await waitFor(() => expect(fixture.features).toHaveBeenCalledOnce());
  fireEvent.click(screen.getByText("settings.providers.diagnostic.refresh"));
  await waitFor(() => expect(fixture.list).toHaveBeenCalledTimes(2));
  expect(fixture.features).toHaveBeenCalledOnce();
  resolveModels();
  await waitFor(() => expect(fixture.features).toHaveBeenCalledTimes(2));
});

it("reports an acknowledged catalog containing a provider error as a model failure", async () => {
  fixture.list.mockResolvedValue({ presets: [] });
  queryClient.setQueryData(["snapshot", "host", "/project"], { entries: [{ provider: "omp", status: "error", error: "Model probe failed" }] });
  mount(); await waitFor(() => expect(fixture.list).toHaveBeenCalledOnce());
  fireEvent.click(screen.getByText("settings.providers.diagnostic.refresh"));
  await screen.findByText("providerControl.presetsRefreshed");
  expect(screen.getByTestId("provider-settings-refresh-result").textContent).toContain("providerControl.modelsRefreshFailed");
  expect(screen.getByTestId("provider-settings-refresh-result").textContent).toContain("Model probe failed");
});
