/** @vitest-environment jsdom */
import React from "react";
import { act, cleanup, fireEvent, render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { i18n } from "@/i18n/i18next";
import type { AccountInfo } from "@ait/client/internal/account-session";
import { OnlineServiceSection } from "./online-service-section";

const mocks = vi.hoisted(() => ({
  load: vi.fn<() => Promise<AccountInfo>>(),
  account: {
    status: "online",
    center: "https://example.test/api",
    name: "Nickname",
    accountId: "first",
  },
  visible: true,
}));
vi.mock("@/runtime/account-state", () => ({
  useAccountState: () => mocks.account,
  loadAccountInfo: mocks.load,
}));
vi.mock("@/hooks/use-app-visible", () => ({ useAppVisible: () => mocks.visible }));
vi.mock("@/components/account-host-panel", () => ({
  AccountHostPanel: ({ onRefresh }: { onRefresh: () => void }) => (
    <button onClick={onRefresh}>Refresh</button>
  ),
}));

beforeEach(async () => {
  vi.clearAllMocks();
  mocks.visible = true;
  mocks.account.status = "online";
  mocks.account.accountId = "first";
  mocks.load.mockResolvedValue({
    loginName: "me@example.test",
    expiresAt: null,
  });
  await i18n.changeLanguage("en");
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

it("shows the login name and unlimited account access below sign-in", async () => {
  const view = render(<OnlineServiceSection />);
  await waitFor(() =>
    expect(view.getByTestId("account-info-login-name").textContent).toContain("me@example.test"),
  );
  expect(view.getByTestId("account-info-expiry").textContent).toContain("Never expires");
  expect(
    view
      .getByTestId("settings-online-service")
      .compareDocumentPosition(view.getByTestId("settings-account-info")) &
      Node.DOCUMENT_POSITION_FOLLOWING,
  ).toBeTruthy();
});

it("refreshes account expiry from the existing Refresh action", async () => {
  const view = render(<OnlineServiceSection />);
  await waitFor(() => expect(mocks.load).toHaveBeenCalledOnce());
  const expiresAt = "2027-03-04T05:06:07Z";
  mocks.load.mockResolvedValue({
    loginName: "me@example.test",
    expiresAt,
  });
  fireEvent.click(view.getByText("Refresh"));
  await waitFor(() =>
    expect(view.getByTestId("account-info-expiry").textContent).toContain(
      new Date(expiresAt).toLocaleString("en"),
    ),
  );
});

it("reports failures and clears stale account information", async () => {
  const view = render(<OnlineServiceSection />);
  await waitFor(() =>
    expect(view.getByTestId("account-info-login-name").textContent).toContain("me@example.test"),
  );
  mocks.load.mockRejectedValue(new Error("offline"));
  fireEvent.click(view.getByText("Refresh"));
  await waitFor(() =>
    expect(view.getByRole("alert").textContent).toContain("Could not load account information"),
  );
  expect(view.getByTestId("account-info-expiry").textContent).toBe("Account expires at—");
  expect(view.getByTestId("account-info-login-name").textContent).toBe("Login name—");
});

it("discards delayed results after an account switch and hides the section on logout", async () => {
  let finish!: (value: AccountInfo) => void;
  mocks.load.mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  const view = render(<OnlineServiceSection />);
  mocks.account.accountId = "second";
  mocks.load.mockResolvedValue({
    loginName: "new@example.test",
    expiresAt: null,
  });
  view.rerender(<OnlineServiceSection />);
  await waitFor(() =>
    expect(view.getByTestId("account-info-login-name").textContent).toContain("new@example.test"),
  );
  await act(async () => finish({ loginName: "old@example.test", expiresAt: null }));
  expect(view.queryByText("old@example.test")).toBeNull();
  mocks.account.status = "logged_out";
  view.rerender(<OnlineServiceSection />);
  expect(view.queryByTestId("settings-account-info")).toBeNull();
});

it("polls while visible and stops polling while hidden or unmounted", async () => {
  vi.useFakeTimers();
  const view = render(<OnlineServiceSection />);
  await act(async () => {});
  expect(mocks.load).toHaveBeenCalledOnce();
  await act(() => vi.advanceTimersByTimeAsync(15_000));
  expect(mocks.load).toHaveBeenCalledTimes(2);
  mocks.visible = false;
  view.rerender(<OnlineServiceSection />);
  await act(() => vi.advanceTimersByTimeAsync(30_000));
  expect(mocks.load).toHaveBeenCalledTimes(2);
  mocks.visible = true;
  view.rerender(<OnlineServiceSection />);
  await act(async () => {});
  expect(mocks.load).toHaveBeenCalledTimes(3);
  view.unmount();
  await act(() => vi.advanceTimersByTimeAsync(30_000));
  expect(mocks.load).toHaveBeenCalledTimes(3);
});
