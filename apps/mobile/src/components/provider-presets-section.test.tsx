// @vitest-environment jsdom
import React from "react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { ProviderPreset, ProviderPresetDescriptor } from "@ait/protocol/messages";
import { ProviderPresetsSection } from "./provider-presets-section";

const client = {
  describeProviderPresets: vi.fn(),
  listProviderPresets: vi.fn(),
  saveProviderPreset: vi.fn(),
  deleteProviderPreset: vi.fn(),
};

vi.mock("@/runtime/host-runtime", () => ({
  useHostRuntimeClient: () => client,
  useHostRuntimeIsConnected: () => true,
}));
vi.mock("@/contexts/toast-context", () => ({
  useToast: () => ({ error: vi.fn() }),
}));
vi.mock("@/utils/error-messages", () => ({
  toErrorMessage: (error: unknown) => String(error),
}));
vi.mock("react-native", () => ({
  Text: ({ children, testID }: { children: React.ReactNode; testID?: string }) => (
    <span data-testid={testID}>{children}</span>
  ),
  View: ({ children, testID }: { children: React.ReactNode; testID?: string }) => (
    <div data-testid={testID}>{children}</div>
  ),
  StyleSheet: { hairlineWidth: 1 },
}));
vi.mock("react-native-unistyles", () => ({
  StyleSheet: {
    create: (factory: (theme: unknown) => unknown) =>
      factory({
        colors: { foreground: "#fff", foregroundMuted: "#888", destructive: "#f00" },
        fontSize: { sm: 12, base: 14 },
        fontWeight: { medium: "500" },
        spacing: { 2: 8, 3: 12, 4: 16, 6: 24 },
        borderRadius: { lg: 8 },
        border: "#333",
      }),
  },
  useUnistyles: () => ({ theme: { colors: { foregroundMuted: "#888" } } }),
}));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));
vi.mock("@/components/ui/button", () => ({
  Button: ({
    children,
    onPress,
    disabled,
    leftIcon: _leftIcon,
    accessibilityLabel,
    ...rest
  }: {
    children?: React.ReactNode;
    onPress: () => void;
    disabled?: boolean;
    leftIcon?: unknown;
    accessibilityLabel?: string;
  }) => (
    <button
      disabled={disabled}
      onClick={onPress}
      aria-label={accessibilityLabel}
      data-testid={(rest as { testID?: string }).testID}
    >
      {children}
    </button>
  ),
}));
vi.mock("@/components/adaptive-modal-sheet", () => ({
  AdaptiveModalSheet: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
  AdaptiveTextInput: ({
    initialValue,
    onChangeText,
    editable,
  }: {
    initialValue: string;
    onChangeText: (value: string) => void;
    editable?: boolean;
  }) => {
    // `editable` defaults to true on the real input; only an explicit false makes it read-only.
    const readOnly = editable === false;
    return (
      <input
        defaultValue={initialValue}
        readOnly={readOnly}
        aria-readonly={readOnly}
        onChange={(event) => onChangeText(event.target.value)}
      />
    );
  },
}));

const queryClient = new QueryClient({
  defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
});

/**
 * Render inside a query provider because the section reads the shared preset store and saving
 * invalidates it.
 */
function renderSection(ui: React.ReactElement) {
  return render(<QueryClientProvider client={queryClient}>{ui}</QueryClientProvider>);
}

beforeEach(() => {
  vi.stubGlobal("React", React);
  for (const fn of Object.values(client)) fn.mockReset();
  // The store is cached per host and provider, so each case starts from an empty cache.
  queryClient.clear();
});
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

function descriptor(overrides: Partial<ProviderPresetDescriptor> = {}): ProviderPresetDescriptor {
  return {
    backend: "ait",
    canCreate: true,
    canEdit: true,
    canDelete: true,
    canRename: true,
    ...overrides,
  };
}

function preset(name: string): ProviderPreset {
  return { name, model: "claude-opus-5" };
}

