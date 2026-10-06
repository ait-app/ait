/** @vitest-environment jsdom */
import React from "react";
import "@/i18n/i18next";
import { cleanup, fireEvent, render, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { accountCommand, accountLoginMethods } from "@/runtime/account-state";
import { AccountHostPanel } from "./account-host-panel";

vi.mock("@/runtime/account-state", () => ({
  accountLoginMethods: vi.fn(async () => ({ hosted: false })),
  accountCommand: vi.fn(async () => ({})),
  useAccountState: () => ({
    status: "logged_out",
    center: "https://dash.ait-app.com:8443/api",
    error: null,
  }),
}));

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("AccountHostPanel email login", () => {
  it("offers browser registration/login when supported and retains explicit legacy login", async () => {
    vi.mocked(accountLoginMethods).mockResolvedValueOnce({ hosted: true });
    const view = render(<AccountHostPanel />);
    await waitFor(() => expect(view.getByTestId("account-unified-login")).toBeTruthy());
    expect(view.queryByTestId("account-password")).toBeNull();
    fireEvent.click(view.getByTestId("account-legacy-login"));
    expect(view.getByTestId("account-password")).toBeTruthy();
    fireEvent.click(view.getByTestId("account-unified-login"));
    await waitFor(() =>
      expect(accountCommand).toHaveBeenCalledWith("account_login_hosted", {
        center: "https://dash.ait-app.com:8443/api",
      }),
    );
  });
  it("submits email credentials through IPC and clears the password field", async () => {
    const view = render(<AccountHostPanel />);
    const email = view.getByLabelText("Email") as HTMLInputElement;
    const password = view.getByLabelText("Password") as HTMLInputElement;
    expect(email.getAttribute("inputmode")).toBe("email");
    fireEvent.change(email, { target: { value: "owl@example.com" } });
    fireEvent.change(password, { target: { value: "  private password  " } });
    fireEvent.click(view.getByTestId("account-login"));
    await waitFor(() => {
      expect(accountCommand).toHaveBeenCalledExactlyOnceWith("account_login", {
        center: "https://dash.ait-app.com:8443/api",
        email: "owl@example.com",
        password: "  private password  ",
      });
    });
    expect(password.value).toBe("");
  });
});
