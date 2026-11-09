// @vitest-environment jsdom
import React from "react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { ProviderNativeActions } from "./provider-native-actions";

const fixture = vi.hoisted(() => ({ run: vi.fn(), send: vi.fn(), download: vi.fn() }));
vi.mock("@tanstack/react-query", () => ({ useQueryClient: () => ({}) }));
vi.mock("@/data/providers-snapshot", () => ({
  refreshOmpModelsOnAccess: vi.fn().mockResolvedValue(null),
}));
vi.mock("react-native-unistyles", () => ({
  StyleSheet: {
    create: (factory: (theme: { colors: Record<string, string> }) => unknown) =>
      factory({ colors: {} }),
  },
}));
vi.mock("react-native", () => ({
  Platform: {
    OS: "web",
    select: (options: { web?: string; default?: string }) => options.web ?? options.default,
  },
  View: ({
    children,
    style,
    testID,
  }: {
    children: React.ReactNode;
    style?: React.CSSProperties;
    testID?: string;
  }) => (
    <div style={style} data-testid={testID}>
      {children}
    </div>
  ),
  Text: ({
    children,
    accessibilityRole,
  }: {
    children: React.ReactNode;
    accessibilityRole?: string;
  }) => <span role={accessibilityRole}>{children}</span>,
  ScrollView: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
  TextInput: ({
    accessibilityLabel,
    value,
    editable,
    onChangeText,
  }: {
    accessibilityLabel: string;
    value: string;
    editable: boolean;
    onChangeText: (value: string) => void;
  }) => (
    <input
      aria-label={accessibilityLabel}
      value={value}
      disabled={!editable}
      onChange={(event) => onChangeText(event.target.value)}
    />
  ),
}));
vi.mock("@/runtime/host-runtime", () => ({
  useHostRuntimeClient: () => ({ runNativeAction: fixture.run, sendMessage: fixture.send }),
}));
vi.mock("@/hooks/use-file-download", () => ({
  useFileDownload: () => fixture.download,
}));
vi.mock("@/components/ui/button", () => ({
  Button: ({
    children,
    onPress,
    disabled,
    leftIcon,
    size,
    testID,
  }: {
    children: React.ReactNode;
    onPress: () => void;
    disabled?: boolean;
    leftIcon?: unknown;
    size?: string;
    testID?: string;
  }) => (
    <button
      onClick={onPress}
      disabled={disabled}
      data-has-icon={leftIcon ? "true" : undefined}
      data-size={size}
      data-testid={testID}
    >
      {children}
    </button>
  ),
}));
vi.mock("@/composer/agent-controls/control", () => ({
  AgentControlTrigger: ({
    onPress,
    disabled,
    accessibilityLabel,
    accessibilityState,
    testID,
  }: {
    onPress: () => void;
    disabled?: boolean;
    accessibilityLabel: string;
    accessibilityState: { expanded: boolean };
    testID: string;
  }) => (
    <button
      onClick={onPress}
      disabled={disabled}
      aria-label={accessibilityLabel}
      aria-expanded={accessibilityState.expanded}
      data-testid={testID}
    >
      …
    </button>
  ),
}));
vi.mock("@/components/ui/tooltip", () => ({
  Tooltip: ({ children }: { children: React.ReactNode }) => <>{children}</>,
  TooltipTrigger: ({ children }: { children: React.ReactNode }) => <>{children}</>,
  TooltipContent: () => null,
}));
vi.mock("@/components/ui/combobox", () => ({
  Combobox: ({
    options,
    onSelect,
    open,
    onOpenChange,
    children,
    header,
    footer,
    keepOpenOnSelect,
  }: {
    options: { id: string; label: string }[];
    onSelect: (id: string) => void;
    open: boolean;
    onOpenChange: (open: boolean) => void;
    children?: React.ReactNode;
    header?: {
      title: string;
      back?: { onPress: () => void };
      search?: { onChange: (value: string) => void };
    };
    footer?: React.ReactNode;
    keepOpenOnSelect?: boolean;
  }) =>
    open ? (
      <div role="dialog">
        {header?.back ? <button onClick={header.back.onPress}>Back</button> : null}
        <span>{header?.title}</span>
        {header?.search ? (
          <input
            aria-label="Search actions"
            onChange={(e) => header.search!.onChange(e.target.value)}
          />
        ) : null}
        <button onClick={() => onOpenChange(false)}>Close</button>
        {children ??
          options.map((option) => (
            <button
              key={option.id}
              onClick={() => {
                onSelect(option.id);
                if (!keepOpenOnSelect) onOpenChange(false);
              }}
            >
              {option.label}
            </button>
          ))}
        {footer}
      </div>
    ) : null,
}));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: { defaultValue?: string }) =>
      ({
        "providerControl.moreActions": "More actions",
        "providerControl.runOperation": "Run",
        "common.loading": "Loading...",
        "panels.file.downloadFile": "Download",
      })[key] ??
      options?.defaultValue ??
      key,
  }),
}));
beforeEach(() => {
  vi.stubGlobal("React", React);
  fixture.run.mockReset();
  fixture.send.mockReset().mockResolvedValue(undefined);
  fixture.download.mockReset();
});
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

