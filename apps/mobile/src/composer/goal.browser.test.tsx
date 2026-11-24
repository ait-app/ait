import React, { act, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Text } from "react-native";
import { page } from "vitest/browser";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { i18n } from "@/i18n/i18next";
import { GoalTrack } from "./goal";
import { readGoal, type GoalAction } from "./goal-model";
import { ComposerInputPresentation } from "./input/presentation";
import { CommandArgumentHint } from "./command-argument-hint-view";

vi.mock("@/constants/layout", async (original) => ({
  ...(await original<typeof import("@/constants/layout")>()),
  useIsCompactFormFactor: () => false,
}));
vi.mock("@/hooks/use-agent-commands-query", () => ({
  useAgentCommandsQuery: () => ({
    commands: [
      {
        name: "goal",
        source: "builtin",
        description: "Goal",
        argumentHint: "[<objective>|pause|resume|clear]",
      },
    ],
  }),
}));
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

function Harness({ actions }: { actions: (action: GoalAction, objective?: string) => void }) {
  const [goal, setGoal] = useState(
    readGoal({
      objective: "完成安全初始化与租户授权验证",
      status: "active",
      tokensUsed: 1200,
      actions: { edit: true, pause: true, clear: true },
    }),
  );
  const [text, setText] = useState("/goal ");
  return (
    <>
      <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 12 }}>
        <span
          data-testid="diff-pill"
          style={{
            color: "#60b781",
            border: "1px solid #444",
            borderRadius: 24,
            padding: "6px 12px",
          }}
        >
          +780 −326
        </span>
        {goal ? (
          <GoalTrack
            goal={goal}
            onAction={async (action, objective) => {
              actions(action, objective);
              setGoal(
                action === "clear"
                  ? null
                  : readGoal({
                      ...goal,
                      objective: objective ?? goal.objective,
                      status:
                        action === "pause"
                          ? "paused"
                          : action === "resume"
                            ? "active"
                            : goal.status,
                      actions: {
                        edit: true,
                        pause: action !== "pause",
                        resume: action === "pause",
                        clear: true,
                      },
                    }),
              );
            }}
          />
        ) : null}
      </div>
      <ComposerInputPresentation
        fullscreen={false}
        sendIconSize={16}
        surface={
          <input
            data-testid="draft"
            value={text}
            onChange={(event) => setText(event.target.value)}
            style={{
              color: "#eee",
              background: "transparent",
              border: 0,
              padding: 0,
              fontSize: 16,
            }}
          />
        }
        inputHint={
          <CommandArgumentHint
            text={{ getSnapshot: () => text, subscribe: () => () => {} }}
            serverId="fixture"
            agentId="fixture"
          />
        }
        toolbar={<Text style={{ color: "#aaa", marginTop: 12 }}>GPT · 高 · 完全访问</Text>}
        send={null}
        error={null}
        onExit={() => {}}
        inputWrapperStyle={{
          borderWidth: 1,
          borderColor: "#444",
          borderRadius: 16,
          padding: 16,
          backgroundColor: "#242321",
        }}
        inputPointerEvents="auto"
        attachmentSlot={null}
        showFullscreenButton={false}
        onEnter={() => {}}
        overlay={null}
      />
    </>
  );
}

it("places Goal beside changes, opens its card and keeps goal guidance inside the composer", async () => {
  await page.viewport(900, 720);
  const host = document.createElement("div");
  host.style.cssText =
    "position:fixed;left:80px;bottom:50px;width:740px;color:#eee;font:14px sans-serif";
  document.body.style.background = "#20201e";
  document.body.append(host);
  root = createRoot(host);
  const actions = vi.fn();
  await act(async () => root!.render(<Harness actions={actions} />));
  const pill = page.getByTestId("agent-goal-pill");
  const diff = page.getByTestId("diff-pill").element().getBoundingClientRect();
  const bounds = pill.element().getBoundingClientRect();
  expect(Math.abs(bounds.top - diff.top)).toBeLessThan(12);
  expect(bounds.left).toBeGreaterThan(diff.right);
  const hint = page.getByTestId("composer-command-argument-hint").element();
  const surface = page.getByTestId("composer-input-surface").element();
  expect(surface.contains(hint)).toBe(true);
  expect(hint.getBoundingClientRect().bottom).toBeLessThan(surface.getBoundingClientRect().bottom);
  await pill.click();
  await expect
    .element(page.getByText("完成安全初始化与租户授权验证", { exact: true }))
    .toBeVisible();
  await page.screenshot({ path: "../.tmp/goal-card.png" });
  await page.getByText("编辑目标", { exact: true }).click();
  await page.getByTestId("goal-objective-input").fill("验证租户授权闭环");
  await page.getByText("保存", { exact: true }).click();
  expect(actions).toHaveBeenCalledWith("edit", "验证租户授权闭环");
  await page.getByText("暂停目标", { exact: true }).click();
  await page.getByText("继续目标", { exact: true }).click();
  await page.getByText("清除目标", { exact: true }).click();
  await expect.element(pill).not.toBeInTheDocument();
  await expect.element(page.getByTestId("draft")).toHaveValue("/goal ");
});