async function ready(described: ProviderPresetDescriptor, presets: ProviderPreset[] = []) {
  // The store is cached per host and provider; a scenario must not see the previous answer.
  queryClient.clear();
  client.describeProviderPresets.mockResolvedValue({ descriptor: described });
  client.listProviderPresets.mockResolvedValue({ presets });
}

it("lists presets and offers the same controls for a native and an Ait backend", async () => {
  for (const backend of ["native", "ait"] as const) {
    await ready(descriptor({ backend }), [preset("team")]);
    const view = renderSection(<ProviderPresetsSection provider="claude" serverId="host-a" />);
    await waitFor(() => expect(screen.getByText("team")).toBeTruthy());
    expect(screen.getByTestId("provider-preset-team")).toBeTruthy();
    // Identical capability bits must render identical controls, whatever the store is.
    expect(screen.getByTestId("provider-presets-add")).toBeTruthy();
    view.unmount();
  }
});

it("disables exactly the operations the descriptor withholds", async () => {
  await ready(
    descriptor({
      backend: "native",
      canCreate: false,
      canEdit: false,
      canDelete: false,
      canRename: false,
      reason: "This OMP version does not expose the modelpreset command",
    }),
    [preset("team")],
  );
  renderSection(<ProviderPresetsSection provider="omp" serverId="host-a" />);
  await waitFor(() => expect(screen.getByText("team")).toBeTruthy());
  expect(screen.queryByTestId("provider-presets-add")).toBeNull();
  expect(screen.getByTestId("provider-presets-reason").textContent).toContain("modelpreset");
  // A read-only row has neither edit nor delete affordances.
  expect(screen.queryByLabelText("settings.providers.presets.edit")).toBeNull();
  expect(screen.queryByLabelText("settings.providers.presets.remove")).toBeNull();
});

it("shows a create entry in the empty state and saves through the unified request", async () => {
  await ready(descriptor(), []);
  client.saveProviderPreset.mockResolvedValue({ saved: "team" });
  renderSection(<ProviderPresetsSection provider="claude" serverId="host-a" />);
  await waitFor(() => expect(screen.getByTestId("provider-presets-empty")).toBeTruthy());
  fireEvent.click(screen.getByTestId("provider-presets-empty-create"));
  await waitFor(() => expect(screen.getByTestId("provider-preset-save")).toBeTruthy());
  fireEvent.change(screen.getAllByRole("textbox")[0], { target: { value: "team" } });
  fireEvent.change(screen.getAllByRole("textbox")[1], {
    target: { value: "claude-opus-5" },
  });
  fireEvent.click(screen.getByTestId("provider-preset-save"));
  await waitFor(() =>
    // A create carries no original name; the write also repeats the list's cwd.
    expect(client.saveProviderPreset).toHaveBeenCalledWith(
      "claude",
      { name: "team", model: "claude-opus-5" },
      undefined,
      {},
    ),
  );
});

it("edits the name and sends the original when the descriptor allows renaming", async () => {
  await ready(descriptor({ canRename: true }), [preset("team")]);
  client.saveProviderPreset.mockResolvedValue({ saved: "squad" });
  renderSection(<ProviderPresetsSection provider="claude" serverId="host-a" />);
  await waitFor(() => expect(screen.getByText("team")).toBeTruthy());
  fireEvent.click(screen.getByLabelText("settings.providers.presets.edit"));
  await waitFor(() => expect(screen.getByTestId("provider-preset-save")).toBeTruthy());

  const nameInput = screen.getAllByRole("textbox")[0] as HTMLInputElement;
  expect(nameInput.readOnly).toBe(false);
  expect(nameInput.value).toBe("team");
  // No read-only explanation is shown when the operation is available.
  expect(screen.queryByText("settings.providers.presets.noRename")).toBeNull();

  fireEvent.change(nameInput, { target: { value: "squad" } });
  fireEvent.click(screen.getByTestId("provider-preset-save"));
  await waitFor(() =>
    expect(client.saveProviderPreset).toHaveBeenCalledWith(
      "claude",
      { name: "squad", model: "claude-opus-5" },
      undefined,
      { original: "team" },
    ),
  );
});

