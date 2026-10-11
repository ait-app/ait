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
}));
vi.mock("react-native", async (original) => {
  const actual = await original<typeof import("react-native")>();
  return { ...actual, Platform: { ...actual.Platform, OS: "android" } };
});
vi.mock("@/runtime/account-state", () => ({
  useAccountState: () => state.value,
  accountCommand: state.command,
}));
afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("Android account panel", () => {
  it("shows account controls without a separate online host picker", async () => {
    const view = render(<AccountHostPanel />);
    expect(view.getByText(/Connected to account/)).toBeTruthy();
    expect(view.queryByText(/Waiting for this host/)).toBeNull();
    expect(view.queryByTestId("account-host-host")).toBeNull();
    expect(view.queryByText("My computer")).toBeNull();
    expect(view.queryByText("Disconnect remote host")).toBeNull();
    expect(view.getByText("Online hosts are automatically added to the host list.")).toBeTruthy();
    fireEvent.click(view.getByRole("button", { name: "Refresh" }));
    await waitFor(() => expect(state.command).toHaveBeenCalledExactlyOnceWith("account_refresh"));
  });

  it("keeps sign out available", async () => {
    const view = render(<AccountHostPanel />);
    fireEvent.click(view.getByRole("button", { name: "Sign out" }));
    await waitFor(() => expect(state.command).toHaveBeenCalledExactlyOnceWith("account_logout"));
  });
});
