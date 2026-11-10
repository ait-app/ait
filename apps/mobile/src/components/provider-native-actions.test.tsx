// @vitest-environment jsdom
import React from "react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { ProviderNativeActions } from "./provider-native-actions";

const fixture = vi.hoisted(() => ({
  commands: [] as { name: string; source: string; description: string; argumentHint: string }[],
  compose: vi.fn(),
  localized: false,
  run: vi.fn(),
  send: vi.fn(),
  download: vi.fn(),
}));
vi.mock("@/hooks/use-agent-commands-query", () => ({
  useAgentCommandsQuery: () => ({ commands: fixture.commands }),
}));
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
vi.mock("@/components/settings/navigation-row", () => ({
  SettingsNavigationRow: ({
    label,
    onPress,
    disabled,
  }: {
    label: string;
    onPress: () => void;
    disabled?: boolean;
  }) => (
    <button onClick={onPress} disabled={disabled}>
      {label}
    </button>
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
      (fixture.localized && key === "providerControl.operations.get_session_stats"
        ? "查看统计"
        : undefined) ??
      {
        "providerControl.moreActions": "More actions",
        "providerControl.runOperation": "Run",
        "common.loading": "Loading...",
        "panels.file.downloadFile": "Download",
      }[key] ??
      options?.defaultValue ??
      key,
  }),
}));
beforeEach(() => {
  fixture.commands = [];
  fixture.localized = false;
  fixture.compose.mockReset();
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
      onComposeCommand={fixture.compose}
      serverId="server"
      agentId="agent"
      cwd="/workspace"
      actions={actions}
      surface={surface}
      vibeEnabled={actions.some((action) => action.id === "vibe_control")}
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
  await act(async () => fireEvent.click(screen.getByText("settings.providers.diagnostic.refresh")));
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

it("keeps BTW history out of the overflow menu so it opens through the slash command", () => {
  mount([stats, { id: "get_btw_history", label: "BTW history", fields: [] }]);
  expect(screen.queryByText("BTW history")).toBeNull();
  expect(fixture.run).not.toHaveBeenCalled();
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

it("queries statistics with one click and requests a durable card without opening a form", async () => {
  fixture.run.mockResolvedValue({ text: "Stats" });
  mount();
  await act(async () => choose("Statistics"));
  expect(fixture.run).toHaveBeenCalledExactlyOnceWith("agent", "get_session_stats", {
    present: "true",
  });
  expect(screen.queryByTestId("native-action-submit")).toBeNull();
  expect(screen.getByTestId("agent-more-actions").getAttribute("aria-expanded")).toBe("false");
});

it("does not expose generic Vibe, Goal or side-question task forms", () => {
  mount();
  for (const text of ["Vibe", "Goal"]) {
    fireEvent.change(screen.getByLabelText("Search actions"), { target: { value: text } });
    expect(screen.queryByRole("button", { name: text })).toBeNull();
  }
});

it("fills plugin and native task commands in the composer without executing or guessing their semantics", () => {
  fixture.commands = [
    { name: "record", source: "extension", description: "Track work", argumentHint: "[work]" },
    {
      name: "status-toggle",
      source: "extension",
      description: "Custom workflow",
      argumentHint: "",
    },
    {
      name: "goal",
      source: "builtin",
      description: "Establish a goal",
      argumentHint: "[objective]",
    },
  ];
  mount();
  choose("/record");
  expect(fixture.compose).toHaveBeenCalledExactlyOnceWith("/record ");
  expect(fixture.run).not.toHaveBeenCalled();
  expect(fixture.send).not.toHaveBeenCalled();
});

it("executes only declared builtin queries directly; query-shaped plugin names still compose", async () => {
  fixture.commands = [
    { name: "tools", source: "builtin", description: "Available tools", argumentHint: "" },
  ];
  fixture.send.mockResolvedValue({});
  mount();
  await act(async () => choose("/tools"));
  expect(fixture.send).toHaveBeenCalledExactlyOnceWith("agent", "/tools");
  expect(fixture.compose).not.toHaveBeenCalled();
});

it("keeps query failures visible and prevents repeat clicks while the read is pending", async () => {
  let reject!: (cause: Error) => void;
  fixture.run.mockImplementation(
    () =>
      new Promise((_resolve, fail) => {
        reject = fail;
      }),
  );
  mount();
  choose("Statistics");
  choose("Statistics");
  expect(fixture.run).toHaveBeenCalledOnce();
  await act(async () => reject(new Error("Native read failed")));
  expect(screen.getByRole("alert").textContent).toContain("Native read failed");
});

it("searches the translated label shown in the menu", async () => {
  fixture.localized = true;
  fixture.run.mockResolvedValue({ text: "Stats" });
  mount();
  fireEvent.change(screen.getByLabelText("Search actions"), { target: { value: "统计" } });
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "查看统计" })));
  expect(fixture.run).toHaveBeenCalledExactlyOnceWith("agent", "get_session_stats", {
    present: "true",
  });
});
