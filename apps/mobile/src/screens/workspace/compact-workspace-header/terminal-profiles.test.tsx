// @vitest-environment jsdom
import React from "react";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { TerminalProfile } from "@ait/protocol/messages";
import { i18n as testI18n } from "@/i18n/i18next";
import { TabSwitcherTerminalProfiles } from "./terminal-profiles";

void testI18n;

const { daemonConfig, push } = vi.hoisted(() => ({
  daemonConfig: { terminalProfiles: undefined as TerminalProfile[] | undefined },
  push: vi.fn(),
}));

vi.mock("@/hooks/use-daemon-config", () => ({
  useDaemonConfig: () => ({ config: daemonConfig }),
}));
vi.mock("@/components/terminal-profile-icon", () => ({ TerminalProfileIcon: () => null }));
vi.mock("expo-router", () => ({ useRouter: () => ({ push }) }));

const PROFILES: TerminalProfile[] = [
  { id: "claude", name: "Claude Code", command: "claude" },
  { id: "htop", name: "htop", command: "htop" },
];

function renderProfiles(disabled = false) {
  const calls: string[] = [];
  const onCreateTerminalWithProfile = vi.fn((profile: TerminalProfile) => {
    calls.push(`create:${profile.id}`);
  });
  const onDismiss = vi.fn(() => {
    calls.push("dismiss");
  });
  render(
    <TabSwitcherTerminalProfiles
      serverId="server-1"
      disabled={disabled}
      onCreateTerminalWithProfile={onCreateTerminalWithProfile}
      onDismiss={onDismiss}
    />,
  );
  return { calls, onCreateTerminalWithProfile, onDismiss };
}

beforeEach(() => {
  vi.stubGlobal("React", React);
  daemonConfig.terminalProfiles = PROFILES;
  push.mockReset();
});
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

describe("tab switcher terminal profiles", () => {
  it("closes the switcher, then opens a terminal with the chosen profile", () => {
    const { calls, onCreateTerminalWithProfile } = renderProfiles();

    expect(screen.getByText("Terminal profiles")).toBeTruthy();
    expect(screen.getByText("Claude Code")).toBeTruthy();
    fireEvent.click(screen.getByTestId("workspace-tab-switcher-terminal-profile-htop"));

    expect(calls).toEqual(["dismiss", "create:htop"]);
    expect(onCreateTerminalWithProfile).toHaveBeenCalledWith(PROFILES[1]);
  });

  it("closes the switcher and opens the host's terminal settings from the edit button", () => {
    const { onDismiss } = renderProfiles();

    fireEvent.click(screen.getByLabelText("Edit profiles"));

    expect(onDismiss).toHaveBeenCalledOnce();
    expect(push).toHaveBeenCalledOnce();
    expect(String(push.mock.calls[0]?.[0])).toContain("server-1");
    expect(String(push.mock.calls[0]?.[0])).toContain("terminals");
  });

  it("does not open terminals while terminal creation is unavailable", () => {
    const { onCreateTerminalWithProfile, onDismiss } = renderProfiles(true);

    fireEvent.click(screen.getByTestId("workspace-tab-switcher-terminal-profile-claude"));

    expect(onCreateTerminalWithProfile).not.toHaveBeenCalled();
    expect(onDismiss).not.toHaveBeenCalled();
  });

  it("renders nothing when the host has no terminal profiles", () => {
    daemonConfig.terminalProfiles = [];
    renderProfiles();

    expect(screen.queryByTestId("workspace-tab-switcher-terminal-profiles")).toBeNull();
  });
});
