// @vitest-environment jsdom
import React from "react";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import type { ProviderSnapshotEntry } from "@ait/protocol/agent-types";
import type { BotPermissions } from "@ait/protocol/bot/types";
import { BotPermissionFields } from "./bot-permissions";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock("./styles", () => ({ botStyles: {} }));
vi.mock("@/stores/session-store", () => ({ useSessionStore: () => true }));
vi.mock("@/components/ui/select-field", () => ({
  SelectField: ({
    value,
    options,
    onChange,
    disabled,
    testID,
  }: {
    value: string;
    options: { id: string; label: string }[];
    onChange: (value: string) => void;
    disabled: boolean;
    testID: string;
  }) => (
    <select
      data-testid={testID}
      value={value}
      disabled={disabled}
      onChange={(event) => onChange(event.target.value)}
    >
      {options.map((option) => (
        <option key={option.id} value={option.id}>
          {option.label}
        </option>
      ))}
    </select>
  ),
}));

const providers = [
  {
    provider: "codex",
    label: "Codex",
    enabled: true,
    status: "ready",
    modes: [
      { id: "read-only", label: "Read only" },
      { id: "auto", label: "Auto" },
      { id: "full-access", label: "Full access" },
    ],
  },
] as unknown as ProviderSnapshotEntry[];

const defaults: BotPermissions = {
  groups: ["observe", "agents", "stop", "schedules"],
  modes: { codex: ["read-only"] },
};

afterEach(cleanup);

function renderFields(value: BotPermissions = defaults, disabled = false) {
  const onChange = vi.fn();
  render(
    <BotPermissionFields
      providers={providers}
      value={value}
      onChange={onChange}
      disabled={disabled}
    />,
  );
  return onChange;
}

it("observe_group_cannot_be_disabled", () => {
  const onChange = renderFields();
  const observe = screen.getByTestId("bot-group-observe");
  expect(observe.getAttribute("aria-checked")).toBe("true");
  expect(observe.getAttribute("aria-disabled")).toBe("true");
  fireEvent.click(observe);
  expect(onChange).not.toHaveBeenCalled();
});

it("toggles groups and flags risky ones", () => {
  const onChange = renderFields();
  expect(screen.getAllByText("bots.permissionRiskHint")).toHaveLength(2);
  fireEvent.click(screen.getByTestId("bot-group-scripts"));
  expect(onChange).toHaveBeenLastCalledWith({
    ...defaults,
    groups: [...defaults.groups, "scripts"],
  });
  fireEvent.click(screen.getByTestId("bot-group-schedules"));
  expect(onChange).toHaveBeenLastCalledWith({
    ...defaults,
    groups: ["observe", "agents", "stop"],
  });
});

it("shows both warnings", () => {
  renderFields();
  expect(screen.getByText("bots.modeCapWarning")).toBeTruthy();
  expect(screen.getByText("bots.commandReachWarning")).toBeTruthy();
});

it("offers no way to grant workspace creation", () => {
  renderFields();
  expect(screen.queryByTestId("bot-group-workspaces")).toBeNull();
});

it("has exactly one mode selector per provider showing the configured mode", () => {
  renderFields({ ...defaults, modes: { codex: ["auto", "read-only"] } });
  expect((screen.getByTestId("bot-mode-codex") as HTMLSelectElement).value).toBe("auto");
  expect(screen.getAllByRole("combobox")).toHaveLength(1);
  expect(screen.queryByRole("checkbox", { name: /Auto/ })).toBeNull();
});

it("stores the chosen mode as the provider's only mode", () => {
  const onChange = renderFields();
  fireEvent.change(screen.getByTestId("bot-mode-codex"), { target: { value: "full-access" } });
  expect(onChange).toHaveBeenCalledWith({
    groups: defaults.groups,
    modes: { codex: ["full-access"] },
  });
});

it("keeps a saved mode the provider no longer lists", () => {
  renderFields({ ...defaults, modes: { codex: ["retired-mode"] } });
  expect((screen.getByTestId("bot-mode-codex") as HTMLSelectElement).value).toBe("retired-mode");
});

it("choosing not allowed removes the provider", () => {
  const onChange = renderFields();
  fireEvent.change(screen.getByTestId("bot-mode-codex"), { target: { value: "" } });
  expect(onChange).toHaveBeenCalledWith({ groups: defaults.groups, modes: {} });
});

it("disables every control when read-only", () => {
  renderFields(defaults, true);
  expect(screen.getByTestId("bot-group-agents").getAttribute("aria-disabled")).toBe("true");
  expect(screen.getByTestId("bot-mode-codex")).toHaveProperty("disabled", true);
});

it("warns that terminals bypass the Bot's limits", () => {
  renderFields();
  const group = screen.getByTestId("bot-group-terminals").parentElement;
  expect(group?.textContent).toContain("bots.terminalsWarning");
  expect(screen.getAllByText("bots.terminalsWarning")).toHaveLength(1);
});

it("keeps a configured provider missing from the snapshot so it can be cleared", () => {
  const onChange = renderFields({ ...defaults, modes: { ...defaults.modes, gone: ["plan"] } });
  const select = screen.getByTestId("bot-mode-gone") as HTMLSelectElement;
  expect(select.value).toBe("plan");
  fireEvent.change(select, { target: { value: "" } });
  expect(onChange).toHaveBeenCalledWith({ groups: defaults.groups, modes: defaults.modes });
});
