import { flushSync } from "react-dom";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import { i18n } from "@/i18n/i18next";
import { HostMonitorView } from "./view";

vi.mock("react-native-reanimated", async (original) => ({
  ...(await original<typeof import("react-native-reanimated")>()),
  Keyframe: class {
    duration() {
      return undefined;
    }
  },
}));

it("keeps the close button over the same point when a sampled host disconnects and reconnects", async () => {
  await i18n.changeLanguage("en");
  const container = document.createElement("div");
  container.style.cssText = "position:fixed;bottom:50px;left:0;width:250px";
  document.body.append(container);
  const root = createRoot(container);
  const host = { serverId: "a", label: "Desktop" };
  const sample = {
    at: 1,
    cpuPercent: 25,
    memoryUsedBytes: 512,
    memoryTotalBytes: 1024,
    rxBytesPerSecond: null,
    txBytesPerSecond: null,
  };
  const props = {
    host,
    hosts: [host],
    picked: null,
    onSelect: vi.fn(),
    open: true,
    onOpenChange: vi.fn(),
    supported: true,
    restricted: false,
    failed: false,
    onRetry: vi.fn(),
    snapshot: {
      platform: "linux",
      cpuCount: 4,
      uptimeSeconds: 800,
      current: sample,
      history: [sample],
      disks: [],
    },
  };
  const render = (connected: boolean) =>
    flushSync(() => root.render(<HostMonitorView {...props} connected={connected} />));
  const close = () =>
    document.querySelector<HTMLElement>(
      '[data-testid="host-monitor-details"] [aria-label="Close"]',
    )!;
  try {
    render(true);
    await expect.poll(() => close()?.getBoundingClientRect().top).toBeGreaterThan(0);
    const initial = close().getBoundingClientRect();
    for (const connected of [false, true, false]) {
      render(connected);
      expect(document.querySelector('[data-testid="host-monitor-state"]')?.textContent).toBe(
        connected ? undefined : "Host offline",
      );
      expect(document.querySelector('[data-testid="host-monitor-footer"]')).toBeTruthy();
      // ResizeObserver reports the content height before the next frame's positioning update.
      await new Promise<void>((resolve) =>
        requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
      );
      const bounds = close().getBoundingClientRect();
      expect([bounds.x, bounds.y]).toEqual([initial.x, initial.y]);
    }
  } finally {
    flushSync(() => root.unmount());
    container.remove();
  }
});

it("truncates the host label instead of letting the metrics overlap it in a narrow sidebar", async () => {
  await i18n.changeLanguage("en");
  const container = document.createElement("div");
  container.style.cssText = "position:fixed;bottom:50px;left:0;width:200px";
  document.body.append(container);
  const root = createRoot(container);
  const host = { serverId: "a", label: "owl-home-workstation" };
  const sample = {
    at: 1,
    cpuPercent: 25,
    memoryUsedBytes: 512,
    memoryTotalBytes: 1024,
    rxBytesPerSecond: null,
    txBytesPerSecond: null,
  };
  try {
    flushSync(() =>
      root.render(
        <HostMonitorView
          host={host}
          hosts={[host]}
          picked={null}
          onSelect={vi.fn()}
          open={false}
          onOpenChange={vi.fn()}
          supported
          restricted={false}
          failed={false}
          onRetry={vi.fn()}
          connected
          snapshot={{
            platform: "linux",
            cpuCount: 4,
            uptimeSeconds: 800,
            current: sample,
            history: [sample],
            disks: [],
          }}
        />,
      ),
    );
    const byTestId = (id: string) =>
      document.querySelector<HTMLElement>(`[data-testid="${id}"]`)!.getBoundingClientRect();
    const trigger = byTestId("sidebar-host-monitor");
    const cpu = byTestId("sidebar-host-monitor-cpu");
    const memory = byTestId("sidebar-host-monitor-memory");
    const label = [...container.querySelectorAll<HTMLElement>("div")]
      .filter((node) => node.textContent === host.label && node.children.length === 0)[0]!
      .getBoundingClientRect();
    expect(label.right).toBeLessThanOrEqual(cpu.left);
    expect(cpu.right).toBeLessThanOrEqual(memory.left);
    expect(memory.right).toBeLessThanOrEqual(trigger.right);
  } finally {
    flushSync(() => root.unmount());
    container.remove();
  }
});