vi.mock("@/components/ui/select-field", () => ({
  SelectField: ({
    label,
    value,
    options,
    disabled,
    onChange,
    testID,
  }: {
    label: string;
    value: string;
    options: { id: string; label: string }[];
    disabled: boolean;
    onChange: (value: string) => void;
    testID: string;
  }) => (
    <select
      aria-label={label}
      data-testid={testID}
      value={value ?? ""}
      disabled={disabled}
      onChange={(e) => onChange(e.target.value)}
    >
      <option value="">Select</option>
      {options.map((item) => (
        <option key={item.id} value={item.id}>
          {item.label}
        </option>
      ))}
    </select>
  ),
}));
const fields = (ids: string[]) =>
  ids.map((id) => ({
    id,
    label: id,
    options:
      id === "operation"
        ? ["list", "spawn", "send", "wait", "kill", "toggle"]
        : id === "op"
          ? ["get", "create", "resume", "pause", "drop"]
          : id === "cli"
            ? ["fast", "good"]
            : [],
    required: id === "operation" || id === "op",
  }));
const vibe = {
  id: "vibe_control",
  label: "Vibe",
  fields: fields([
    "operation",
    "enabled",
    "cli",
    "prompt",
    "name",
    "sessionId",
    "message",
    "sessions",
    "timeout",
  ]),
};
const goal = { id: "goal", label: "Goal", fields: fields(["op", "objective", "token_budget"]) };

