/** @vitest-environment jsdom */
import React from "react";
import { cleanup, fireEvent, render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { MutableDaemonConfig } from "@ait/protocol/messages";
import { i18n } from "@/i18n/i18next";
import { AcceptPeersCard } from "./accept-peers-card";

const mocks = vi.hoisted(() => ({
  connected: true,
  config: null as unknown,
  patchConfig: vi.fn(async () => undefined),
}));

vi.mock("@/hooks/use-daemon-config", () => ({
  useDaemonConfig: () => ({ config: mocks.config, patchConfig: mocks.patchConfig }),
}));
vi.mock("@/runtime/host-runtime", () => ({
  useHostRuntimeIsConnected: () => mocks.connected,
}));
vi.mock("@/components/ui/switch", () => ({
  Switch: ({
    value,
    onValueChange,
    accessibilityLabel,
  }: {
    value: boolean;
    onValueChange: (next: boolean) => void;
    accessibilityLabel?: string;
  }) => (
    <button
      role="switch"
      aria-checked={value}
      aria-label={accessibilityLabel}
      onClick={() => onValueChange(!value)}
    />
  ),
}));

function configWithMcp(mcp: Record<string, boolean>): MutableDaemonConfig {
  return { mcp } as MutableDaemonConfig;
}

function acceptPeersSwitch(view: ReturnType<typeof render>) {
  return view.getByRole("switch", { name: "Let agents on your other hosts use this host" });
}

beforeEach(async () => {
  vi.clearAllMocks();
  mocks.connected = true;
  mocks.config = configWithMcp({ injectIntoAgents: true });
  await i18n.changeLanguage("en");
});
afterEach(() => cleanup());

describe("AcceptPeersCard", () => {
  it("is off when the config has no acceptPeers value", () => {
    const view = render(<AcceptPeersCard serverId="remote" />);
    expect(view.getByTestId("host-page-accept-peers-card")).toBeTruthy();
    expect(view.getByText("Let agents on your other hosts use this host")).toBeTruthy();
    expect(acceptPeersSwitch(view).getAttribute("aria-checked")).toBe("false");
  });

  it("is off while the config has not loaded", () => {
    mocks.config = null;
    const view = render(<AcceptPeersCard serverId="remote" />);
    expect(acceptPeersSwitch(view).getAttribute("aria-checked")).toBe("false");
  });

  it("is on only when acceptPeers is exactly true", () => {
    mocks.config = configWithMcp({ injectIntoAgents: false, acceptPeers: true });
    const view = render(<AcceptPeersCard serverId="remote" />);
    expect(acceptPeersSwitch(view).getAttribute("aria-checked")).toBe("true");
  });

  it("is off when acceptPeers is false", () => {
    mocks.config = configWithMcp({ acceptPeers: false });
    const view = render(<AcceptPeersCard serverId="remote" />);
    expect(acceptPeersSwitch(view).getAttribute("aria-checked")).toBe("false");
  });

  it("turns acceptPeers on through patchConfig without touching other mcp keys", () => {
    const view = render(<AcceptPeersCard serverId="remote" />);
    fireEvent.click(acceptPeersSwitch(view));
    expect(mocks.patchConfig).toHaveBeenCalledExactlyOnceWith({ mcp: { acceptPeers: true } });
  });

  it("turns acceptPeers off when it is on", () => {
    mocks.config = configWithMcp({ acceptPeers: true });
    const view = render(<AcceptPeersCard serverId="remote" />);
    fireEvent.click(acceptPeersSwitch(view));
    expect(mocks.patchConfig).toHaveBeenCalledExactlyOnceWith({ mcp: { acceptPeers: false } });
  });

  it("renders nothing while the host is not connected", () => {
    mocks.connected = false;
    const view = render(<AcceptPeersCard serverId="remote" />);
    expect(view.queryByTestId("host-page-accept-peers-card")).toBeNull();
    expect(view.container.textContent).toBe("");
  });
});
