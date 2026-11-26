import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { page } from "vitest/browser";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { i18n } from "@/i18n/i18next";
import { VibeTrack } from "./vibe";
import { readVibe } from "./vibe-model";

// Exercise the real shared card at phone widths. Native sheet gestures/keyboard need a device.
vi.mock("@/constants/layout", async (original) => ({
  ...(await original<typeof import("@/constants/layout")>()),
  useIsCompactFormFactor: () => window.innerWidth < 500,
}));
// Native sheets cannot mount in Chromium; keep their compact card geometry in the popover.
vi.mock("@/components/ui/menu/menu-context", async (original) => {
  const actual = await original<typeof import("@/components/ui/menu/menu-context")>();
  return {
    ...actual,
    useMenuState: (options: Parameters<typeof actual.useMenuState>[0]) =>
      actual.useMenuState({ ...options, compactMode: "popover" }),
  };
});
// Verify placement and interactions deterministically, without the native entering animation.
vi.mock("react-native-reanimated", async (original) => ({
  ...(await original<typeof import("react-native-reanimated")>()),
  Keyframe: class {
    duration() {
      return undefined;
    }
  },
}));
vi.mock("react-native-unistyles", async (original) => {
  const actual = await original<typeof import("react-native-unistyles")>();
  const { darkTheme } = await import("@/styles/theme");
  return {
    ...actual,
    StyleSheet: {
      ...actual.StyleSheet,
      absoluteFillObject: { position: "absolute", top: 0, right: 0, bottom: 0, left: 0 },
      create: (factory: unknown) => (typeof factory === "function" ? factory(darkTheme) : factory),
    },
    useUnistyles: () => ({ theme: darkTheme, rt: {} }),
  };
});

const vibe = readVibe({
  enabled: true,
  workers: [
    {
      id: "layout",
      description: "修复 Vibe 卡片在窄屏下的任务布局",
      model: '["google", "gemini-4-carbon"]',
      status: "running",
    },
    {
      id: "tests",
      description: "验证退出失败后的重试",
      model: '["openai", "gpt-5.4"]',
      status: "failed",
    },
    {
      id: "review",
      description: "审查状态与真实任务是否一致",
      model: '["anthropic", "claude-sonnet-4-6"]',
      status: "completed",
    },
  ],
})!;
let root: Root | null = null;
Object.defineProperty(globalThis, "IS_REACT_ACT_ENVIRONMENT", { value: true, configurable: true });
beforeEach(async () => {
  vi.stubGlobal("React", React);
  await i18n.changeLanguage("zh-CN");
});
afterEach(async () => {
  await act(async () => root?.unmount());
  root = null;
  document.body.replaceChildren();
});

it.each([393, 320, 900])(
  "keeps Vibe actions visible at %dpx and retains failed state",
  async (width) => {
    await page.viewport(width, 852);
    const host = document.createElement("div");
    host.style.cssText = "position:fixed;left:16px;bottom:32px;right:16px;font:14px sans-serif";
    document.body.style.background = "#1d1c1a";
    document.body.append(host);
    root = createRoot(host);
    const exit = vi.fn().mockRejectedValue(new Error("Native refusal"));
    await act(async () => root!.render(<VibeTrack vibe={vibe} onExit={exit} />));
    await page.getByTestId("agent-vibe-pill").click();
    await expect.element(page.getByText("停止任务并退出 Vibe", { exact: true })).toBeVisible();
    await expect
      .poll(() => {
        const bounds = page.getByTestId("agent-vibe-card").element().getBoundingClientRect();
        return bounds.width > 200 && bounds.left >= 0 && bounds.right <= width;
      })
      .toBe(true);
    const card = page.getByTestId("agent-vibe-card").element();
    for (const node of [card, ...card.querySelectorAll('[role="button"]')]) {
      const bounds = node.getBoundingClientRect();
      expect(bounds.left).toBeGreaterThanOrEqual(0);
      expect(bounds.right).toBeLessThanOrEqual(width);
      expect(bounds.bottom).toBeLessThanOrEqual(852);
    }
    const button = page.getByRole("button", { name: "停止任务并退出 Vibe" });
    expect(button.element().getBoundingClientRect().height).toBeGreaterThanOrEqual(
      width < 500 ? 44 : 32,
    );
    await expect.element(page.getByText("修复 Vibe 卡片在窄屏下的任务布局")).toBeVisible();
    await expect.element(page.getByText("google / gemini-4-carbon")).toBeVisible();
    await expect.element(page.getByText("运行中", { exact: true })).toBeVisible();
    await expect.element(page.getByText("失败", { exact: true })).toBeVisible();
    await expect.element(page.getByText("已完成", { exact: true })).toBeVisible();
    await page.screenshot({ path: `../../.tmp/vibe-${width}.png` });
    await button.click();
    await expect.element(page.getByText("Native refusal")).toBeVisible();
    await expect.element(page.getByTestId("agent-vibe-pill")).toBeVisible();
  },
);

it("keeps empty and disconnected cards compact without implying live work", async () => {
  await page.viewport(320, 700);
  const host = document.createElement("div");
  host.style.cssText = "position:fixed;left:16px;bottom:32px;right:16px";
  document.body.style.background = "#1d1c1a";
  document.body.append(host);
  root = createRoot(host);
  const exit = vi.fn();
  await act(async () => root!.render(<VibeTrack vibe={{ workers: [] }} onExit={exit} />));
  await page.getByTestId("agent-vibe-pill").click();
  await expect.element(page.getByText("尚未分配任务")).toBeVisible();
  expect(page.getByTestId("agent-vibe-card").element().getBoundingClientRect().height).toBeLessThan(
    120,
  );
  await page.screenshot({ path: "../../.tmp/vibe-empty-320.png" });
  await act(async () => root!.render(<VibeTrack vibe={vibe} disabled onExit={exit} />));
  await expect.element(page.getByText("已断开 · 显示上次任务记录")).toBeVisible();
  await expect.element(page.getByText("运行中", { exact: true })).not.toBeInTheDocument();
  await expect.element(page.getByRole("button", { name: "停止任务并退出 Vibe" })).toBeDisabled();
  await page.screenshot({ path: "../../.tmp/vibe-disconnected-320.png" });
});

it("keeps a full worker registry scrollable and its exit action reachable", async () => {
  await page.viewport(900, 700);
  const host = document.createElement("div");
  host.style.cssText = "position:fixed;left:16px;bottom:32px;right:16px";
  document.body.append(host);
  root = createRoot(host);
  const openWorker = vi.fn();
  const exit = vi.fn().mockResolvedValue(undefined);
  const workers = Array.from({ length: 32 }, (_, index) => ({
    ...vibe.workers![0]!,
    id: `worker-${index}`,
    description: `检查任务 ${index + 1}`,
  }));
  await act(async () =>
    root!.render(
      <VibeTrack
        vibe={{ workers }}
        onExit={exit}
        workerLink={(id) => ({ onPress: () => openWorker(id), closeOnSelect: false })}
      />,
    ),
  );
  await page.getByTestId("agent-vibe-pill").click();
  await page.getByRole("button", { name: "检查任务 32", exact: true }).click();
  expect(openWorker).toHaveBeenCalledWith("worker-31");
  await page.getByRole("button", { name: "停止任务并退出 Vibe" }).click();
  expect(exit).toHaveBeenCalledOnce();
  const bounds = page.getByTestId("agent-vibe-pill-panel").element().getBoundingClientRect();
  expect(bounds.top).toBeGreaterThanOrEqual(0);
  expect(bounds.bottom).toBeLessThanOrEqual(700);
});
