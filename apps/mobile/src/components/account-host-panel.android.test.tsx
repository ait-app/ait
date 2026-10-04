/** @vitest-environment jsdom */
import React from "react";
import "@/i18n/i18next";
import { cleanup, fireEvent, render, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { AccountHostPanel } from "./account-host-panel";

const state = vi.hoisted(() => ({
  value: {
    status: "online",
    center: "https://dash.ait-app.com:8443/api",
    name: "Me",
    hostOnline: false,
    stale: false,
    error: null,
    selected: null,
    hosts: [{ host_id: "host", server_id: "server", name: "My computer", platform: "linux" }],
  },
  command: vi.fn(async () => ({})),
  push: vi.fn(),
}));
vi.mock("react-native", async (original) => {
  const actual = await original<typeof import("react-native")>();
  return { ...actual, Platform: { ...actual.Platform, OS: "android" } };
});
vi.mock("@/runtime/account-state", () => ({
  useAccountState: () => state.value,
  accountCommand: state.command,
}));
vi.mock("expo-router", () => ({ useRouter: () => ({ push: state.push }) }));
afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("Android account panel", () => {
  it("shows a client connection and lets the user select an online computer", async () => {
    const connected = vi.fn();
    const view = render(<AccountHostPanel onConnected={connected} />);
    expect(view.getByText(/Connected to account/)).toBeTruthy();
    expect(view.queryByText(/Waiting for this host/)).toBeNull();
    fireEvent.click(view.getByTestId("account-host-host"));
    await waitFor(() =>
      expect(state.command).toHaveBeenCalledWith("account_select", { hostId: "host" }),
    );
    expect(connected).toHaveBeenCalledExactlyOnceWith("server");
    expect(state.push).not.toHaveBeenCalled();
  });

  it("navigates directly when the panel is outside a sheet", async () => {
    const view = render(<AccountHostPanel />);
    fireEvent.click(view.getByTestId("account-host-host"));
    await waitFor(() => expect(state.push).toHaveBeenCalledWith("/h/server"));
    expect(state.push).toHaveBeenCalledWith("/h/server");
  });
});
