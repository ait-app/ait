import "@/styles/unistyles";
import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { page } from "vitest/browser";
import { UnistylesRuntime } from "react-native-unistyles";
import { REGISTERED_THEMES } from "@/styles/theme";
import { i18n } from "@/i18n/i18next";
import { GoalTrack } from "./goal";
import { readGoal } from "./goal-model";
import { AgentFeaturesControl } from "./agent-controls/features-menu";

vi.mock("@/constants/layout", async (original) => ({
  ...(await original<typeof import("@/constants/layout")>()),
  useIsCompactFormFactor: () => false,
}));
vi.mock("react-native-reanimated", async (original) => ({
  ...(await original<typeof import("react-native-reanimated")>()),
  Keyframe: class {
    duration() {
      return undefined;
    }
  },
}));

Object.defineProperty(globalThis, "IS_REACT_ACT_ENVIRONMENT", { value: true, configurable: true });
let root: Root;
let host: HTMLDivElement;
beforeEach(async () => {
  vi.stubGlobal("React", React);
  UnistylesRuntime.setAdaptiveThemes(false);
  UnistylesRuntime.setTheme("light");
  await i18n.changeLanguage("zh-CN");
  await page.viewport(1000, 720);
  host = document.createElement("div");
  host.style.cssText = "position:fixed;left:40px;bottom:60px;display:flex;gap:24px";
  document.body.append(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  UnistylesRuntime.updateTheme("light", () => REGISTERED_THEMES.light);
  UnistylesRuntime.setTheme("light");
});

function rgb(color: string) {
  const probe = document.createElement("span");
  probe.style.color = color;
  return probe.style.color;
}

it("updates mounted Goal and functions through every registered theme", async () => {
  const goal = readGoal({
    objective: "验证主题切换",
    status: "active",
    actions: { edit: true, clear: true },
  })!;
  await act(async () =>
    root.render(
      <>
        <GoalTrack goal={goal} onAction={async () => {}} />
        <AgentFeaturesControl
          provider="omp"
          features={[{ id: "advisorEnabled", type: "toggle", label: "Advisor", value: true }]}
          onSetFeature={() => {}}
        />
      </>,
    ),
  );
  await page.getByTestId("agent-goal-pill").click();
  const objective = page.getByText("验证主题切换", { exact: true });
  const feature = page.getByText("指导模型", { exact: true });
  const command = page.getByText("/advisor", { exact: true });
  const clear = page.getByText("清除目标", { exact: true });
  for (const name of [
    ...Object.keys(REGISTERED_THEMES),
    "light",
  ] as (keyof typeof REGISTERED_THEMES)[]) {
    await act(async () => UnistylesRuntime.setTheme(name));
    const theme = REGISTERED_THEMES[name];
    await expect
      .poll(() => getComputedStyle(objective.element()).color)
      .toBe(rgb(theme.colors.foreground));
    await expect
      .poll(() => getComputedStyle(clear.element()).color)
      .toBe(rgb(theme.colors.destructive));
    const panel = page.getByTestId("agent-goal-pill-panel");
    await expect
      .poll(() => getComputedStyle(panel.element()).backgroundColor)
      .toBe(rgb(theme.colors.surface1));
    await expect
      .poll(() => getComputedStyle(panel.element()).borderColor)
      .toBe(rgb(theme.colors.borderAccent));
    await expect
      .poll(() => getComputedStyle(page.getByTestId("agent-goal-pill").element()).backgroundColor)
      .toBe(rgb(theme.colors.surface2));
  }
  await act(async () =>
    UnistylesRuntime.updateTheme("light", () => ({
      ...REGISTERED_THEMES.light,
      colors: { ...REGISTERED_THEMES.light.colors, foreground: "#183a5b", destructive: "#a23467" },
      fontSize: { ...REGISTERED_THEMES.light.fontSize, base: 18 },
    })),
  );
  await expect.poll(() => getComputedStyle(objective.element()).color).toBe(rgb("#183a5b"));
  await expect.poll(() => getComputedStyle(objective.element()).fontSize).toBe("18px");
  await expect.poll(() => getComputedStyle(clear.element()).color).toBe(rgb("#a23467"));
  await page.getByTestId("agent-goal-pill-panel-backdrop").click();
  await page.getByTestId("agent-controls-features").click();
  for (const name of [
    ...Object.keys(REGISTERED_THEMES),
    "light",
  ] as (keyof typeof REGISTERED_THEMES)[]) {
    await act(async () => {
      UnistylesRuntime.updateTheme("light", () => REGISTERED_THEMES.light);
      UnistylesRuntime.setTheme(name);
    });
    const theme = REGISTERED_THEMES[name];
    await expect
      .poll(() => getComputedStyle(feature.element()).color)
      .toBe(rgb(theme.colors.foreground));
    await expect
      .poll(() => getComputedStyle(command.element()).color)
      .toBe(rgb(theme.colors.foregroundMuted));
  }
});
