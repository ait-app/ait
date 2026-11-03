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

vi.mock("./account-state", () => ({
  useAccountState: create(() => ({ status: "online", center: "https://center.test" })),
}));

const toast = { show: vi.fn(), error: vi.fn(), copied: vi.fn() };
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
  useAccountState.setState({ status: "online" });
  useHubServices.setState({ connection: null, snapshot: null });
  vi.clearAllMocks();
});
afterEach(cleanup);

describe("online service connection notice", () => {
  it("shows a neutral loading notice once per reconnection and allows a new notice after recovery", () => {
    mount();
    setState("online");
    setState("reconnecting");
    expect(toast.show).toHaveBeenCalledExactlyOnceWith(reconnecting, { variant: "loading" });
    setState("connecting");
    setState("reconnecting");
    expect(toast.show).toHaveBeenCalledOnce();

    setState("online");
    setState("reconnecting");
    expect(toast.show).toHaveBeenCalledTimes(2);
    expect(toast.error).not.toHaveBeenCalled();
  });

  it("keeps an unavailable connection as a loading notice while it retries", () => {
    mount();
    setState("reconnecting");
    setState("unavailable");
    expect(toast.show).toHaveBeenLastCalledWith(unavailable, {
      variant: "loading",
      durationMs: 3200,
    });
    setState("connecting");
    setState("reconnecting");
    setState("unavailable");
    expect(shown(reconnecting)).toHaveLength(1);
    expect(shown(unavailable)).toHaveLength(1);

    setState("online");
    setState("reconnecting");
    expect(shown(reconnecting)).toHaveLength(2);
    expect(toast.error).not.toHaveBeenCalled();
  });

  it("reports an initial connection timeout without repeating on retries", () => {
    mount();
    setState("connecting");
    expect(toast.show).not.toHaveBeenCalled();
    setState("unavailable");
    expect(shown(unavailable)).toHaveLength(1);
    setState("connecting");
    setState("unavailable");
    expect(toast.show).toHaveBeenCalledOnce();
    expect(toast.error).not.toHaveBeenCalled();
  });

  it("reports a connection that was already unavailable when the UI mounted", () => {
    setState("unavailable");
    mount();
    expect(shown(unavailable)).toHaveLength(1);
    expect(toast.error).not.toHaveBeenCalled();
  });

  it("does not report intentional shutdown or stale connection state after sign-out", () => {
    mount();
    setState("connecting");
    setState("stopped");
    act(() => useAccountState.setState({ status: "logged_out" }));
    setState("reconnecting");
    setState("unavailable");
    expect(toast.error).not.toHaveBeenCalled();
    expect(toast.show).not.toHaveBeenCalled();
  });
});