const stats = { id: "get_session_stats", label: "Statistics", fields: [] };
const exportAction = { id: "export_html", label: "Export", fields: [] };
const all = [
  stats,
  exportAction,
  vibe,
  goal,
  { id: "model_preset_save", label: "Save preset", fields: [] },
  { id: "model_preset_delete", label: "Delete preset", fields: [] },
];
function mount(actions = all, surface: "composer" | "settings" = "composer") {
  const result = render(
    <ProviderNativeActions
      serverId="server"
      agentId="agent"
      cwd="/workspace"
      actions={actions}
      surface={surface}
      disabled={false}
      workers={[
        {
          id: "worker-1",
          parentId: "agent",
          status: "completed",
          description: "Design task",
          model: null,
        },
      ]}
    />,
  );
  fireEvent.click(
    screen.getByRole("button", {
      name: surface === "composer" ? "More actions" : "providerControl.tasks.providerSettings",
    }),
  );
  return result;
}
function choose(label: string) {
  fireEvent.change(screen.getByLabelText("Search actions"), { target: { value: label } });
  fireEvent.click(screen.getByRole("button", { name: label }));
}
const submit = () => screen.getByTestId("native-action-submit") as HTMLButtonElement;
function select(id: string, value: string) {
  fireEvent.change(screen.getByTestId(`native-field-${id}`), { target: { value } });
}
it("groups task menus and never offers generic preset writes", () => {
  mount();
  expect(screen.getByRole("button", { name: "providerControl.groups.conversation" })).toBeTruthy();
  expect(screen.queryByRole("button", { name: "Save preset" })).toBeNull();
  fireEvent.change(screen.getByLabelText("Search actions"), { target: { value: "preset" } });
  expect(screen.queryByRole("button", { name: "Delete preset" })).toBeNull();
  expect(fixture.run).not.toHaveBeenCalled();
});
it("preserves statistics while refreshing, then downloads an export in the original host scope", async () => {
  fixture.run.mockResolvedValueOnce({ text: "Statistics ready" }).mockResolvedValueOnce({
    text: "Exported",
    file: { path: "file.html", fileName: "file.html" },
  });
  mount();
  choose("Statistics");
  await act(async () => fireEvent.click(submit()));
  expect(fixture.run).toHaveBeenCalledWith("agent", "get_session_stats", {});
  expect(screen.getByText("Statistics ready")).toBeTruthy();
  fireEvent.click(screen.getByText("Back"));
  choose("Export");
  await act(async () => fireEvent.click(submit()));
  fireEvent.click(screen.getByText("Download"));
  expect(fixture.download).toHaveBeenCalledWith({ path: "file.html", fileName: "file.html" });
});
it("changing spawn to list removes old parameters and sends only list", async () => {
  fixture.run.mockResolvedValue({ text: "Done" });
  mount();
  choose("Vibe");
  select("operation", "spawn");
  select("cli", "fast");
  fireEvent.change(screen.getByLabelText("prompt"), { target: { value: "Build board" } });
  expect(submit().disabled).toBe(false);
  select("operation", "list");
  expect(screen.queryByLabelText("prompt")).toBeNull();
  await act(async () => fireEvent.click(submit()));
  expect(fixture.run).toHaveBeenCalledExactlyOnceWith("agent", "vibe_control", {
    operation: "list",
  });
  select("operation", "spawn");
  expect(submit().disabled).toBe(true);
});
it("spawn requires both class and task; send requires a selected worker and guidance", async () => {
  mount();
  choose("Vibe");
  select("operation", "spawn");
  expect(submit().disabled).toBe(true);
  fireEvent.click(submit());
  expect(fixture.run).not.toHaveBeenCalled();
  select("cli", "fast");
  expect(submit().disabled).toBe(true);
  select("operation", "send");
  expect(submit().disabled).toBe(true);
  select("sessionId", "worker-1");
  fireEvent.change(screen.getByLabelText("message"), { target: { value: "Review accessibility" } });
  expect(submit().disabled).toBe(false);
  fixture.run.mockResolvedValue({ text: "Accepted" });
  await act(async () => fireEvent.click(submit()));
  expect(fixture.run).toHaveBeenCalledExactlyOnceWith("agent", "vibe_control", {
    operation: "send",
    sessionId: "worker-1",
    message: "Review accessibility",
  });
});
it("Goal switching drops objective and budget, validates create before RPC", async () => {
  fixture.run.mockResolvedValue({ text: "Goal" });
  mount();
  choose("Goal");
  select("op", "create");
  expect(submit().disabled).toBe(true);
  fireEvent.change(screen.getByLabelText("objective"), { target: { value: "Deliver board" } });
  fireEvent.change(screen.getByLabelText("token_budget"), { target: { value: "-1" } });
  expect(submit().disabled).toBe(true);
  select("op", "get");
  expect(screen.queryByLabelText("objective")).toBeNull();
  await act(async () => fireEvent.click(submit()));
  expect(fixture.run).toHaveBeenCalledExactlyOnceWith("agent", "goal", { op: "get" });
});
it("selects history entries by content and submits their bound target without text IDs", async () => {
  fixture.run
    .mockResolvedValueOnce({
      text: JSON.stringify({
        entries: [{ id: "entry-17", message: { role: "user", content: "Board requirements" } }],
      }),
    })
    .mockResolvedValueOnce({ text: "Branched" });
  mount([
    {
      id: "branch",
      label: "Branch",
      fields: [{ id: "entryId", label: "Entry", required: true, options: [] }],
    },
  ]);
  choose("Branch");
  expect(submit().disabled).toBe(true);
  await act(async () => {});
  expect(screen.getByRole("option", { name: "Board requirements" })).toBeTruthy();
  select("entryId", "entry-17");
  await act(async () => fireEvent.click(submit()));
  expect(fixture.run).toHaveBeenLastCalledWith("agent", "branch", { entryId: "entry-17" });
  expect(screen.queryByRole("textbox", { name: "Entry" })).toBeNull();
});
it("list query failure blocks mutation and retry reloads choices", async () => {
  fixture.run.mockRejectedValueOnce(new Error("History unavailable")).mockResolvedValueOnce({
    text: JSON.stringify({ entries: [{ id: "entry-1", label: "Request", role: "user" }] }),
  });
  mount([
    {
      id: "branch",
      label: "Branch",
      fields: [{ id: "entryId", label: "Entry", required: true, options: [] }],
    },
  ]);
  choose("Branch");
  await act(async () => {});
  expect(submit().disabled).toBe(true);
  expect(screen.getByRole("alert").textContent).toBe("History unavailable");
  await act(async () => fireEvent.click(screen.getByText("common.actions.refresh")));
  select("entryId", "entry-1");
  expect(submit().disabled).toBe(false);
  expect(fixture.run.mock.calls.every((call) => call[1] === "get_entries")).toBe(true);
});
it("provider account actions are only available inside provider settings", () => {
  const actions = [{ id: "logout", label: "Sign out", fields: [] }];
  const result = render(
    <ProviderNativeActions
      serverId="server"
      agentId="agent"
      cwd="/workspace"
      actions={actions}
      disabled={false}
    />,
  );
  expect(screen.queryByRole("button", { name: "More actions" })).toBeNull();
  result.unmount();
  mount(actions, "settings");
  expect(screen.getByRole("button", { name: "providerControl.groups.accounts" })).toBeTruthy();
});

