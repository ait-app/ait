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
    expect(view.getByTestId("account-login")).toBeTruthy();
    fireEvent.click(view.getByTestId("account-legacy-login"));
    expect(view.queryByTestId("account-password")).toBeNull();
    fireEvent.click(view.getByTestId("account-unified-login"));
    await waitFor(() =>
      expect(accountCommand).toHaveBeenCalledWith("account_login_hosted", {
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
    fireEvent.change(view.getByLabelText("Email"), { target: { value: "owl@example.com" } });
    fireEvent.change(view.getByLabelText("Password"), { target: { value: "password" } });
    fireEvent.click(view.getByTestId("account-login"));
    await waitFor(() =>
      expect(accountCommand).toHaveBeenCalledWith("account_login", {
        center: "https://private.example/api",
        email: "owl@example.com",
        password: "password",
      }),
    );
    await waitFor(() =>
      expect(view.getByRole("button", { name: "Cancel" }).hasAttribute("disabled")).toBe(false),
    );
    fireEvent.click(view.getByRole("button", { name: "Cancel" }));
    expect(cancel).toHaveBeenCalledOnce();
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
