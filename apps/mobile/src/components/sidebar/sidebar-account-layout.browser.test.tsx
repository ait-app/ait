import { flushSync } from "react-dom";
import { createRoot } from "react-dom/client";
import { create } from "zustand";
import { expect, it, vi } from "vitest";
import { i18n } from "@/i18n/i18next";
import { SidebarAccountMenu } from "./sidebar-account-menu";

const first = { serverId: "first", label: "Desktop" };
const second = { serverId: "second", label: "Laptop" };
const useHostsFixture = create(() => ({ hosts: [first, second], online: true }));
vi.mock("@/runtime/host-runtime", () => ({
  useHosts: () => useHostsFixture((state) => state.hosts),
  getHostRuntimeStore: () => ({ getHosts: () => useHostsFixture.getState().hosts }),
  useHostRuntimeIsConnected: (id: string) =>
    useHostsFixture((state) => id !== second.serverId || state.online),
}));
vi.mock("@/stores/session-store", () => ({ useSessionStore: () => "0.2.25" }));
vi.mock("@/runtime/account-state", () => ({
  useAccountState: () => "owner",
  accountCommand: vi.fn(),
}));
vi.mock("./account-expiry", () => ({ useAccountExpiry: () => ({ kind: "never" }) }));
vi.mock("@/contexts/toast-context", () => ({ useToast: () => ({}) }));
vi.mock("@/hooks/use-shortcut-keys", () => ({ useShortcutKeys: () => null }));
vi.mock("@/keyboard/availability", () => ({ useKeyboardShortcutsAvailable: () => true }));
vi.mock("@/utils/app-version", () => ({ resolveAppVersion: () => "0.2.25" }));
vi.mock("@/changelog", () => ({ openChangelog: () => {} }));

vi.mock("react-native-reanimated", async (original) => ({
  ...(await original<typeof import("react-native-reanimated")>()),
  Keyframe: class {
    duration() {
      return undefined;
    }
  },
}));

function element(id: string) {
  const node = document.querySelector<HTMLElement>(`[data-testid="${id}"]`);
  if (!node) throw new Error(`Missing ${id}`);
  return node;
}

it("keeps the open account menu in place through disconnects and host list refreshes", async () => {
  await i18n.changeLanguage("en");
  const container = document.createElement("div");
  container.style.cssText = "position:fixed;bottom:0;left:0;width:280px";
  document.body.append(container);
  const root = createRoot(container);
  const menu = () => element("sidebar-account-menu");
  const laptop = () => element("sidebar-help-host-version-second");
  try {
    flushSync(() =>
      root.render(<SidebarAccountMenu onOpenSettings={vi.fn()} onImportSession={vi.fn()} />),
    );
    element("sidebar-account").click();
    await expect.poll(() => menu().getBoundingClientRect().top).toBeGreaterThan(0);
    const top = menu().getBoundingClientRect().top;
    const height = menu().getBoundingClientRect().height;
    for (const connected of [false, true, false]) {
      flushSync(() => useHostsFixture.setState({ online: connected }));
      expect(laptop().textContent).toContain(connected ? "v0.2.25" : "Offline");
      expect(menu().getBoundingClientRect().height).toBe(height);
      expect(menu().getBoundingClientRect().top).toBe(top);
    }
    flushSync(() => useHostsFixture.setState({ hosts: [first] }));
    expect(menu().getBoundingClientRect().height).toBe(height);
    expect(menu().getBoundingClientRect().top).toBe(top);
    expect(laptop().textContent).toContain("Offline");

    flushSync(() =>
      useHostsFixture.setState({ hosts: [first, { serverId: "new", label: "New host" }] }),
    );
    expect(menu().querySelector('[data-testid="sidebar-help-host-version-new"]')).toBeNull();
    element("sidebar-account-menu-backdrop").click();
    await expect
      .poll(() => document.querySelector('[data-testid="sidebar-account-menu"]'))
      .toBeNull();
    element("sidebar-account").click();
    await expect.poll(() => menu().getBoundingClientRect().top).toBeGreaterThan(0);
    expect(menu().querySelector('[data-testid="sidebar-help-host-version-second"]')).toBeNull();
    expect(element("sidebar-help-host-version-new").textContent).toContain("New host");
  } finally {
    flushSync(() => root.unmount());
    container.remove();
  }
});
