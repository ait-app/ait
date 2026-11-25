import { useRef } from "react";
import { flushSync } from "react-dom";
import { createRoot } from "react-dom/client";
import { Pressable, Text, View } from "react-native";
import { page } from "vitest/browser";
import { expect, it, vi } from "vitest";
import { HostPicker } from "@/components/hosts/host-picker";
import { i18n } from "@/i18n/i18next";
import { useSidebarHostPicker } from "./use-sidebar-host-picker";

const saved = Array.from({ length: 11 }, (_, i) => ({
  serverId: String(i),
  label: `Machine ${i}`,
}));
let available = saved;
let registered = saved;
vi.mock("@/runtime/host-runtime", () => ({
  getHostRuntimeStore: () => ({ getAvailableHosts: () => available, getHosts: () => registered }),
  useHostRuntimeSnapshot: () => null,
  useHostRuntimeConnectionStatus: (id: string) =>
    available.some((host) => host.serverId === id) ? "online" : "offline",
}));
vi.mock("@/hooks/use-is-local-daemon", () => ({ useLocalDaemonServerId: () => null }));

function Picker({ onSettings }: { onSettings: (id: string) => void }) {
  const anchorRef = useRef<View>(null);
  const { hosts, handleOpenChange, openHostSettings } = useSidebarHostPicker(onSettings);
  return (
    <HostPicker
      hosts={hosts ?? []}
      open={hosts !== null}
      onOpenChange={handleOpenChange}
      anchorRef={anchorRef}
      value=""
      onSelect={openHostSettings}
      onOpenHostSettings={openHostSettings}
      includeAddHost
      searchable
      showActiveConnection
      desktopPlacement="top-start"
      hostOptionTestID={(id) => `host-${id}`}
    >
      <Pressable
        ref={anchorRef}
        onPress={() => handleOpenChange(true)}
        accessibilityRole="button"
        accessibilityLabel="Hosts"
      >
        <Text>Hosts</Text>
      </Pressable>
    </HostPicker>
  );
}

it("preserves the host search and its matches until the next open", async () => {
  await i18n.changeLanguage("en");
  const container = document.createElement("div");
  container.style.cssText = "position:fixed;bottom:0;left:0;width:260px";
  document.body.append(container);
  const root = createRoot(container);
  const onSettings = vi.fn();
  const render = () => flushSync(() => root.render(<Picker onSettings={onSettings} />));
  const trigger = page.getByRole("button", { name: "Hosts", exact: true });
  const input = page.getByRole("textbox");
  try {
    render();
    await trigger.click();
    await input.fill("Machine 10");
    const match = page.getByTestId("host-10");
    const top = match.element().getBoundingClientRect().top;
    available = saved.slice(0, 10);
    render();
    expect(input.element()).toBe(document.activeElement);
    expect((input.element() as HTMLInputElement).value).toBe("Machine 10");
    expect(match.element().getBoundingClientRect().top).toBe(top);
    expect(page.getByTestId("host-0").query()).toBeNull();
    await match.click();
    expect(onSettings).toHaveBeenCalledExactlyOnceWith("10");

    await trigger.click();
    expect(input.query()).toBeNull();
    expect(match.query()).toBeNull();
    registered = saved.slice(1);
    await page.getByTestId("host-0").click();
    expect(onSettings).toHaveBeenCalledTimes(1);
  } finally {
    flushSync(() => root.unmount());
    container.remove();
    available = registered = saved;
  }
});
