/** @vitest-environment jsdom */
import React from "react";
import { cleanup, fireEvent, render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { i18n } from "@/i18n/i18next";
import { defaultHostAppearance } from "@/hosts/appearance";
import { OnlineServiceHostSection } from "./online-service-host-section";

const mocks = vi.hoisted(() => ({
  disconnect: vi.fn(),
  synchronize: vi.fn(),
  loggedOut: true,
  enabled: true,
}));
vi.mock("@/runtime/account-state", () => ({
  useAccountState: () => ({
    status: mocks.loggedOut ? "logged_out" : "online",
    name: "Me",
    center: "https://example.test/api",
  }),
}));
vi.mock("@/runtime/host-runtime", () => ({
  useHostRuntimeClient: () => ({
    getLastServerInfoMessage: () => ({ features: { onlineServiceSync: true } }),
  }),
  useHostRuntimeIsConnected: () => true,
}));
vi.mock("@/runtime/online-service-host-sync", () => ({
  disconnectOnlineServiceHost: mocks.disconnect,
  synchronizeOnlineServiceHost: mocks.synchronize,
  useOnlineServiceHostSync: (select: (state: unknown) => unknown) =>
    select({
      hosts: {
        remote: {
          enabled: mocks.enabled,
          busy: false,
          error: null,
          status: { status: { online: mocks.enabled, connecting: false, error: null } },
        },
      },
    }),
}));
vi.mock("@/components/settings", () => {
  const Frame = ({ children }: { children: React.ReactNode }) => <div>{children}</div>;
  return { SettingsSection: Frame, SettingsCard: Frame, SettingsRow: Frame };
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
beforeEach(async () => {
  vi.clearAllMocks();
  mocks.loggedOut = true;
  mocks.enabled = true;
  await i18n.changeLanguage("en");
});
afterEach(() => cleanup());

describe("remote host synchronization after client logout", () => {
  it("keeps the explicit stop action available while the client is signed out", () => {
    const view = render(<OnlineServiceHostSection host={host} />);
    const stop = view.getByTestId("host-online-service-sync") as HTMLButtonElement;
    expect(stop.disabled).toBe(false);
    expect(stop.textContent).toBe("Stop synchronization");
    fireEvent.click(stop);
    expect(mocks.disconnect).toHaveBeenCalledExactlyOnceWith("remote");
  });

  it("requires sign-in to start a new host publication", () => {
    mocks.enabled = false;
    const view = render(<OnlineServiceHostSection host={host} />);
    const start = view.getByTestId("host-online-service-sync") as HTMLButtonElement;
    expect(start.disabled).toBe(true);
    fireEvent.click(start);
    expect(mocks.synchronize).not.toHaveBeenCalledWith("remote", host.label, true);
  });
});
