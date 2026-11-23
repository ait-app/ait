/** @vitest-environment jsdom */
import React from "react";
import { cleanup, fireEvent, render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AccountHost } from "@ait/client/internal/account-session";
import { SignedInScreen } from "./signed-in-screen";

function accountHost(overrides: Partial<AccountHost>): AccountHost {
  return {
    host_id: "host",
    node_id: "node",
    server_id: "server",
    instance_id: "instance",
    name: "Workstation",
    platform: "linux",
    relay_modes: [],
    online: true,
    status: "active",
    ...overrides,
  };
}

const mocks = vi.hoisted(() => ({
  replace: vi.fn(),
  push: vi.fn(),
  onlineServerId: null as string | null,
  browserAccount: false,
  account: {
    status: "online",
    name: "Ada",
    hosts: [] as AccountHost[],
  },
}));
vi.mock("expo-router", () => ({ useRouter: () => ({ replace: mocks.replace, push: mocks.push }) }));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, values?: Record<string, string>) =>
      values ? `${key}:${JSON.stringify(values)}` : key,
  }),
}));
vi.mock("@/app/_layout", () => ({
  useEarliestOnlineHostServerId: () => mocks.onlineServerId,
}));
vi.mock("@/runtime/account-state", () => ({
  useAccountState: <T,>(select?: (state: typeof mocks.account) => T) =>
    select ? select(mocks.account) : mocks.account,
}));
vi.mock("@/runtime/browser-account", () => ({
  supportsBrowserAccount: () => mocks.browserAccount,
}));
vi.mock("./icons/ait-logo", () => ({ AitLogo: () => <div>Ait</div> }));

beforeEach(() => {
  vi.clearAllMocks();
  mocks.onlineServerId = null;
  mocks.browserAccount = false;
  mocks.account = { status: "online", name: "Ada", hosts: [] };
});
afterEach(cleanup);

describe("signed-in welcome page", () => {
  it("greets the signed-in account and lists its hosts", () => {
    mocks.account.hosts = [
      accountHost({ host_id: "a", name: "Workstation", online: true }),
      accountHost({ host_id: "b", name: "Laptop", online: false }),
    ];
    const view = render(<SignedInScreen />);
    expect(view.getByText("onboarding.signedIn.title")).toBeTruthy();
    expect(view.getByText('onboarding.signedIn.account:{"name":"Ada"}')).toBeTruthy();
    expect(view.getByTestId("signed-in-host-a").textContent).toContain("Workstation");
    expect(view.getByTestId("signed-in-host-a").textContent).toContain(
      "common.connectionStatus.online",
    );
    expect(view.getByTestId("signed-in-host-b").textContent).toContain(
      "onboarding.signedIn.offline",
    );
  });

  it("explains how to add a host when the account has none", () => {
    const view = render(<SignedInScreen />);
    expect(view.getByText("onboarding.signedIn.noHosts")).toBeTruthy();
  });

  it("opens projects once a host is connected", () => {
    mocks.account.hosts = [accountHost({ host_id: "a" })];
    const view = render(<SignedInScreen />);
    fireEvent.click(view.getByTestId("signed-in-continue"));
    expect(mocks.replace).not.toHaveBeenCalled();
    expect(view.getByText("onboarding.signedIn.waiting")).toBeTruthy();

    mocks.onlineServerId = "server";
    view.rerender(<SignedInScreen />);
    expect(view.queryByText("onboarding.signedIn.waiting")).toBeNull();
    fireEvent.click(view.getByTestId("signed-in-continue"));
    expect(mocks.replace).toHaveBeenCalledExactlyOnceWith("/open-project");
  });

  it("offers the other connection methods", () => {
    const view = render(<SignedInScreen />);
    fireEvent.click(view.getByTestId("signed-in-other-connection"));
    expect(mocks.replace).toHaveBeenCalledExactlyOnceWith("/welcome");
  });

  it("hides other connection methods in the website client but keeps settings reachable", () => {
    mocks.browserAccount = true;
    const view = render(<SignedInScreen />);
    expect(view.queryByTestId("signed-in-other-connection")).toBeNull();
    fireEvent.click(view.getByTestId("signed-in-open-settings"));
    expect(mocks.push).toHaveBeenCalledExactlyOnceWith("/settings");
  });

  it("omits the account line when the account has no display name", () => {
    mocks.account.name = "";
    const view = render(<SignedInScreen />);
    expect(view.queryByText(/onboarding\.signedIn\.account/)).toBeNull();
  });

  it("returns to the welcome page when the account signs out", () => {
    const view = render(<SignedInScreen />);
    expect(mocks.replace).not.toHaveBeenCalled();
    mocks.account = { ...mocks.account, status: "logged_out" };
    view.rerender(<SignedInScreen />);
    expect(mocks.replace).toHaveBeenCalledExactlyOnceWith("/welcome");
  });
});
