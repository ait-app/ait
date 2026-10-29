/** @vitest-environment jsdom */
import React from "react";
import { cleanup, fireEvent, render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { i18n } from "@/i18n/i18next";
import { defaultHostAppearance } from "@/hosts/appearance";
import type { HostSyncState } from "@/runtime/online-service-host-sync";
import { OnlineServiceHostSection } from "./online-service-host-section";

const mocks = vi.hoisted(() => ({
  disconnect: vi.fn(),
  synchronize: vi.fn(),
  navigate: vi.fn(),
  loggedOut: true,
  connected: true,
  supported: true,
  sync: undefined as HostSyncState | undefined,
}));
vi.mock("expo-router", () => ({ useRouter: () => ({ push: mocks.navigate }) }));
vi.mock("@/utils/host-routes", () => ({ buildSettingsSectionRoute: (section: string) => section }));
vi.mock("@/runtime/account-state", () => ({
  useAccountState: () => ({
    status: mocks.loggedOut ? "logged_out" : "online",
    name: "Me",
    center: "https://example.test/api",
  }),
}));
vi.mock("@/runtime/host-runtime", () => ({
  useHostRuntimeClient: () => ({
    getLastServerInfoMessage: () => ({ features: { onlineServiceSync: mocks.supported } }),
  }),
  useHostRuntimeIsConnected: () => mocks.connected,
}));
vi.mock("@/runtime/online-service-host-sync", () => ({
  disconnectOnlineServiceHost: mocks.disconnect,
  synchronizeOnlineServiceHost: mocks.synchronize,
  useOnlineServiceHostSync: (select: (state: unknown) => unknown) =>
    select({ hosts: { remote: mocks.sync } }),
}));
vi.mock("@/components/settings", () => {
  const Frame = ({ children }: { children: React.ReactNode }) => <div>{children}</div>;
  const Row = ({
    children,
    hint,
    error,
  }: {
    children: React.ReactNode;
    hint?: string;
    error?: string;
  }) => (
    <div>
      <span>{hint}</span>
      {children}
      {error && <span role="alert">{error}</span>}
    </div>
  );
  return { SettingsSection: Frame, SettingsCard: Frame, SettingsRow: Row };
});
const host = {
  serverId: "remote",
  label: "Remote workstation",
  appearance: defaultHostAppearance(),
  lifecycle: {},
  connections: [],
  preferredConnectionId: null,
  createdAt: "",
  updatedAt: "",
};
function snapshot(): HostSyncState {
  const status = {
    serverId: "remote",
    instanceId: "runtime",
    platform: "darwin",
    management: {
      mode: "managed" as const,
      autonomous: true,
      phase: "running",
      binding: { node_id: "node", host_id: "host", server_id: "remote", grant_id: "grant" },
    },
    status: { online: true, connecting: false, error: null, epoch: null },
  };
  return {
    enabled: true,
    busy: false,
    error: null,
    status,
    publication: {
      ...status,
      hub: { status: { ...status.status, rejected: null } },
      relay: status.status,
    },
  };
}
beforeEach(async () => {
  vi.clearAllMocks();
  mocks.loggedOut = true;
  mocks.connected = true;
  mocks.supported = true;
  mocks.sync = snapshot();
  await i18n.changeLanguage("en");
});
afterEach(() => cleanup());

function button(view: ReturnType<typeof render>) {
  const element = view.getByTestId("host-online-service-sync");
  if (!(element instanceof HTMLButtonElement)) throw new Error("Expected a synchronization button");
  return element;
}

describe("host synchronization actions", () => {
  it("keeps Stop synchronization available after client logout", () => {
    const view = render(<OnlineServiceHostSection host={host} />);
    expect(button(view).disabled).toBe(false);
    expect(button(view).textContent).toBe("Stop synchronization");
    fireEvent.click(button(view));
    expect(mocks.disconnect).toHaveBeenCalledExactlyOnceWith("remote");
    expect(view.getAllByRole("button")).toHaveLength(1);
  });

  it("uses the same button to sign in before starting synchronization", () => {
    mocks.sync!.enabled = false;
    const view = render(<OnlineServiceHostSection host={host} />);
    expect(button(view).disabled).toBe(false);
    expect(button(view).textContent).toBe("Sign in");
    fireEvent.click(button(view));
    expect(mocks.navigate).toHaveBeenCalledExactlyOnceWith("online-service");
    expect(mocks.synchronize).not.toHaveBeenCalledWith("remote", host.label, true);
  });

  it("starts synchronization when signed in", () => {
    mocks.loggedOut = false;
    mocks.sync!.enabled = false;
    const view = render(<OnlineServiceHostSection host={host} />);
    expect(button(view).disabled).toBe(false);
    expect(button(view).textContent).toBe("Connect to online service");
    fireEvent.click(button(view));
    expect(mocks.synchronize).toHaveBeenCalledWith("remote", host.label, true);
  });

  it.each([
    [true, "Connecting to online service..."],
    [false, "Not connected to online service"],
  ])("allows stopping background reconnects (connecting=%s)", (connecting, message) => {
    mocks.sync!.publication!.hub.status.online = false;
    mocks.sync!.publication!.hub.status.connecting = connecting;
    const view = render(<OnlineServiceHostSection host={host} />);
    expect(view.getByText(message)).toBeTruthy();
    expect(button(view).textContent).toBe("Stop synchronization");
    expect(button(view).disabled).toBe(false);
    fireEvent.click(button(view));
    expect(mocks.disconnect).toHaveBeenCalledOnce();
  });

  it("does not treat a retained binding as valid authorization after revocation", () => {
    mocks.sync!.enabled = false;
    mocks.sync!.publication!.management!.phase = "reauthorization_required";
    const view = render(<OnlineServiceHostSection host={host} />);
    expect(view.getByText("Sign in under App → Online Service first.")).toBeTruthy();
    expect(button(view).textContent).toBe("Sign in");
    fireEvent.click(button(view));
    expect(mocks.disconnect).not.toHaveBeenCalled();
    expect(mocks.navigate).toHaveBeenCalledOnce();
  });
});

it.each(["managed", "external"] as const)("requires an update for old %s daemons", (mode) => {
  mocks.sync!.enabled = false;
  mocks.sync!.publication = undefined;
  mocks.sync!.status!.management = { mode };
  const view = render(<OnlineServiceHostSection host={host} />);
  expect(view.getByText("Update this host to enable online service synchronization.")).toBeTruthy();
  expect(button(view).disabled).toBe(true);
  fireEvent.click(button(view));
  expect(mocks.disconnect).not.toHaveBeenCalled();
  expect(mocks.navigate).not.toHaveBeenCalled();
});

it("disables actions when the client cannot reach the host even with cached online status", () => {
  mocks.connected = false;
  const view = render(<OnlineServiceHostSection host={host} />);
  expect(view.getByText("Connect to this host first.")).toBeTruthy();
  expect(button(view).disabled).toBe(true);
  expect(mocks.synchronize).not.toHaveBeenCalled();
});

it("disables actions for a host without synchronization support", () => {
  mocks.supported = false;
  const view = render(<OnlineServiceHostSection host={host} />);
  expect(button(view).disabled).toBe(true);
  expect(mocks.synchronize).not.toHaveBeenCalled();
});

it("blocks duplicate actions while a control request is pending", () => {
  mocks.sync!.busy = true;
  const view = render(<OnlineServiceHostSection host={host} />);
  expect(button(view).textContent).toBe("Synchronizing...");
  expect(button(view).disabled).toBe(true);
  fireEvent.click(button(view));
  expect(mocks.disconnect).not.toHaveBeenCalled();
});

it("blocks actions while the first status query is pending", () => {
  mocks.sync = { enabled: false, busy: true, error: null, status: null };
  const view = render(<OnlineServiceHostSection host={host} />);
  expect(button(view).disabled).toBe(true);
});

it("allows retrying synchronization after a failed status query", () => {
  mocks.loggedOut = false;
  mocks.sync = { enabled: false, busy: false, error: "Connection lost", status: null };
  const view = render(<OnlineServiceHostSection host={host} />);
  expect(view.getByRole("alert").textContent).toBe("Connection lost");
  expect(button(view).disabled).toBe(false);
  fireEvent.click(button(view));
  expect(mocks.synchronize).toHaveBeenCalledWith("remote", host.label, true);
});