it("keeps the name read-only and explains why when renaming is withheld", async () => {
  await ready(
    descriptor({ backend: "native", canRename: false, reason: "OMP has no rename command" }),
    [preset("team")],
  );
  renderSection(<ProviderPresetsSection provider="omp" serverId="host-a" />);
  await waitFor(() => expect(screen.getByText("team")).toBeTruthy());
  fireEvent.click(screen.getByLabelText("settings.providers.presets.edit"));
  await waitFor(() => expect(screen.getByTestId("provider-preset-save")).toBeTruthy());

  const nameInput = screen.getAllByRole("textbox")[0] as HTMLInputElement;
  expect(nameInput.readOnly).toBe(true);
  expect(nameInput.value).toBe("team");
  // The sheet repeats the descriptor's reason rather than offering a dead control; the section
  // header shows the same reason, so the text appears twice.
  expect(screen.getAllByText("OMP has no rename command")).toHaveLength(2);
});

it("deletes through the same request and refreshes the list", async () => {
  await ready(descriptor(), [preset("team")]);
  client.deleteProviderPreset.mockResolvedValue({ deleted: "team" });
  renderSection(<ProviderPresetsSection provider="claude" serverId="host-a" />);
  await waitFor(() => expect(screen.getByText("team")).toBeTruthy());
  fireEvent.click(screen.getByLabelText("settings.providers.presets.remove"));
  await waitFor(() =>
    expect(client.deleteProviderPreset).toHaveBeenCalledWith("claude", "team", undefined),
  );
  await waitFor(() => expect(client.listProviderPresets.mock.calls.length).toBeGreaterThan(1));
});

it("refuses a create that reuses an existing name instead of overwriting it", async () => {
  await ready(descriptor(), [preset("team")]);
  renderSection(<ProviderPresetsSection provider="claude" serverId="host-a" />);
  await waitFor(() => expect(screen.getByText("team")).toBeTruthy());
  fireEvent.click(screen.getByTestId("provider-presets-add"));
  await waitFor(() => expect(screen.getByTestId("provider-preset-save")).toBeTruthy());

  fireEvent.change(screen.getAllByRole("textbox")[0], { target: { value: "team" } });
  fireEvent.change(screen.getAllByRole("textbox")[1], {
    target: { value: "claude-opus-5" },
  });
  // The store would upsert, replacing the existing preset's roles and thinking level; the create
  // entry explains the collision and sends nothing.
  expect(screen.getByTestId("provider-preset-name-taken")).toBeTruthy();
  expect(screen.getByTestId("provider-preset-save").hasAttribute("disabled")).toBe(true);
  fireEvent.click(screen.getByTestId("provider-preset-save"));
  expect(client.saveProviderPreset).not.toHaveBeenCalled();

  // Renaming that record is still allowed, because the edit names it as `original`.
  fireEvent.change(screen.getAllByRole("textbox")[0], { target: { value: "squad" } });
  expect(screen.queryByTestId("provider-preset-name-taken")).toBeNull();
});

it("never branches on the provider identity to decide which controls exist", async () => {
  // Same capabilities plus the same list must render the same tree for two different providers.
  await ready(descriptor({ backend: "native" }), [preset("team")]);
  const native = renderSection(<ProviderPresetsSection provider="omp" serverId="host-a" />);
  await waitFor(() => expect(screen.getByText("team")).toBeTruthy());
  const nativeMarkup = native.container.innerHTML;
  native.unmount();

  await ready(descriptor({ backend: "ait" }), [preset("team")]);
  const managed = renderSection(<ProviderPresetsSection provider="claude" serverId="host-a" />);
  await waitFor(() => expect(screen.getByText("team")).toBeTruthy());
  expect(managed.container.innerHTML).toBe(nativeMarkup);
});
