import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { page } from "vitest/browser";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { i18n } from "@/i18n/i18next";
import { VibeTrack } from "./vibe";

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
  const { lightTheme } = await import("@/styles/theme");
  return {
    ...actual,
    StyleSheet: {
      ...actual.StyleSheet,
      absoluteFillObject: { position: "absolute", top: 0, right: 0, bottom: 0, left: 0 },
      create: (factory: unknown) => (typeof factory === "function" ? factory(lightTheme) : factory),
    },
    useUnistyles: () => ({ theme: lightTheme, rt: {} }),
  };
});

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
    document.body.style.background = "#f5f5f5";
    document.body.append(host);
    root = createRoot(host);
    const exit = vi.fn().mockRejectedValue(new Error("Native refusal"));
    await act(async () => root!.render(<VibeTrack onExit={exit} />));
    await page.getByTestId("agent-vibe-pill").click();
    await expect.element(page.getByText("退出 Vibe", { exact: true })).toBeVisible();
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
    const button = page.getByRole("button", { name: "退出 Vibe" });
    expect(button.element().getBoundingClientRect().height).toBeGreaterThanOrEqual(
      width < 500 ? 44 : 32,
    );
    await page.screenshot({ path: `../../.tmp/vibe-${width}.png` });
    await button.click();
    await expect.element(page.getByText("Native refusal")).toBeVisible();
    await expect.element(page.getByText("Vibe ON", { exact: true })).toBeVisible();
  },
);
