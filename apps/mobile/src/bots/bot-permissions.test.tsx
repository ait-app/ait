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
  modes: { codex: ["read-only", "auto"] },
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
  expect(screen.getAllByText("bots.permissionRiskHint")).toHaveLength(3);
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

it("unchecking every mode removes the provider from the cap", () => {
  const onChange = renderFields({ ...defaults, modes: { codex: ["auto"] } });
  fireEvent.click(screen.getByTestId("bot-mode-codex-auto"));
  expect(onChange).toHaveBeenCalledWith({ groups: defaults.groups, modes: {} });
});

it("marks the first mode as default and moves a chosen one to the front", () => {
  const onChange = renderFields();
  expect(screen.getByTestId("bot-mode-default-codex-read-only")).toBeTruthy();
  fireEvent.click(screen.getByTestId("bot-mode-make-default-codex-auto"));
  expect(onChange).toHaveBeenCalledWith({
    groups: defaults.groups,
    modes: { codex: ["auto", "read-only"] },
  });
  fireEvent.click(screen.getByTestId("bot-mode-codex-full-access"));
  expect(onChange).toHaveBeenLastCalledWith({
    groups: defaults.groups,
    modes: { codex: ["read-only", "auto", "full-access"] },
  });
});

it("disables every control when read-only", () => {
  renderFields(defaults, true);
  for (const id of ["bot-group-agents", "bot-mode-codex-auto", "bot-mode-codex-full-access"])
    expect(screen.getByTestId(id).getAttribute("aria-disabled")).toBe("true");
  expect(screen.getByTestId("bot-mode-make-default-codex-auto").getAttribute("aria-disabled")).toBe(
    "true",
  );
});

it("warns that terminals bypass the Bot's limits", () => {
  renderFields();
  const group = screen.getByTestId("bot-group-terminals").parentElement;
  expect(group?.textContent).toContain("bots.terminalsWarning");
  expect(screen.getAllByText("bots.terminalsWarning")).toHaveLength(1);
});

it("lists cap entries for providers missing from the snapshot so they can be removed", () => {
  const onChange = renderFields({ ...defaults, modes: { ...defaults.modes, gone: ["plan"] } });
  expect(screen.getByTestId("bot-modes-gone").textContent).toContain("bots.providerUnavailable");
  const plan = screen.getByTestId("bot-mode-gone-plan");
  expect(plan.getAttribute("aria-checked")).toBe("true");
  fireEvent.click(plan);
  expect(onChange).toHaveBeenCalledWith({ groups: defaults.groups, modes: defaults.modes });
});
