/** @vitest-environment jsdom */
import React from "react";
import "@/i18n/i18next";
import { cleanup, fireEvent, render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { accountCommand } from "@/runtime/account-state";
import { AccountHostPanel } from "./account-host-panel";

const state = vi.hoisted(() => ({ loginPending: false }));
vi.mock("@/runtime/account-state", () => ({
  accountCommand: vi.fn(async () => ({})),
  useAccountState: () => ({
    status: "logged_out",
    center: "https://dash.ait-app.com:8443/api",
    error: null,
    loginPending: state.loginPending,
  }),
}));

beforeEach(() => {
  state.loginPending = false;
});
afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("AccountHostPanel browser login", () => {
  it("offers only browser registration/login from the first render", async () => {
    const view = render(<AccountHostPanel />);
    expect(view.queryByTestId("account-email")).toBeNull();
    expect(view.queryByTestId("account-password")).toBeNull();
    expect(view.queryByTestId("account-legacy-login")).toBeNull();
    expect(view.queryByTestId("account-login")).toBeNull();
    fireEvent.click(view.getByTestId("account-unified-login"));
    await waitFor(() =>
      expect(accountCommand).toHaveBeenCalledExactlyOnceWith("account_login_hosted", {
        center: "https://dash.ait-app.com:8443/api",
      }),
    );
  });

  it("keeps service settings optional and submits the edited service address", async () => {
    const cancel = vi.fn();
    const view = render(<AccountHostPanel onCancel={cancel} />);
    expect(view.queryByTestId("account-center")).toBeNull();
    fireEvent.click(view.getByTestId("account-service-settings"));
    fireEvent.change(view.getByLabelText("Service URL"), {
      target: { value: "https://private.example/api" },
    });
    fireEvent.click(view.getByTestId("account-unified-login"));
    await waitFor(() =>
      expect(accountCommand).toHaveBeenCalledExactlyOnceWith("account_login_hosted", {
        center: "https://private.example/api",
      }),
    );
    await waitFor(() =>
      expect(view.getByRole("button", { name: "Cancel" }).hasAttribute("disabled")).toBe(false),
    );
    fireEvent.click(view.getByRole("button", { name: "Cancel" }));
    expect(cancel).toHaveBeenCalledOnce();
  });

  it.each([
    "This service does not support client browser sign-in. Update the service or choose another service URL.",
    "Network request failed",
  ])("shows a login failure without offering a password fallback: %s", async (message) => {
    vi.mocked(accountCommand).mockRejectedValueOnce(new Error(message));
    const view = render(<AccountHostPanel />);
    fireEvent.click(view.getByTestId("account-unified-login"));
    await waitFor(() => expect(view.getByRole("alert").textContent).toBe(message));
    expect(view.queryByTestId("account-email")).toBeNull();
    expect(view.queryByTestId("account-password")).toBeNull();
    expect(view.queryByTestId("account-legacy-login")).toBeNull();
    fireEvent.click(view.getByTestId("account-unified-login"));
    await waitFor(() => expect(accountCommand).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(view.queryByRole("alert")).toBeNull());
    expect(
      vi.mocked(accountCommand).mock.calls.every(([command]) => command === "account_login_hosted"),
    ).toBe(true);
  });

  it("lets the user cancel a pending browser login", async () => {
    state.loginPending = true;
    const view = render(<AccountHostPanel />);
    expect(view.getByTestId("account-unified-login").hasAttribute("disabled")).toBe(true);
    fireEvent.click(view.getByTestId("account-cancel-login"));
    await waitFor(() =>
      expect(accountCommand).toHaveBeenCalledExactlyOnceWith("account_cancel_login"),
    );
  });
});