it("switching provider ignores late account results and binds sign out to the selected account", async () => {
  let finishFirst!: (value: { text: string }) => void;
  let finishSecond!: (value: { text: string }) => void;
  fixture.run.mockImplementation((_agent, action, args) => {
    if (action === "get_login_providers")
      return Promise.resolve({
        text: JSON.stringify({
          providers: [
            { id: "first", name: "First provider" },
            { id: "second", name: "Second provider" },
          ],
        }),
      });
    if (action === "get_logout_accounts")
      return new Promise((resolve) => {
        if (args.providerId === "first") finishFirst = resolve;
        else finishSecond = resolve;
      });
    return Promise.resolve({ text: "Signed out" });
  });
  mount(
    [
      {
        id: "logout",
        label: "Sign out",
        fields: [
          { id: "providerId", label: "Provider", required: true, options: [] },
          { id: "credentialId", label: "Account", required: true, options: [] },
        ],
      },
    ],
    "settings",
  );
  choose("Sign out");
  await act(async () => {});
  select("providerId", "first");
  expect(submit().disabled).toBe(true);
  select("providerId", "second");
  await act(async () =>
    finishSecond({
      text: JSON.stringify({ accounts: [{ credentialId: 8, label: "Team account" }] }),
    }),
  );
  select("credentialId", "8");
  await act(async () =>
    finishFirst({
      text: JSON.stringify({ accounts: [{ credentialId: 2, label: "Old account" }] }),
    }),
  );
  expect(screen.queryByRole("option", { name: "Old account" })).toBeNull();
  await act(async () => fireEvent.click(submit()));
  expect(fixture.run).toHaveBeenLastCalledWith("agent", "logout", {
    providerId: "second",
    credentialId: "8",
  });
});

it("treats a truncated native list as a read error and blocks mutation", async () => {
  fixture.run.mockResolvedValue({ text: '{"entries":[' });
  mount([
    {
      id: "branch",
      label: "Branch",
      fields: [{ id: "entryId", label: "Entry", required: true, options: [] }],
    },
  ]);
  choose("Branch");
  await act(async () => {});
  expect(screen.getByRole("alert").textContent).toBe("providerControl.tasks.listIncomplete");
  expect(submit().disabled).toBe(true);
  expect(fixture.run).toHaveBeenCalledExactlyOnceWith("agent", "get_entries", {});
});

it("opens side question history in the existing topic panel instead of displaying raw records", async () => {
  mount([{ id: "get_btw_history", label: "Side question history", fields: [] }]);
  choose("Side question history");
  await act(async () => fireEvent.click(submit()));
  expect(fixture.send).toHaveBeenCalledExactlyOnceWith("agent", "/btw");
  expect(fixture.run).not.toHaveBeenCalled();
  expect(screen.queryByTestId("agent-action-form")).toBeNull();
});

it("prevents history changes while a main turn or Vibe owns the conversation", async () => {
  const branch = {
    id: "branch",
    label: "Branch",
    fields: [{ id: "entryId", label: "Entry", required: true, options: [] }],
  };
  fixture.run.mockResolvedValue({
    text: JSON.stringify({ entries: [{ id: "entry", label: "Request", role: "user" }] }),
  });
  const result = render(
    <ProviderNativeActions
      serverId="server"
      agentId="agent"
      cwd="/workspace"
      actions={[branch]}
      disabled={false}
      turnActive
    />,
  );
  fireEvent.click(screen.getByRole("button", { name: "More actions" }));
  choose("Branch");
  await act(async () => {});
  select("entryId", "entry");
  expect(submit().disabled).toBe(true);
  expect(screen.getByText("providerControl.tasks.finishTurn")).toBeTruthy();
  result.rerender(
    <ProviderNativeActions
      serverId="server"
      agentId="agent"
      cwd="/workspace"
      actions={[branch]}
      disabled={false}
      vibeEnabled
    />,
  );
  expect(screen.getByText("providerControl.tasks.exitVibe")).toBeTruthy();
  fireEvent.click(submit());
  expect(fixture.run).toHaveBeenCalledExactlyOnceWith("agent", "get_entries", {});
});

it("offers only user messages for branching and explains bounded recent history", async () => {
  fixture.run.mockResolvedValue({
    text: JSON.stringify({
      omitted: 42,
      entries: [
        { id: "config", label: "Model changed", entryType: "model_change" },
        { id: "answer", label: "Implementation response", role: "assistant", entryType: "message" },
        { id: "request", label: "Create a handoff card", role: "user", entryType: "message" },
      ],
    }),
  });
  mount([
    {
      id: "branch",
      label: "Branch",
      fields: [{ id: "entryId", label: "Entry", required: true, options: [] }],
    },
  ]);
  choose("Branch");
  await act(async () => {});
  expect(screen.queryByRole("option", { name: "Implementation response" })).toBeNull();
  expect(screen.queryByRole("option", { name: "Model changed" })).toBeNull();
  expect(screen.getByRole("option", { name: "Create a handoff card" })).toBeTruthy();
  expect(screen.getByText("providerControl.tasks.recentHistory")).toBeTruthy();
});
