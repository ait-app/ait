// @vitest-environment jsdom
import React from "react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { ProviderNativeActions } from "./provider-native-actions";

const fixture = vi.hoisted(() => ({ run: vi.fn(), download: vi.fn() }));
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
  useHostRuntimeClient: () => ({ runNativeAction: fixture.run }),
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
  }: {
    children: React.ReactNode;
    onPress: () => void;
    disabled?: boolean;
    leftIcon?: unknown;
    size?: string;
  }) => (
    <button
      onClick={onPress}
      disabled={disabled}
      data-has-icon={leftIcon ? "true" : undefined}
      data-size={size}
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
      </div>
    ) : null,
}));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) =>
      ({
        "providerControl.moreActions": "More actions",
        "providerControl.runOperation": "Run",
        "common.loading": "Loading...",
        "panels.file.downloadFile": "Download",
      })[key] ?? key,
  }),
}));
beforeEach(() => {
  vi.stubGlobal("React", React);
  fixture.run.mockReset();
  fixture.download.mockReset();
});
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});
const actions = [
  { id: "get_session_stats", label: "Session statistics", fields: [] },
  {
    id: "login",
    label: "Log in",
    fields: [
      {
        id: "providerId",
        label: "Provider",
        options: [],
        required: true,
      },
    ],
  },
];

function chooseAction(label: string) {
  const back = screen.queryByRole("button", { name: "Back" });
  if (back) fireEvent.click(back);
  fireEvent.click(screen.getByRole("button", { name: label }));
}

it("keeps actions and results behind the icon and preserves results when reopened", async () => {
  fixture.run.mockResolvedValue({ text: "Statistics ready" });
  const onClose = vi.fn();
  render(
    <ProviderNativeActions
      serverId="server"
      agentId="agent"
      cwd="/workspace"
      actions={actions}
      disabled={false}
      onClose={onClose}
    />,
  );
  const trigger = screen.getByRole("button", { name: "More actions" });
  expect(trigger.textContent).toBe("…");
  expect(trigger.getAttribute("aria-expanded")).toBe("false");
  expect(screen.queryByRole("dialog")).toBeNull();
  fireEvent.click(trigger);
  expect(trigger.getAttribute("aria-expanded")).toBe("true");
  expect(screen.queryByTestId("agent-action-form")).toBeNull();
  fireEvent.change(screen.getByLabelText("Search actions"), { target: { value: "statistics" } });
  expect(screen.queryByRole("button", { name: "Log in" })).toBeNull();
  chooseAction("Session statistics");
  expect(fixture.run).not.toHaveBeenCalled();
  await act(async () => {
    fireEvent.click(screen.getByText("Run"));
  });
  expect(fixture.run).toHaveBeenCalledExactlyOnceWith("agent", "get_session_stats", {});
  expect(screen.getByText("Statistics ready")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Close" }));
  expect(onClose).toHaveBeenCalledOnce();
  expect(screen.queryByText("Statistics ready")).toBeNull();
  expect(screen.queryByRole("dialog")).toBeNull();
  fireEvent.click(trigger);
  expect(screen.queryByTestId("agent-action-form")).toBeNull();
  chooseAction("Session statistics");
  expect(screen.getByText("Statistics ready")).toBeTruthy();
});

it("requires fields and preserves input for retry", async () => {
  fixture.run
    .mockRejectedValueOnce(new Error("Provider unavailable"))
    .mockResolvedValueOnce({ text: "Signed in" });
  render(
    <ProviderNativeActions
      serverId="server"
      agentId="agent"
      cwd="/workspace"
      actions={actions}
      disabled={false}
    />,
  );
  fireEvent.click(screen.getByRole("button", { name: "More actions" }));
  chooseAction("Log in");
  expect(screen.getByText("Run").closest("button")?.disabled).toBe(true);
  fireEvent.change(screen.getByLabelText("Provider"), {
    target: { value: "test-provider" },
  });
  await act(async () => {
    fireEvent.click(screen.getByText("Run"));
  });
  expect(screen.getByRole("alert").textContent).toBe("Provider unavailable");
  expect((screen.getByLabelText("Provider") as HTMLInputElement).value).toBe("test-provider");
  await act(async () => {
    fireEvent.click(screen.getByText("Run"));
  });
  expect(screen.getByText("Signed in")).toBeTruthy();
  expect(fixture.run).toHaveBeenNthCalledWith(2, "agent", "login", {
    providerId: "test-provider",
  });
});

it("keeps pending actions independent and preserves the previous result while refreshing", async () => {
  let finishLogin!: (result: { text: string }) => void;
  let finishRefresh!: (result: { text: string }) => void;
  fixture.run
    .mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finishLogin = resolve;
        }),
    )
    .mockResolvedValueOnce({ text: "Statistics ready" })
    .mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finishRefresh = resolve;
        }),
    );
  render(
    <ProviderNativeActions
      serverId="server"
      agentId="agent"
      cwd="/workspace"
      actions={actions}
      disabled={false}
    />,
  );
  fireEvent.click(screen.getByRole("button", { name: "More actions" }));
  chooseAction("Log in");
  fireEvent.change(screen.getByLabelText("Provider"), {
    target: { value: "test-provider" },
  });
  fireEvent.click(screen.getByText("Run"));
  expect(screen.getByText("Loading...").closest("button")?.disabled).toBe(true);
  chooseAction("Session statistics");
  await act(async () => {
    fireEvent.click(screen.getByText("Run"));
  });
  expect(screen.getByText("Statistics ready")).toBeTruthy();
  fireEvent.click(screen.getByText("Run"));
  expect(screen.getByText("Statistics ready")).toBeTruthy();
  chooseAction("Log in");
  expect((screen.getByLabelText("Provider") as HTMLInputElement).value).toBe("test-provider");
  expect(screen.queryByText("Statistics ready")).toBeNull();
  await act(async () => {
    finishRefresh({ text: "New statistics" });
  });
  expect(screen.getByText("Loading...").closest("button")?.disabled).toBe(true);
  await act(async () => {
    finishLogin({ text: "Signed in" });
  });
  expect(screen.getByText("Signed in")).toBeTruthy();
  chooseAction("Session statistics");
  expect(screen.getByText("New statistics")).toBeTruthy();
});

it("downloads the native export using its original host workspace scope", async () => {
  fixture.run.mockResolvedValue({
    text: "Exported",
    file: {
      path: "ait-export-test.html",
      fileName: "ait-export-test.html",
    },
  });
  render(
    <ProviderNativeActions
      serverId="server"
      agentId="agent"
      cwd="/workspace"
      actions={actions}
      disabled={false}
    />,
  );
  fireEvent.click(screen.getByRole("button", { name: "More actions" }));
  chooseAction("Session statistics");
  await act(async () => {
    fireEvent.click(screen.getByText("Run"));
  });
  expect(screen.getByText("Download")).toBeTruthy();
  for (const label of ["Run", "Download"]) {
    const button = screen.getByText(label).closest("button")!;
    expect(button.dataset.size).toBe("sm");
    expect(button.dataset.hasIcon).toBe("true");
    expect(button.parentElement?.style.flexDirection).toBe("row");
    expect(button.parentElement?.style.flexWrap).toBe("wrap");
  }
  fireEvent.click(screen.getByText("Download"));
  expect(fixture.download).toHaveBeenCalledExactlyOnceWith({
    path: "ait-export-test.html",
    fileName: "ait-export-test.html",
  });
});
