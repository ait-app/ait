// @vitest-environment jsdom
import React from "react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { ProviderPresetEntry, ProviderPresetDescriptor } from "@ait/protocol/messages";
import { ProviderPresetsSection } from "./provider-presets-section";

vi.mock("@/constants/layout", () => ({ useIsCompactFormFactor: () => false }));
vi.mock("@/i18n/i18next", () => ({ i18n: { t: (key: string) => key } }));
vi.mock("@/components/ui/select-field", () => ({
  SelectField: ({
    testID,
    options,
    value,
    onChange,
    selectedDisplay,
    disabled,
  }: {
    testID: string;
    options: { id: string; value: string; label: string }[];
    value: string;
    onChange: (value: string) => void;
    selectedDisplay: { label: string } | null;
    disabled?: boolean;
  }) => (
    <select
      data-testid={testID}
      disabled={disabled}
      aria-label={selectedDisplay?.label}
      value={value}
      onChange={(event) => onChange(event.target.value)}
    >
      {options.map((option) => (
        <option key={option.id} value={option.value}>
          {option.label}
        </option>
      ))}
    </select>
  ),
}));
const client = {
  describeProviderPresets: vi.fn(),
  listProviderPresets: vi.fn(),
  saveProviderPreset: vi.fn(),
  deleteProviderPreset: vi.fn(),
  listProviderFeatures: vi.fn(),
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
  AdaptiveModalSheet: ({
    children,
    footer,
    testID,
  }: {
    children: React.ReactNode;
    footer?: React.ReactNode;
    testID?: string;
  }) => (
    <div data-testid={testID}>
      {children}
      {footer}
    </div>
  ),
  AdaptiveTextInput: ({
    initialValue,
    placeholder,
    onChangeText,
    editable,
  }: {
    initialValue: string;
    placeholder?: string;
    onChangeText: (value: string) => void;
    editable?: boolean;
  }) => {
    // `editable` defaults to true on the real input; only an explicit false makes it read-only.
    const readOnly = editable === false;
    return (
      <input
        defaultValue={initialValue}
        placeholder={placeholder}
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
  return render(
    <QueryClientProvider client={queryClient}>
      {React.cloneElement(ui as React.ReactElement<{ models?: unknown }>, {
        models: (ui.props as { models?: unknown }).models ?? [
          { provider: "claude", id: "claude-opus-5", label: "Opus" },
        ],
      })}
    </QueryClientProvider>,
  );
}

beforeEach(() => {
  vi.stubGlobal("React", React);
  for (const fn of Object.values(client)) fn.mockReset();
  // The store is cached per host and provider, so each case starts from an empty cache.
  queryClient.clear();
  client.listProviderFeatures.mockResolvedValue({ features: [] });
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

function preset(name: string): ProviderPresetEntry {
  return { name, model: "claude-opus-5", source: "global", editable: true };
}

async function ready(described: ProviderPresetDescriptor, presets: ProviderPresetEntry[] = []) {
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
  fireEvent.click(screen.getByLabelText("providerControl.viewPreset"));
  expect(screen.getByText("This OMP version does not expose the modelpreset command")).toBeTruthy();
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
  fireEvent.change(screen.getByTestId("provider-preset-model-options"), {
    target: { value: "claude-opus-5" },
  });
  fireEvent.click(screen.getByTestId("provider-preset-save"));
  await waitFor(() =>
    // A create carries no original name; the write also repeats the list's cwd.
    expect(client.saveProviderPreset).toHaveBeenCalledWith(
      "claude",
      { name: "team", model: "claude-opus-5", featureValues: {} },
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
      { name: "squad", model: "claude-opus-5", featureValues: {} },
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
  expect(screen.getByTestId("preset-name-readonly-reason").textContent).toBe(
    "settings.providers.presets.noRename",
  );
  expect(screen.queryByText("providerControl.roleRoutingHint")).toBeNull();
});

it("deletes through the same request and refreshes the list", async () => {
  await ready(descriptor(), [preset("team")]);
  client.deleteProviderPreset.mockResolvedValue({ deleted: "team" });
  renderSection(<ProviderPresetsSection provider="claude" serverId="host-a" />);
  await waitFor(() => expect(screen.getByText("team")).toBeTruthy());
  fireEvent.click(screen.getByLabelText("settings.providers.presets.remove"));
  expect(client.deleteProviderPreset).not.toHaveBeenCalled();
  fireEvent.click(screen.getByTestId("provider-preset-confirm-delete"));
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
  fireEvent.change(screen.getByTestId("provider-preset-model-options"), {
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
  const nativeActions = Array.from(native.container.querySelectorAll("button")).map(
    (button) => button.getAttribute("aria-label") ?? button.textContent,
  );
  native.unmount();
  await ready(descriptor({ backend: "ait" }), [preset("team")]);
  const managed = renderSection(<ProviderPresetsSection provider="claude" serverId="host-a" />);
  await waitFor(() => expect(screen.getByText("team")).toBeTruthy());
  expect(
    Array.from(managed.container.querySelectorAll("button")).map(
      (button) => button.getAttribute("aria-label") ?? button.textContent,
    ),
  ).toEqual(nativeActions);
});

it("offers catalog models and reasoning levels while allowing role overrides to be edited", async () => {
  await ready(descriptor(), []);
  client.listProviderFeatures.mockResolvedValue({
    features: [
      {
        type: "select",
        id: "modelRole.advisor",
        label: "Advisor",
        value: "native",
        options: [
          { id: "native", label: "Native" },
          { id: "unset", label: "Unset" },
          { id: "opus", label: "Opus" },
        ],
      },
    ],
  });
  renderSection(
    <ProviderPresetsSection
      provider="claude"
      serverId="host-a"
      models={[
        {
          provider: "claude",
          id: "opus",
          label: "Opus",
          thinkingOptions: [{ id: "high", label: "High" }],
        },
      ]}
    />,
  );
  await waitFor(() => expect(screen.getByTestId("provider-presets-empty-create")).toBeTruthy());
  fireEvent.click(screen.getByTestId("provider-presets-empty-create"));
  fireEvent.change(screen.getAllByRole("textbox")[0], { target: { value: "catalog" } });
  fireEvent.change(screen.getByTestId("provider-preset-model-options"), {
    target: { value: "opus" },
  });
  fireEvent.change(screen.getByTestId("provider-preset-thinking-options"), {
    target: { value: "high" },
  });
  expect(screen.getByRole("combobox", { name: "Opus" })).toBeTruthy();
  expect(screen.getByRole("combobox", { name: "schedules.thinking.high" })).toBeTruthy();
  fireEvent.change(screen.getByTestId("provider-preset-add-role"), {
    target: { value: "modelRole.advisor" },
  });
  expect(screen.getByTestId(/^preset-role-name-/).textContent).toBe("ADVISOR");
  expect(screen.getByTestId(/^preset-role-name-/).tagName).toBe("SPAN");
  expect(screen.queryByTestId("provider-preset-roles")).toBeNull();
  fireEvent.click(screen.getByTestId("provider-preset-save"));
  await waitFor(() =>
    expect(client.saveProviderPreset).toHaveBeenCalledWith(
      "claude",
      {
        name: "catalog",
        model: "opus",
        thinkingOptionId: "high",
        featureValues: { "modelRole.advisor": "native" },
      },
      undefined,
      {},
    ),
  );
});

it("shows project scope and prevents global writes to an overriding project preset", async () => {
  await ready(descriptor({ backend: "native" }), [
    { ...preset("team"), source: "project", editable: false },
  ]);
  renderSection(<ProviderPresetsSection provider="omp" serverId="host-a" cwd="/repo/project" />);
  await waitFor(() => expect(screen.getByText("team")).toBeTruthy());
  expect(screen.getByTestId("provider-presets-scope").textContent).toBe("/repo/project");
  expect(client.listProviderPresets).toHaveBeenCalledWith("omp", "/repo/project");
  expect(
    (screen.getByLabelText("settings.providers.presets.remove") as HTMLButtonElement).disabled,
  ).toBe(true);
  fireEvent.click(screen.getByLabelText("providerControl.viewPreset"));
  expect(screen.queryByTestId("provider-preset-save")).toBeNull();
  expect(screen.getByText("providerControl.projectPresetReadOnly")).toBeTruthy();
  expect(client.saveProviderPreset).not.toHaveBeenCalled();
  expect(client.deleteProviderPreset).not.toHaveBeenCalled();
});

it("keeps native entries read-only when the list has no ownership metadata", async () => {
  await ready(descriptor({ backend: "native" }), [{ name: "team", model: "opus" }]);
  renderSection(<ProviderPresetsSection provider="omp" serverId="host-a" />);
  await screen.findByText("team");
  expect(
    (screen.getByLabelText("settings.providers.presets.remove") as HTMLButtonElement).disabled,
  ).toBe(true);
  fireEvent.click(screen.getByLabelText("providerControl.viewPreset"));
  expect(screen.queryByTestId("provider-preset-save")).toBeNull();
  expect(client.deleteProviderPreset).not.toHaveBeenCalled();
  expect(client.saveProviderPreset).not.toHaveBeenCalled();
});

it("disables an open editor while ownership refresh is delayed, then stays read-only on project ownership", async () => {
  await ready(descriptor({ backend: "native" }), [preset("team")]);
  renderSection(<ProviderPresetsSection provider="omp" serverId="host-a" />);
  await screen.findByText("team");
  fireEvent.click(screen.getByLabelText("settings.providers.presets.edit"));
  const save = screen.getByTestId("provider-preset-save") as HTMLButtonElement;
  let resolve: (value: { presets: ProviderPresetEntry[] }) => void = () => undefined;
  client.listProviderPresets.mockImplementationOnce(
    () =>
      new Promise((done) => {
        resolve = done;
      }),
  );
  let refreshed: Promise<void>;
  await act(async () => {
    refreshed = queryClient.invalidateQueries({ queryKey: ["provider-presets"] });
  });
  await waitFor(() => expect(save.disabled).toBe(true));
  fireEvent.click(save);
  expect(client.saveProviderPreset).not.toHaveBeenCalled();
  await act(async () => {
    resolve({ presets: [{ ...preset("team"), source: "project", editable: false }] });
    await refreshed;
  });
  expect(save.disabled).toBe(true);
  fireEvent.click(save);
  expect(client.saveProviderPreset).not.toHaveBeenCalled();
});

it("keeps cached rows and an open editor read-only after an ownership query fails", async () => {
  await ready(descriptor({ backend: "native" }), [preset("team")]);
  renderSection(<ProviderPresetsSection provider="omp" serverId="host-a" />);
  await screen.findByText("team");
  fireEvent.click(screen.getByLabelText("settings.providers.presets.edit"));
  client.listProviderPresets.mockRejectedValueOnce(new Error("Scope read failed"));
  await act(async () => {
    await queryClient.invalidateQueries({ queryKey: ["provider-presets"] });
  });
  await screen.findByText("Scope read failed");
  const save = screen.getByTestId("provider-preset-save") as HTMLButtonElement;
  expect(save.disabled).toBe(true);
  fireEvent.click(save);
  fireEvent.click(screen.getByLabelText("settings.providers.presets.remove"));
  expect(client.saveProviderPreset).not.toHaveBeenCalled();
  expect(client.deleteProviderPreset).not.toHaveBeenCalled();
});

it.each(["save", "delete"] as const)(
  "rechecks ownership before %s when a project override appears during editing",
  async (operation) => {
    await ready(descriptor({ backend: "native" }), [preset("team")]);
    renderSection(<ProviderPresetsSection provider="omp" serverId="host-a" />);
    await screen.findByText("team");
    if (operation === "save")
      fireEvent.click(screen.getByLabelText("settings.providers.presets.edit"));
    client.listProviderPresets.mockResolvedValueOnce({
      presets: [{ ...preset("team"), source: "project", editable: false }],
    });
    fireEvent.click(
      operation === "save"
        ? screen.getByTestId("provider-preset-save")
        : screen.getByLabelText("settings.providers.presets.remove"),
    );
    if (operation === "delete")
      fireEvent.click(screen.getByTestId("provider-preset-confirm-delete"));
    await waitFor(() => expect(client.listProviderPresets).toHaveBeenCalledTimes(2));
    await act(async () => {});
    expect(client.saveProviderPreset).not.toHaveBeenCalled();
    expect(client.deleteProviderPreset).not.toHaveBeenCalled();
  },
);

it.each(["save", "delete"] as const)(
  "disables mutations when the final %s ownership check fails",
  async (operation) => {
    await ready(descriptor({ backend: "native" }), [preset("team")]);
    renderSection(<ProviderPresetsSection provider="omp" serverId="host-a" />);
    await screen.findByText("team");
    if (operation === "save")
      fireEvent.click(screen.getByLabelText("settings.providers.presets.edit"));
    client.listProviderPresets.mockRejectedValueOnce(new Error("Ownership check failed"));
    fireEvent.click(
      operation === "save"
        ? screen.getByTestId("provider-preset-save")
        : screen.getByLabelText("settings.providers.presets.remove"),
    );
    if (operation === "delete")
      fireEvent.click(screen.getByTestId("provider-preset-confirm-delete"));
    await waitFor(() =>
      expect(
        (screen.getByLabelText("settings.providers.presets.remove") as HTMLButtonElement).disabled,
      ).toBe(true),
    );
    if (operation === "save")
      expect((screen.getByTestId("provider-preset-save") as HTMLButtonElement).disabled).toBe(true);
    expect(client.saveProviderPreset).not.toHaveBeenCalled();
    expect(client.deleteProviderPreset).not.toHaveBeenCalled();
  },
);

it("requires confirmation, allows cancel and shows deletion progress and completion", async () => {
  await ready(descriptor(), [preset("team")]);
  let resolve!: (value: unknown) => void;
  client.deleteProviderPreset.mockImplementation(
    () =>
      new Promise((done) => {
        resolve = done;
      }),
  );
  renderSection(<ProviderPresetsSection provider="claude" serverId="host-a" />);
  await screen.findByText("team");
  fireEvent.click(screen.getByLabelText("settings.providers.presets.remove"));
  expect(client.deleteProviderPreset).not.toHaveBeenCalled();
  expect(screen.getByTestId("provider-preset-delete-confirmation").textContent).toContain("team");
  fireEvent.click(screen.getByText("common.actions.cancel"));
  expect(screen.queryByTestId("provider-preset-confirm-delete")).toBeNull();
  expect(client.deleteProviderPreset).not.toHaveBeenCalled();
  fireEvent.click(screen.getByLabelText("settings.providers.presets.remove"));
  fireEvent.click(screen.getByTestId("provider-preset-confirm-delete"));
  await waitFor(() => expect(client.deleteProviderPreset).toHaveBeenCalledOnce());
  expect((screen.getByTestId("provider-preset-confirm-delete") as HTMLButtonElement).disabled).toBe(
    true,
  );
  fireEvent.click(screen.getByTestId("provider-preset-confirm-delete"));
  expect(client.deleteProviderPreset).toHaveBeenCalledOnce();
  await act(async () => {
    resolve({ deleted: "team" });
  });
  await waitFor(() => expect(screen.queryByTestId("provider-preset-confirm-delete")).toBeNull());
  expect(screen.getByTestId("provider-presets-notice")).toBeTruthy();
});

it("creates and reopens a native preset using only model, role and effort choices", async () => {
  const rows: ProviderPresetEntry[] = [];
  await ready(descriptor({ backend: "native" }), rows);
  client.listProviderPresets.mockImplementation(async () => ({ presets: [...rows] }));
  client.listProviderFeatures.mockResolvedValue({
    features: [
      {
        type: "select",
        id: "modelRole.advisor",
        label: "Advisor",
        value: "native",
        options: [
          { id: "native", label: "Default" },
          { id: "unset", label: "Unassigned" },
          { id: "native/model", label: "Local model" },
          { id: "native/model:high", label: "Local model high" },
        ],
      },
    ],
  });
  client.saveProviderPreset.mockImplementation(async (_provider, stored) => {
    rows.push({ ...stored, source: "global", editable: true });
    return { saved: stored.name };
  });
  renderSection(
    <ProviderPresetsSection
      provider="omp"
      serverId="host-a"
      models={[
        {
          provider: "omp",
          id: '["native","model"]',
          label: "Local model",
          thinkingOptions: [{ id: "high", label: "High" }],
        },
      ]}
    />,
  );
  await screen.findByTestId("provider-presets-empty-create");
  fireEvent.click(screen.getByTestId("provider-presets-empty-create"));
  fireEvent.change(screen.getByRole("textbox"), { target: { value: "workflow" } });
  fireEvent.change(screen.getByTestId("provider-preset-model-options"), {
    target: { value: '["native","model"]' },
  });
  fireEvent.change(screen.getByTestId("provider-preset-thinking-options"), {
    target: { value: "high" },
  });
  await waitFor(() =>
    expect((screen.getByTestId("provider-preset-add-role") as HTMLButtonElement).disabled).toBe(
      false,
    ),
  );
  fireEvent.change(screen.getByTestId("provider-preset-add-role"), {
    target: { value: "modelRole.advisor" },
  });
  fireEvent.change(screen.getByTestId(/^preset-role-model-/), {
    target: { value: "native/model" },
  });
  fireEvent.change(screen.getByTestId(/^preset-role-thinking-/), { target: { value: "high" } });
  expect(screen.getByTestId(/^preset-role-name-/).textContent).toBe("ADVISOR");
  expect(screen.getByTestId(/^preset-role-name-/).tagName).toBe("SPAN");
  expect(screen.queryByTestId("provider-preset-roles")).toBeNull();
  expect(screen.getAllByRole("textbox")).toHaveLength(1);
  fireEvent.click(screen.getByTestId("provider-preset-save"));
  await waitFor(() => expect(screen.queryByTestId("provider-preset-save")).toBeNull());
  expect(rows[0]).toMatchObject({
    model: '["native","model"]',
    thinkingOptionId: "high",
    featureValues: { "modelRole.advisor": "native/model:high" },
  });
  fireEvent.click(screen.getByLabelText("settings.providers.presets.edit"));
  expect((screen.getByTestId("provider-preset-model-options") as HTMLSelectElement).value).toBe(
    '["native","model"]',
  );
  expect((screen.getByTestId(/^preset-role-model-/) as HTMLSelectElement).value).toBe(
    "native/model",
  );
  expect((screen.getByTestId(/^preset-role-thinking-/) as HTMLSelectElement).value).toBe("high");
  fireEvent.click(screen.getByTestId(/^preset-role-remove-/));
  expect(screen.queryByTestId(/^preset-role-model-/)).toBeNull();
});

it("starts native presets with a fixed roster of configured roles and resolved default previews", async () => {
  await ready(descriptor({ backend: "native" }));
  client.listProviderFeatures.mockResolvedValue({
    features: [
      {
        type: "select",
        id: "modelRole.smol",
        label: "Fast model (smol)",
        value: "native",
        options: [
          {
            id: "native",
            label: "Use native default",
            metadata: {
              nativeSelector: "provider/model:high",
              resolvedSelector: "provider/model:high",
            },
          },
          { id: "provider/model", label: "Model" },
          { id: "provider/model:high", label: "Model (high)" },
        ],
      },
    ],
  });
  renderSection(
    <ProviderPresetsSection
      provider="omp"
      serverId="host-a"
      models={[{ provider: "omp", id: '["provider","model"]', label: "Fast model" }]}
    />,
  );
  await screen.findByTestId("provider-presets-empty-create");
  fireEvent.click(screen.getByTestId("provider-presets-empty-create"));
  const identity = await screen.findByTestId("preset-role-name-initial-modelRole.smol");
  expect(identity.textContent).toBe("SMOL");
  expect(identity.tagName).toBe("SPAN");
  expect((screen.getByTestId(/^preset-role-model-/) as HTMLSelectElement).value).toBe("native");
  expect((screen.getByTestId(/^preset-role-thinking-/) as HTMLSelectElement).disabled).toBe(true);
  fireEvent.click(screen.getByTestId(/^preset-role-remove-/));
  expect(screen.queryByTestId(/^preset-role-model-/)).toBeNull();
  fireEvent.change(screen.getByTestId("provider-preset-add-role"), {
    target: { value: "modelRole.smol" },
  });
  expect(screen.getByTestId(/^preset-role-name-/).textContent).toBe("SMOL");
});

it("does not show OMP worker routing in an Ait-managed provider editor", async () => {
  await ready(descriptor(), [preset("team")]);
  renderSection(<ProviderPresetsSection provider="claude" serverId="host-a" />);
  await waitFor(() => expect(screen.getByText("team")).toBeTruthy());
  fireEvent.click(screen.getByLabelText("settings.providers.presets.edit"));
  await waitFor(() => expect(screen.getByTestId("provider-preset-save")).toBeTruthy());
  expect(screen.queryByText("providerControl.roleRoutingHint")).toBeNull();
});

it("validates native names before saving and offers an OMP-compatible example", async () => {
  await ready(descriptor({ backend: "native" }), [preset("team")]);
  renderSection(
    <ProviderPresetsSection
      provider="omp"
      serverId="host-a"
      models={[{ provider: "omp", id: "model-a", label: "Model A" }]}
    />,
  );
  fireEvent.click(await screen.findByTestId("provider-presets-add"));
  const name = screen.getByPlaceholderText("TeamDefault");
  fireEvent.change(name, { target: { value: "团队默认" } });
  expect(screen.getByTestId("provider-preset-invalid-name")).toBeTruthy();
  expect((screen.getByTestId("provider-preset-save") as HTMLButtonElement).disabled).toBe(true);
  fireEvent.change(name, { target: { value: "Team Default" } });
  expect(screen.getByTestId("provider-preset-invalid-name")).toBeTruthy();
  fireEvent.change(name, { target: { value: "Team_Default-2" } });
  expect(screen.queryByTestId("provider-preset-invalid-name")).toBeNull();
  expect(client.saveProviderPreset).not.toHaveBeenCalled();
});
