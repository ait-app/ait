import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { page } from "vitest/browser";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { i18n } from "@/i18n/i18next";
import { GoalTrack } from "./goal";
import { readGoal } from "./goal-model";

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

const objective =
  "完成示例项目当前规格定义的产品实现与验收。按指定顺序核实并修复授权摘要、初始化与恢复、真实隔离，再按依赖完成控制面、编译器、节点执行和恢复、业务控制台、系统适配和虚拟网络验收。使用任务清单维护执行状态，保存真实验证证据，只创建本地功能分支提交，不推送、不部署生产或影响共享网络。仅在全部要求具备实现和验证证据时完成目标。";

it.each([393, 320])("keeps a long Goal readable and its actions visible at %dpx", async (width) => {
  await page.viewport(width, 852);
  const host = document.createElement("div");
  host.style.cssText = "position:fixed;left:16px;bottom:32px;right:16px;font:14px sans-serif";
  document.body.style.background = "#f5f5f5";
  document.body.append(host);
  root = createRoot(host);
  const actions = vi.fn();
  const goal = readGoal({
    objective,
    status: "active",
    tokensUsed: 13933915,
    actions: { edit: true, pause: true, clear: true },
  })!;
  await act(async () => root!.render(<GoalTrack goal={goal} onAction={actions} />));
  await page.getByTestId("agent-goal-pill").click();
  await expect.element(page.getByText("已用 Token：13,933,915", { exact: true })).toBeVisible();
  await expect
    .poll(() => {
      const rect = page.getByTestId("agent-goal-card").element().getBoundingClientRect();
      return rect.width > 200 && rect.left >= 0 && rect.bottom <= 852;
    })
    .toBe(true);
  const card = page.getByTestId("agent-goal-card").element();
  const metadata = page.getByTestId("goal-metadata").element().getBoundingClientRect();
  const scroll = page.getByTestId("goal-objective-scroll").element() as HTMLElement;
  const text = page.getByTestId("goal-objective").element();
  const footer = page.getByTestId("goal-actions").element().getBoundingClientRect();
  const bounds = card.getBoundingClientRect();
  expect(bounds.left).toBeGreaterThanOrEqual(0);
  expect(bounds.right).toBeLessThanOrEqual(width);
  expect(metadata.bottom).toBeLessThanOrEqual(scroll.getBoundingClientRect().top);
  expect(scroll.getBoundingClientRect().bottom).toBeLessThan(footer.top);
  expect(getComputedStyle(text).fontWeight).toBe("400");
  expect(Number.parseFloat(getComputedStyle(text).lineHeight)).toBeGreaterThan(
    Number.parseFloat(getComputedStyle(text).fontSize) * 1.4,
  );
  const buttons = ["编辑目标", "暂停目标", "清除目标"].map((name) =>
    page.getByRole("button", { name, exact: true }).element().getBoundingClientRect(),
  );
  expect(new Set(buttons.map((rect) => rect.top)).size).toBe(1);
  for (const button of buttons) {
    expect(button.height).toBeGreaterThanOrEqual(44);
    expect(button.right).toBeLessThanOrEqual(bounds.right);
    expect(button.bottom).toBeLessThanOrEqual(852);
  }
  await page.screenshot({ path: `../.tmp/goal-mobile-${width}.png` });
  const longObjective = [objective, objective, objective].join("\n\n");
  await act(async () =>
    root!.render(<GoalTrack goal={{ ...goal, objective: longObjective }} onAction={actions} />),
  );
  await expect.poll(() => scroll.scrollHeight).toBeGreaterThan(scroll.clientHeight);
  await expect
    .poll(() => page.getByTestId("goal-actions").element().getBoundingClientRect().bottom)
    .toBeLessThanOrEqual(852);
  const longFooter = page.getByTestId("goal-actions").element().getBoundingClientRect();
  expect(longFooter.bottom).toBeLessThanOrEqual(852);
  scroll.scrollTop = scroll.scrollHeight;
  expect(scroll.scrollTop).toBeGreaterThan(0);
  expect(page.getByTestId("goal-actions").element().getBoundingClientRect().top).toBe(
    longFooter.top,
  );
  await page.getByRole("button", { name: "编辑目标", exact: true }).click();
  await expect.element(page.getByTestId("goal-objective-input")).toHaveValue(longObjective);
  const input = page.getByTestId("goal-objective-input").element() as HTMLTextAreaElement;
  expect(input.getBoundingClientRect().height).toBeLessThanOrEqual(240);
  expect(input.scrollHeight).toBeGreaterThan(input.clientHeight);
  await expect
    .poll(() => page.getByTestId("goal-actions").element().getBoundingClientRect().bottom)
    .toBeLessThanOrEqual(852);
  await page.screenshot({ path: `../.tmp/goal-mobile-edit-long-${width}.png` });
  await page.getByTestId("goal-objective-input").fill("验证产品完整交付");
  await page.screenshot({ path: `../.tmp/goal-mobile-edit-${width}.png` });
  await page.getByRole("button", { name: "保存", exact: true }).click();
  expect(actions).toHaveBeenCalledWith("edit", "验证产品完整交付");
});
