// @vitest-environment jsdom
import React from "react";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  get: vi.fn(),
  subscribe: vi.fn(),
  supported: true,
  os: "web",
  invoke: undefined as unknown,
}));
vi.mock("./browser-account", () => ({
  getBrowserAccount: mocks.get,
  subscribeBrowserAccount: mocks.subscribe,
  supportsBrowserAccount: () => mocks.supported,
}));
vi.mock("@/desktop/host", () => ({ getDesktopHost: () => ({ invoke: mocks.invoke }) }));
vi.mock("react-native", () => ({
  Platform: {
    get OS() {
      return mocks.os;
    },
  },
  View: ({ children }: React.PropsWithChildren) => <div>{children}</div>,
  Text: ({ children }: React.PropsWithChildren) => <span>{children}</span>,
  Button: ({ title, onPress }: { title: string; onPress: () => void }) => (
    <button onClick={onPress}>{title}</button>
  ),
}));
import { BrowserAccountGate } from "./browser-account-gate";

let receive: (state: { status: string }) => void;
let replace: ReturnType<typeof vi.fn>;
let originalLocation: Location;
const snapshot = (status: string) => ({ snapshot: () => ({ status }) });
const mount = () =>
  render(
    <BrowserAccountGate>
      <div>Cached workspace</div>
    </BrowserAccountGate>,
  );
beforeEach(() => {
  vi.clearAllMocks();
  mocks.os = "web";
  mocks.supported = true;
  mocks.invoke = undefined;
  localStorage.clear();
  mocks.subscribe.mockImplementation((listener) => {
    receive = listener;
    return vi.fn();
  });
  mocks.get.mockResolvedValue(snapshot("connecting"));
  originalLocation = window.location;
  replace = vi.fn();
  Object.defineProperty(window, "location", { configurable: true, value: { replace } });
});
afterEach(() => {
  cleanup();
  Object.defineProperty(window, "location", { configurable: true, value: originalLocation });
});

describe("hosted browser account gate", () => {
  it("hides cached workspace until account validation finishes", async () => {
    let resolve!: (value: unknown) => void;
    mocks.get.mockReturnValue(
      new Promise((done) => {
        resolve = done;
      }),
    );
    mount();
    expect(screen.queryByText("Cached workspace")).toBeNull();
    await act(async () => resolve(snapshot("connecting")));
    expect(screen.getByText("Cached workspace")).toBeTruthy();
    expect(replace).not.toHaveBeenCalled();
  });
  it("replaces signed-out startup with the website login", async () => {
    mocks.get.mockResolvedValue(snapshot("logged_out"));
    mount();
    await waitFor(() => expect(replace).toHaveBeenCalledWith("/login?client=1"));
    expect(screen.queryByText("Cached workspace")).toBeNull();
  });
  it("unmounts workspace and redirects once when logout or expiry is published", async () => {
    mount();
    await screen.findByText("Cached workspace");
    act(() => {
      receive({ status: "logged_out" });
      receive({ status: "logged_out" });
    });
    expect(screen.queryByText("Cached workspace")).toBeNull();
    expect(replace).toHaveBeenCalledTimes(1);
  });
  it("blocks stale content immediately when another tab signs out", async () => {
    mount();
    await screen.findByText("Cached workspace");
    act(() =>
      window.dispatchEvent(
        new StorageEvent("storage", {
          key: "ait.access-token",
          storageArea: localStorage,
        }),
      ),
    );
    expect(screen.queryByText("Cached workspace")).toBeNull();
    expect(replace).toHaveBeenCalledWith("/login?client=1");
  });
  it("hides the old account during a cross-tab login without interrupting the reload", async () => {
    mount();
    await screen.findByText("Cached workspace");
    localStorage.setItem("ait.access-token", "new-login");
    act(() => {
      window.dispatchEvent(
        new StorageEvent("storage", {
          key: "ait.access-token",
          storageArea: localStorage,
        }),
      );
      receive({ status: "logged_out" });
    });
    expect(screen.queryByText("Cached workspace")).toBeNull();
    expect(replace).not.toHaveBeenCalled();
  });
  it("ignores shutdown snapshots while navigating away", async () => {
    mount();
    await screen.findByText("Cached workspace");
    act(() => {
      window.dispatchEvent(new Event("pagehide"));
      receive({ status: "logged_out" });
    });
    expect(replace).not.toHaveBeenCalled();
  });
  it("allows retry after network failure without exposing cached content or redirecting", async () => {
    mocks.get.mockRejectedValueOnce(new Error("offline"));
    mount();
    await screen.findByText("Could not connect to the account service. Please try again.");
    expect(screen.queryByText("Cached workspace")).toBeNull();
    expect(replace).not.toHaveBeenCalled();
    fireEvent.click(screen.getByText("Retry"));
    await screen.findByText("Cached workspace");
  });
  it.each(["ios", "android"])("keeps %s app access unchanged", (os) => {
    mocks.os = os;
    mount();
    expect(screen.getByText("Cached workspace")).toBeTruthy();
    expect(mocks.get).not.toHaveBeenCalled();
  });
  it("keeps desktop access unchanged", () => {
    mocks.invoke = vi.fn();
    mount();
    expect(screen.getByText("Cached workspace")).toBeTruthy();
    expect(mocks.get).not.toHaveBeenCalled();
  });
  it("keeps standalone web builds unchanged", () => {
    mocks.supported = false;
    mount();
    expect(screen.getByText("Cached workspace")).toBeTruthy();
    expect(mocks.get).not.toHaveBeenCalled();
  });
});
