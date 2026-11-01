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
const message = "Connection to the online server was lost. Reconnecting automatically.";

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
    expect(toast.show).toHaveBeenCalledExactlyOnceWith("Restoring connection…", {
      variant: "loading",
    });
    expect(toast.error).not.toHaveBeenCalled();
    setState("connecting");
    setState("reconnecting");
    expect(toast.show).toHaveBeenCalledOnce();
    expect(toast.error).not.toHaveBeenCalled();

    setState("online");
    setState("reconnecting");
    expect(toast.show).toHaveBeenCalledTimes(2);
    expect(toast.error).not.toHaveBeenCalled();
  });

  it("escalates to an error only when reconnection becomes unavailable", () => {
    mount();
    setState("reconnecting");
    setState("unavailable");
    expect(toast.error).toHaveBeenCalledExactlyOnceWith(message);
    setState("connecting");
    setState("reconnecting");
    setState("unavailable");
    expect(toast.show).toHaveBeenCalledOnce();
    expect(toast.error).toHaveBeenCalledOnce();

    setState("online");
    setState("reconnecting");
    expect(toast.show).toHaveBeenCalledTimes(2);
  });

  it("reports an initial connection timeout without repeating on retries", () => {
    mount();
    setState("connecting");
    expect(toast.error).not.toHaveBeenCalled();
    expect(toast.show).not.toHaveBeenCalled();
    setState("unavailable");
    expect(toast.error).toHaveBeenCalledExactlyOnceWith(message);
    setState("connecting");
    setState("unavailable");
    expect(toast.error).toHaveBeenCalledOnce();
  });

  it("reports a connection that was already unavailable when the UI mounted", () => {
    setState("unavailable");
    mount();
    expect(toast.error).toHaveBeenCalledExactlyOnceWith(message);
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
