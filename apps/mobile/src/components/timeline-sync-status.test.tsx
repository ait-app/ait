// @vitest-environment jsdom
import React from "react";
import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { i18n } from "@/i18n/i18next";
import { useHubServices } from "@/runtime/hub-service-store";
import { TimelineSyncStatus } from "./timeline-sync-status";

vi.mock("@/components/toast-host", () => ({
  ToastViewport: ({ toast }: { toast: { content: string } | null }) =>
    toast ? <div data-testid="notice">{toast.content}</div> : null,
}));

beforeEach(async () => {
  await i18n.changeLanguage("en");
  useHubServices.setState({ connectionRecovering: false });
  vi.useFakeTimers();
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

it("retains a stale-data marker and delays the single local connection notice", () => {
  const view = render(
    <TimelineSyncStatus sync={{ status: "reconnecting" }} toast={null} onDismiss={() => {}} />,
  );
  expect(screen.getByText("Saved messages · pending update")).toBeTruthy();
  expect(screen.queryByTestId("notice")).toBeNull();
  act(() => vi.advanceTimersByTime(2_000));
  expect(screen.getByTestId("notice").textContent).toBe("Reconnecting to host");
  view.rerender(
    <TimelineSyncStatus
      sync={{ status: "catching_up", ui: "status" }}
      toast={null}
      onDismiss={() => {}}
    />,
  );
  expect(screen.getAllByTestId("notice")).toHaveLength(1);
  view.rerender(<TimelineSyncStatus sync={{ status: "idle" }} toast={null} onDismiss={() => {}} />);
  expect(screen.queryByTestId("notice")).toBeNull();
  expect(screen.queryByText("Saved messages · pending update")).toBeNull();
});

it("coalesces connection notices while preserving explicit operation errors", () => {
  useHubServices.setState({ connectionRecovering: true });
  const view = render(
    <TimelineSyncStatus sync={{ status: "reconnecting" }} toast={null} onDismiss={() => {}} />,
  );
  act(() => vi.advanceTimersByTime(2_000));
  expect(screen.queryByTestId("notice")).toBeNull();
  view.rerender(
    <TimelineSyncStatus
      sync={{ status: "reconnecting" }}
      toast={{
        id: 99,
        content: "Provider operation failed",
        icon: null,
        variant: "error",
        durationMs: null,
      }}
      onDismiss={() => {}}
    />,
  );
  expect(screen.getByTestId("notice").textContent).toBe("Provider operation failed");
});

it("cancels obsolete delayed notices on unmount", () => {
  const view = render(
    <TimelineSyncStatus sync={{ status: "reconnecting" }} toast={null} onDismiss={() => {}} />,
  );
  view.unmount();
  expect(vi.getTimerCount()).toBe(0);
});
