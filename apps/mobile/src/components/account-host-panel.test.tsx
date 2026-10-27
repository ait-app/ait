/** @vitest-environment jsdom */
import React from "react";
import "@/i18n/i18next";
import { act, cleanup, fireEvent, render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { accountCommand } from "@/runtime/account-state";
import { confirmDialog } from "@/utils/confirm-dialog";
import { AccountHostPanel } from "./account-host-panel";

const state = vi.hoisted(() => ({ loginPending: false, status: "logged_out", accountId: "first" }));
vi.mock("@/utils/confirm-dialog", () => ({ confirmDialog: vi.fn(async () => false) }));
vi.mock("@/runtime/account-state", () => ({
  accountCommand: vi.fn(async () => ({ status: "connecting" })),
  useAccountState: () => ({
    status: state.status,
    accountId: state.accountId,
    center: "https://aitapp.cn/api",
    error: null,
    loginPending: state.loginPending,
  }),
}));

beforeEach(() => {
  state.loginPending = false;
  state.status = "logged_out";
  state.accountId = "first";
});
afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("AccountHostPanel browser login", () => {
  it("offers only browser registration/login from the first render", async () => {
    const signedIn = vi.fn();
    const view = render(<AccountHostPanel onSignedIn={signedIn} />);
    expect(view.queryByTestId("account-email")).toBeNull();
    expect(view.queryByTestId("account-password")).toBeNull();
    expect(view.queryByTestId("account-legacy-login")).toBeNull();
    expect(view.queryByTestId("account-login")).toBeNull();
    expect(view.queryByTestId("account-delete")).toBeNull();
    fireEvent.click(view.getByTestId("account-unified-login"));
    await waitFor(() =>
      expect(accountCommand).toHaveBeenCalledExactlyOnceWith("account_login_hosted", {
        center: "https://aitapp.cn/api",
      }),
    );
    expect(signedIn).toHaveBeenCalledOnce();
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
    const signedIn = vi.fn();
    const view = render(<AccountHostPanel onSignedIn={signedIn} />);
    fireEvent.click(view.getByTestId("account-unified-login"));
    await waitFor(() => expect(view.getByRole("alert").textContent).toBe(message));
    expect(signedIn).not.toHaveBeenCalled();
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

describe("AccountHostPanel account deletion", () => {
  beforeEach(() => {
    state.status = "online";
  });

  it("places deletion before sign out and leaves the account alone when cancelled", async () => {
    const view = render(<AccountHostPanel />);
    const remove = view.getByRole("button", { name: "Delete account" });
    const signOut = view.getByRole("button", { name: "Sign out" });
    expect(remove.parentElement).toBe(signOut.parentElement);
    expect(remove.compareDocumentPosition(signOut) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    fireEvent.click(remove);
    await waitFor(() => expect(confirmDialog).toHaveBeenCalledOnce());
    expect(confirmDialog).toHaveBeenCalledWith({
      title: "Delete your account?",
      message: expect.stringContaining("This cannot be undone"),
      confirmLabel: "Delete account",
      cancelLabel: "Cancel",
      destructive: true,
    });
    await waitFor(() => expect(remove.hasAttribute("disabled")).toBe(false));
    expect(accountCommand).not.toHaveBeenCalled();
  });

  it("waits for confirmation and prevents duplicate requests until deletion completes", async () => {
    let confirm!: (value: boolean) => void;
    vi.mocked(confirmDialog).mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          confirm = resolve;
        }),
    );
    let finish!: (value: Awaited<ReturnType<typeof accountCommand>>) => void;
    vi.mocked(accountCommand).mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    const view = render(<AccountHostPanel />);
    const remove = view.getByTestId("account-delete");
    fireEvent.click(remove);
    fireEvent.click(remove);
    expect(confirmDialog).toHaveBeenCalledOnce();
    expect(accountCommand).not.toHaveBeenCalled();
    await act(async () => confirm(true));
    expect(accountCommand).toHaveBeenCalledExactlyOnceWith("account_delete");
    expect(remove.hasAttribute("disabled")).toBe(true);
    expect(view.getByRole("button", { name: "Sign out" }).hasAttribute("disabled")).toBe(true);
    fireEvent.click(remove);
    await act(async () =>
      finish({ status: "logged_out" } as Awaited<ReturnType<typeof accountCommand>>),
    );
    expect(accountCommand).toHaveBeenCalledOnce();
    expect(remove.hasAttribute("disabled")).toBe(false);
  });

  it("reports deletion failures and lets the user retry", async () => {
    vi.mocked(confirmDialog).mockResolvedValueOnce(true);
    vi.mocked(accountCommand).mockRejectedValueOnce(new Error("Account service unavailable"));
    const view = render(<AccountHostPanel />);
    fireEvent.click(view.getByTestId("account-delete"));
    await waitFor(() =>
      expect(view.getByRole("alert").textContent).toBe("Account service unavailable"),
    );
    expect(view.getByTestId("account-delete").hasAttribute("disabled")).toBe(false);
    expect(view.getByRole("button", { name: "Sign out" })).toBeTruthy();
    expect(accountCommand).toHaveBeenCalledExactlyOnceWith("account_delete");
    vi.mocked(confirmDialog).mockResolvedValueOnce(true);
    fireEvent.click(view.getByTestId("account-delete"));
    await waitFor(() => expect(accountCommand).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(view.queryByRole("alert")).toBeNull());
  });

  it("does not delete a different account after a delayed confirmation", async () => {
    let confirm!: (value: boolean) => void;
    vi.mocked(confirmDialog).mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          confirm = resolve;
        }),
    );
    const view = render(<AccountHostPanel />);
    fireEvent.click(view.getByTestId("account-delete"));
    state.accountId = "second";
    view.rerender(<AccountHostPanel />);
    await act(async () => confirm(true));
    expect(accountCommand).not.toHaveBeenCalled();
  });
});
