import React, { act, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { page, userEvent } from "vitest/browser";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { AgentFeature } from "@ait/protocol/agent-types";
import { i18n } from "@/i18n/i18next";
import { AgentFeaturesControl, ComposerFeatureCommandsProvider } from "./features-menu";

vi.mock("@/constants/layout", () => ({ useIsCompactFormFactor: () => false }));
vi.mock("@/hooks/use-agent-autocomplete", () => ({
  useAgentAutocomplete: ({
    userInput,
    setUserInput,
  }: {
    userInput: string;
    setUserInput: (value: string) => void;
  }) => ({
    options: userInput
      ? [
          {
            id: "btw",
            label: "/btw",
            type: "provider_command",
            command: { name: "btw", source: "builtin", argumentHint: "", description: "侧会话" },
          },
          {
            id: "compact",
            label: "/compact",
            type: "provider_command",
            command: {
              name: "compact",
              source: "builtin",
              argumentHint: "",
              description: "压缩上下文",
            },
          },
          {
            id: "goal",
            label: "/goal",
            type: "provider_command",
            command: { name: "goal", source: "builtin", argumentHint: "", description: "管理目标" },
          },
          {
            id: "extension",
            label: "/very-long-extension-command-for-narrow-viewport",
            type: "provider_command",
            command: {
              name: "very-long-extension-command-for-narrow-viewport",
              source: "extension",
              description: "Extension",
            },
          },
        ]
      : [],
    onSelectOption: (option: { id: string }) => setUserInput(`/${option.id} `),
    isLoading: false,
  }),
}));
vi.mock("react-native-unistyles", async (importOriginal) => {
  const original = await importOriginal<typeof import("react-native-unistyles")>();
  const { darkTheme } = await import("@/styles/theme");
  return {
    ...original,
    StyleSheet: {
      ...original.StyleSheet,
      absoluteFillObject: { position: "absolute", top: 0, right: 0, bottom: 0, left: 0 },
      create: (factory: unknown) => (typeof factory === "function" ? factory(darkTheme) : factory),
    },
    useUnistyles: () => ({ theme: darkTheme, rt: {} }),
  };
});

let root: Root | null = null;
beforeEach(async () => {
  vi.stubGlobal("React", React);
  await i18n.changeLanguage("zh-CN");
});
afterEach(async () => {
  await act(async () => root?.unmount());
  root = null;
  document.body.replaceChildren();
});

function Functions({ insert }: { insert: (text: string) => void }) {
  const [features, setFeatures] = useState<AgentFeature[]>([
    { id: "advisorEnabled", type: "toggle", label: "Advisor", value: true, icon: "bot" },
    { id: "fastMode", type: "toggle", label: "快速服务档位", value: false, icon: "zap" },
    {
      id: "modelRole.advisor",
      type: "select",
      label: "Advisor model",
      value: "model",
      options: [],
    },
  ]);
  return (
    <ComposerFeatureCommandsProvider
      value={{ serverId: "host", agentId: "agent", setUserInput: insert }}
    >
      <AgentFeaturesControl
        provider="omp"
        features={features}
        onSetFeature={(id, value) =>
          setFeatures((current) =>
            current.map((feature) =>
              feature.id === id && feature.type === "toggle"
                ? { ...feature, value: Boolean(value) }
                : feature,
            ),
          )
        }
      />
    </ComposerFeatureCommandsProvider>
  );
}

it("shares the mode picker's anchored search surface and supports keyboard command selection", async () => {
  await page.viewport(800, 650);
  const insert = vi.fn();
  const host = document.createElement("div");
  host.style.cssText =
    "position:fixed;left:120px;bottom:80px;width:540px;padding:24px;background:#242321;border:1px solid #393835;border-radius:16px;display:flex;gap:16px;align-items:center;color:#eee;font:14px sans-serif";
  document.body.style.background = "#1c1b19";
  document.body.append(host);
  root = createRoot(host);
  await act(async () =>
    root!.render(
      <>
        <span>高</span>
        <span>完全访问</span>
        <Functions insert={insert} />
      </>,
    ),
  );
  await page.getByTestId("agent-controls-features").click();
  await expect.element(page.getByPlaceholder("搜索功能...")).toBeVisible();
  await expect.element(page.getByText("Advisor model")).not.toBeInTheDocument();
  const trigger = page.getByTestId("agent-controls-features").element().getBoundingClientRect();
  const row = page.getByText("指导模型", { exact: true }).element().getBoundingClientRect();
  expect(row.bottom).toBeLessThan(trigger.top);
  await page.screenshot({ path: "../../.tmp/features-menu-desktop.png" });
  const advisor = page.getByTestId("agent-feature-advisorEnabled");
  const color = () => advisor.element().querySelector("svg")?.getAttribute("stroke");
  const enabledColor = color();
  await page.getByText("指导模型", { exact: true }).click();
  if (enabledColor) await expect.poll(color).not.toBe(enabledColor);
  await page.getByPlaceholder("搜索功能...").fill("compact");
  await expect.element(page.getByText("指导模型", { exact: true })).not.toBeInTheDocument();
  await userEvent.keyboard("{ArrowDown}{Enter}");
  expect(insert).toHaveBeenCalledWith("/compact ");
  await expect.element(page.getByPlaceholder("搜索功能...")).not.toBeInTheDocument();
  await page.viewport(390, 650);
  host.style.left = "16px";
  host.style.width = "358px";
  host.style.boxSizing = "border-box";
  await page.getByTestId("agent-controls-features").click();
  const search = page.getByPlaceholder("搜索功能...");
  await search.fill("very-long-extension");
  const extension = page.getByText("very-long-extension-command-for-narrow-viewport", {
    exact: true,
  });
  await expect.element(extension).toBeVisible();
  const extensionBounds = extension.element().getBoundingClientRect();
  expect(extensionBounds.left).toBeGreaterThanOrEqual(0);
  expect(extensionBounds.right).toBeLessThanOrEqual(390);
  expect(search.element().getBoundingClientRect().right).toBeLessThanOrEqual(390);
  await page.screenshot({ path: "../../.tmp/features-menu-narrow.png" });
});
