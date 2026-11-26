/** @vitest-environment jsdom */
import React from "react";
import "@/i18n/i18next";
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { create } from "zustand";
import type { HubLinkState } from "@ait/client/internal/hub";
import { ToastApiProvider } from "@/contexts/toast-api-context";
import { useAccountState } from "./account-state";
import { useHubServices } from "./hub-service-store";
import { useOnlineServiceConnectionNotice } from "./use-online-service-connection-notice";

const host = vi.hoisted(() => ({ active: false, status: "online", restoring: false }));

vi.mock("./account-state", () => ({
  useAccountState: create(() => ({ status: "online", center: "https://center.test" })),
}));
vi.mock("./host-runtime", () => ({
  useHostRuntimeConnectionStatus: () => host.status,
  useHostRuntimeClient: () => ({
    subscribeRecoveryStatus: () => () => {},
    get isRestoringSubscriptions() {
      return host.restoring;
    },
  }),
}));
vi.mock("@/stores/navigation-active-workspace-store", () => ({
  useActiveWorkspaceSelection: () =>
    host.active ? { serverId: "host", workspaceId: "workspace" } : null,
}));

const toast = { show: vi.fn(), error: vi.fn(), copied: vi.fn() };
const dismiss = vi.fn();
const reconnecting = "Restoring connection…";
const unavailable = "Connecting to the online service…";

function setState(state: HubLinkState) {
  act(() => {
    useHubServices.setState({
      snapshot: { state, linkId: null, centerUrl: null, methods: [] },
    });
  });
}

function mount() {
  return renderHook(useOnlineServiceConnectionNotice, {
    wrapper: ({ children }) => (
      <React.StrictMode>
        <ToastApiProvider api={toast}>{children}</ToastApiProvider>
      </React.StrictMode>
    ),
  });
}

function shown(message: string) {
  return toast.show.mock.calls.filter(([content]) => content === message);
}

beforeEach(() => {
  Object.assign(host, { active: false, status: "online", restoring: false });
  useAccountState.setState({ status: "online" });
  useHubServices.setState({ connection: null, snapshot: null });
  vi.useFakeTimers();
  vi.clearAllMocks();
  toast.show.mockReturnValue(dismiss);
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

function advance(ms = 2_000) {
  act(() => vi.advanceTimersByTime(ms));
}

describe("online service connection notice", () => {
  it("holds one notice until the active host's business subscriptions recover", () => {
    host.active = true;
    const view = mount();
    setState("online");
    host.status = "connecting";
    view.rerender();
    setState("reconnecting");
    advance();
    expect(toast.show).toHaveBeenCalledOnce();
    host.status = "online";
    host.restoring = true;
    setState("online");
    expect(dismiss).not.toHaveBeenCalled();
    expect(useHubServices.getState().connectionRecovering).toBe(true);
    host.restoring = false;
    view.rerender();
    expect(dismiss).toHaveBeenCalledOnce();
    expect(useHubServices.getState().connectionRecovering).toBe(false);
  });
  it("delays the single notice and dismisses it after recovery", () => {
    mount();
    setState("online");
    setState("reconnecting");
    advance(1_999);
    expect(toast.show).not.toHaveBeenCalled();
    advance(1);
    expect(toast.show).toHaveBeenCalledExactlyOnceWith(reconnecting, { variant: "loading" });
    setState("unavailable");
    setState("connecting");
    setState("reconnecting");
    advance(20_000);
    expect(toast.show).toHaveBeenCalledOnce();
    setState("online");
    expect(dismiss).toHaveBeenCalledOnce();
    setState("reconnecting");
    advance();
    expect(toast.show).toHaveBeenCalledTimes(2);
  });

  it("does not show a notice for a short interruption", () => {
    mount();
    setState("online");
    setState("reconnecting");
    advance(1_000);
    setState("online");
    advance();
    expect(toast.show).not.toHaveBeenCalled();
  });

  it("keeps one initial connection notice through unavailable retries", () => {
    setState("unavailable");
    mount();
    advance();
    expect(shown(unavailable)).toHaveLength(1);
    setState("connecting");
    setState("unavailable");
    advance(20_000);
    expect(toast.show).toHaveBeenCalledOnce();
    expect(toast.error).not.toHaveBeenCalled();
    setState("online");
    expect(dismiss).toHaveBeenCalledOnce();
  });

  it("cancels delayed notices and dismisses existing ones on logout and unmount", () => {
    const view = mount();
    setState("online");
    setState("reconnecting");
    act(() => useAccountState.setState({ status: "logged_out" }));
    advance();
    expect(toast.show).not.toHaveBeenCalled();
    act(() => useAccountState.setState({ status: "online" }));
    advance();
    expect(toast.show).toHaveBeenCalledOnce();
    view.unmount();
    expect(dismiss).toHaveBeenCalledOnce();
    expect(vi.getTimerCount()).toBe(0);
  });

  it("does not report intentional shutdown", () => {
    mount();
    setState("connecting");
    setState("stopped");
    advance();
    expect(toast.show).not.toHaveBeenCalled();
    expect(toast.error).not.toHaveBeenCalled();
  });
});
