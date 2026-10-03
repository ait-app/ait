/** @vitest-environment jsdom */
import React from "react";
import { cleanup, fireEvent, render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { Platform } from "react-native";
import { WelcomeScreen } from "./welcome-screen";

const mocks = vi.hoisted(() => ({
  command: vi.fn(async () => ({})),
  push: vi.fn(),
  replace: vi.fn(),
  desktop: false,
  runtime: { subscribeAll: () => () => {}, getSnapshot: () => undefined },
}));
vi.mock("react-native", async (original) => {
  const actual = await original<typeof import("react-native")>();
  return { ...actual, Platform: { ...actual.Platform, OS: "android" } };
});
vi.mock("@/desktop/host", () => ({
  isElectronRuntime: () => mocks.desktop,
  getDesktopHost: () => (mocks.desktop ? { invoke: vi.fn() } : undefined),
}));
vi.mock("@/runtime/host-runtime", () => ({
  useHosts: () => [],
  getHostRuntimeStore: () => mocks.runtime,
  isHostRuntimeConnected: () => false,
}));
vi.mock("@/runtime/account-state", async (original) => ({
  ...(await original<typeof import("@/runtime/account-state")>()),
  useAccountState: () => ({
    status: "logged_out",
    center: "https://dash.ait-app.com:8443/api",
    error: null,
  }),
  accountCommand: mocks.command,
}));
vi.mock("expo-router", () => ({ useRouter: () => ({ push: mocks.push, replace: mocks.replace }) }));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock("@/utils/app-version", () => ({ resolveAppVersion: () => "0.0.14" }));
vi.mock("@/utils/open-external-url", () => ({ openExternalUrl: vi.fn() }));
vi.mock("@/desktop/updates/desktop-updates", () => ({
  formatVersionWithPrefix: (version: string) => `v${version}`,
}));
vi.mock("./icons/ait-logo", () => ({ AitLogo: () => <div>Ait</div> }));
vi.mock("./add-host-modal", () => ({
  AddHostModal: ({ visible }: { visible: boolean }) =>
    visible ? <div data-testid="direct-form" /> : null,
}));
vi.mock("./add-remote-ssh-host-modal", () => ({ AddRemoteSshHostModal: () => null }));
vi.mock("./adaptive-modal-sheet", () => ({
  AdaptiveModalSheet: ({ visible, children }: { visible: boolean; children: React.ReactNode }) =>
    visible ? <div>{children}</div> : null,
}));

beforeEach(() => {
  Platform.OS = "android";
  mocks.desktop = false;
  vi.clearAllMocks();
});
afterEach(cleanup);

describe("welcome account entry", () => {
  it("lets an Android user with no hosts reach and submit the login form directly from welcome", async () => {
    const view = render(<WelcomeScreen />);
    expect(view.queryByTestId("account-email")).toBeNull();
    const account = view.getByTestId("welcome-account-relay");
    const direct = view.getByTestId("welcome-direct-connection");
    expect(account.compareDocumentPosition(direct) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    fireEvent.click(account);
    expect(view.getByTestId("account-host-panel")).toBeTruthy();
    fireEvent.change(view.getByLabelText("Email"), { target: { value: "me@example.test" } });
    fireEvent.change(view.getByLabelText("Password"), { target: { value: "test-password" } });
    fireEvent.click(view.getByTestId("account-login"));
    await waitFor(() =>
      expect(mocks.command).toHaveBeenCalledWith("account_login", {
        center: "https://dash.ait-app.com:8443/api",
        email: "me@example.test",
        password: "test-password",
      }),
    );
    expect(mocks.push).not.toHaveBeenCalled();
  });

  it("keeps the direct connection entry usable", () => {
    const view = render(<WelcomeScreen />);
    fireEvent.click(view.getByTestId("welcome-direct-connection"));
    expect(view.getByTestId("direct-form")).toBeTruthy();
    expect(view.queryByTestId("account-host-panel")).toBeNull();
  });

  it.each(["ios", "web"] as const)(
    "does not advertise account relay on unsupported %s",
    (platform) => {
      Platform.OS = platform;
      const view = render(<WelcomeScreen />);
      expect(view.queryByTestId("welcome-account-relay")).toBeNull();
      expect(view.getByTestId("welcome-direct-connection")).toBeTruthy();
    },
  );

  it("also exposes the account entry in Electron", () => {
    Platform.OS = "web";
    mocks.desktop = true;
    const view = render(<WelcomeScreen />);
    expect(view.getByTestId("welcome-account-relay")).toBeTruthy();
    expect(view.getByTestId("welcome-remote-ssh")).toBeTruthy();
  });
});
