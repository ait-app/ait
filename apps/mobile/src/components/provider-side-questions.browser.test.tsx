import React, { act, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { page, userEvent } from "vitest/browser";
import type { SideQuestionsState, SideTopic } from "@ait/protocol/messages";
import { i18n } from "@/i18n/i18next";
import { BtwConversation } from "./btw-conversation";
import { BtwWindow } from "./btw-window";
import { BtwFloatingSurface } from "./btw-floating-surface";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { UnistylesRuntime, useUnistyles } from "react-native-unistyles";
import {
  lightTheme,
  darkTheme,
  darkClaudeTheme,
  REGISTERED_THEMES,
  type Theme,
} from "@/styles/theme";
import { Button } from "@/components/ui/button";
import { AppearanceStyleBoundary } from "@/components/appearance-style-boundary";
import { applyAppearance } from "@/appearance/apply";

const mocks = vi.hoisted(() => ({
  ask: vi.fn(),
  cancel: vi.fn(),
  transfer: vi.fn(),
  state: null as SideQuestionsState | null,
}));

const hasThemeRuntime = typeof UnistylesRuntime.setAdaptiveThemes === "function";

beforeEach(async () => {
  if (!hasThemeRuntime) return;
  await act(async () => {
    UnistylesRuntime.setAdaptiveThemes(false);
    UnistylesRuntime.setTheme("light");
  });
});

Object.defineProperty(globalThis, "IS_REACT_ACT_ENVIRONMENT", { value: true, configurable: true });
let root: Root | null = null;
let queryClient: QueryClient | null = null;

function Harness({ behind }: { behind: () => void }) {
  const { theme } = useUnistyles();
  const [open, setOpen] = useState(true);
  const [expanded, setExpanded] = useState(false);
  const [topic, setTopic] = useState<SideTopic | null>(mocks.state!.records[0] ?? null);
  const [draft, setDraft] = useState("");
  const [historyOpen, setHistoryOpen] = useState(false);
  const submit = () => {
    if (!draft.trim()) return;
    mocks.ask({ agentId: "agent", question: draft, recordId: topic?.id });
    setDraft("");
  };
  return (
    <AppearanceStyleBoundary>
      <div
        style={{
          minHeight: "100vh",
          background: theme.colors.surface0,
          fontFamily: theme.fontFamily.ui,
          color: theme.colors.foreground,
          fontSize: theme.fontSize.content,
        }}
      >
        <div
          style={{
            padding: `${theme.spacing[6]}px ${theme.spacing[6]}px`,
            borderBottom: `${theme.borderWidth[1]}px solid ${theme.colors.border}`,
            fontWeight: theme.fontWeight.semibold,
          }}
        >
          Review provider lifecycle
        </div>
        <main
          style={{
            padding: `${theme.spacing[12]}px ${theme.spacing[6]}px`,
            maxWidth: 640,
            margin: "0 auto",
          }}
        >
          {expanded && topic ? (
            <>
              <h2
                style={{
                  fontSize: theme.fontSize.lg,
                  fontWeight: theme.fontWeight.medium,
                  marginBottom: theme.spacing[8],
                }}
              >
                Side conversation
              </h2>
              <BtwConversation topic={topic} />
              <textarea
                aria-label="Continue full conversation"
                placeholder="Continue this conversation…"
                style={{
                  width: "100%",
                  boxSizing: "border-box",
                  minHeight: 80,
                  marginTop: theme.spacing[8],
                  padding: theme.spacing[4],
                  border: `${theme.borderWidth[1]}px solid ${theme.colors.border}`,
                  borderRadius: theme.borderRadius["2xl"],
                  background: theme.colors.surface1,
                  color: theme.colors.foreground,
                  fontFamily: theme.fontFamily.ui,
                  fontSize: theme.fontSize.content,
                }}
              />
            </>
          ) : (
            <>
              <p style={{ fontSize: theme.fontSize.content, lineHeight: 1.4 }}>
                The main conversation continues working while you ask a side question.
              </p>
              <Button
                variant="outline"
                size="sm"
                onPress={behind}
                style={{ marginTop: theme.spacing[6] }}
              >
                Continue main conversation
              </Button>
            </>
          )}
        </main>
        <BtwFloatingSurface visible={open} onClose={() => setOpen(false)} onSubmit={submit}>
          <BtwWindow
            sourceTitle="Review provider lifecycle"
            state={mocks.state!}
            topic={topic}
            draft={draft}
            error={null}
            loading={false}
            pending={false}
            disabled={false}
            historyOpen={historyOpen}
            canTransfer={Boolean(topic)}
            onDraftChange={setDraft}
            onSubmit={submit}
            onCancel={() => mocks.cancel()}
            onClose={() => setOpen(false)}
            onTransfer={() => {
              mocks.transfer(topic!.id);
              setExpanded(true);
              setOpen(false);
            }}
            onNew={() => setTopic(null)}
            onHistoryToggle={() => setHistoryOpen((current) => !current)}
            onSelectTopic={(id) =>
              setTopic(mocks.state!.records.find((record) => record.id === id) ?? null)
            }
            onRetry={() => {}}
          />
        </BtwFloatingSurface>
      </div>
    </AppearanceStyleBoundary>
  );
}

async function mount(width: number, height: number) {
  await page.viewport(width, height);
  await i18n.changeLanguage("en");
  mocks.state = {
    records: [
      {
        id: "topic",
        question: "Can I keep asking here while the main task runs?",
        answer:
          "Yes. This conversation has its own history and input. Your main task continues independently.\n\nYou can ask another question here, or open this conversation in a full tab when you want to take it further.",
        status: "complete",
        createdAt: 1,
        updatedAt: 2,
        leafId: null,
        strategy: "isolated",
        truncated: false,
      },
    ],
    omitted: 0,
    available: true,
  };
  mocks.ask.mockResolvedValue({ receipt: "accepted", recordId: "topic" });
  mocks.cancel.mockResolvedValue(true);
  mocks.transfer.mockResolvedValue(undefined);
  const host = document.createElement("div");
  document.body.style.margin = "0";
  document.body.append(host);
  root = createRoot(host);
  const behind = vi.fn();
  queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  await act(async () =>
    root!.render(
      <QueryClientProvider client={queryClient!}>
        <Harness behind={behind} />
      </QueryClientProvider>,
    ),
  );
  await expect
    .element(page.getByText("Can I keep asking here while the main task runs?"))
    .toBeVisible();
  return { behind };
}

afterEach(async () => {
  await act(async () => {
    root?.unmount();
    if (hasThemeRuntime) {
      for (const name of Object.keys(REGISTERED_THEMES) as (keyof typeof REGISTERED_THEMES)[]) {
        UnistylesRuntime.updateTheme(name, () => REGISTERED_THEMES[name]);
      }
      UnistylesRuntime.setTheme("light");
    }
  });
  root = null;
  queryClient?.clear();
  document.body.replaceChildren();
  vi.clearAllMocks();
});

it("leaves the main conversation interactive and follows up in a desktop floating window", async () => {
  const { behind } = await mount(1440, 900);
  const window = document.querySelector('[data-testid="btw-window"]')!.getBoundingClientRect();
  expect(window.width).toBe(448);
  expect(window.right).toBeLessThanOrEqual(1440);
  expect(getComputedStyle(document.querySelector("textarea")!).outlineStyle).toBe("none");
  await page.screenshot({ path: "../../.vitest-screenshots/btw-desktop.png" });
  await page.getByRole("button", { name: "Continue main conversation" }).click();
  expect(behind).toHaveBeenCalledTimes(1);
  await page.getByTestId("side-question-input").fill("What changes when I expand this window?");
  await page.getByTestId("side-question-input").click();
  await userEvent.keyboard("{Enter}");
  expect(mocks.ask).toHaveBeenCalledWith({
    agentId: "agent",
    question: "What changes when I expand this window?",
    recordId: "topic",
  });
  expect(mocks.ask).toHaveBeenCalledTimes(1);
});

it("fits the narrow viewport and expands without resending the side conversation", async () => {
  await mount(390, 844);
  const window = document.querySelector('[data-testid="btw-window"]')!.getBoundingClientRect();
  expect(window.left).toBeGreaterThanOrEqual(8);
  expect(window.right).toBeLessThanOrEqual(382);
  expect(window.bottom).toBeLessThanOrEqual(836);
  await expect
    .element(page.getByRole("button", { name: "Open as a full conversation" }))
    .toBeVisible();
  await page.screenshot({ path: "../../.vitest-screenshots/btw-mobile.png" });
  await page.getByRole("button", { name: "Open as a full conversation" }).click();
  expect(mocks.transfer).toHaveBeenCalledWith("topic");
  expect(mocks.ask).not.toHaveBeenCalled();
  await expect.element(page.getByTestId("btw-window")).not.toBeInTheDocument();
  await expect
    .element(page.getByRole("textbox", { name: "Continue full conversation" }))
    .toBeVisible();
  await page.screenshot({ path: "../../.vitest-screenshots/btw-expanded-mobile.png" });
});

function cssColor(color: string): string {
  const element = document.createElement("span");
  element.style.color = color;
  return element.style.color;
}

function palette(theme: Theme) {
  return {
    panel: cssColor(theme.colors.surface1),
    bubble: cssColor(theme.colors.surface3),
    foreground: cssColor(theme.colors.foreground),
    inputSurface: cssColor(theme.colors.surface0),
    placeholder: cssColor(theme.colors.foregroundMuted),
    sourceIcon: cssColor(theme.colors.foregroundMuted),
    send: cssColor(theme.colors.accent),
    focus: cssColor(theme.colors.accent),
  };
}

function colors() {
  const element = (id: string) => document.querySelector(`[data-testid="${id}"]`)!;
  const input = element("side-question-input");
  return {
    panel: getComputedStyle(element("btw-window")).backgroundColor,
    bubble: getComputedStyle(element("side-question-user-bubble")).backgroundColor,
    foreground: getComputedStyle(element("side-question-user-text")).color,
    inputSurface: getComputedStyle(element("side-question-compose")).backgroundColor,
    placeholder: getComputedStyle(input, "::placeholder").color,
    sourceIcon: getComputedStyle(element("side-question-source").querySelector("svg")!).stroke,
    send: getComputedStyle(element("side-question-ask")).backgroundColor,
    focus: getComputedStyle(element("side-question-compose")).outlineColor,
  };
}

async function switchTheme(name: "light" | "dark" | "darkClaude") {
  await act(async () => {
    UnistylesRuntime.setTheme(name);
    await new Promise(requestAnimationFrame);
  });
}

it.skipIf(!hasThemeRuntime)(
  "updates the mounted window, inputs, prose and shared controls across app palettes",
  async () => {
    await mount(1440, 900);
    await page.getByTestId("side-question-input").fill("How does the branch keep context?");
    await expect.poll(colors).toEqual(palette(lightTheme));
    await page.screenshot({ path: "../../.vitest-screenshots/btw-theme-light-desktop.png" });
    await switchTheme("dark");
    await expect.poll(colors).toEqual(palette(darkTheme));
    await page.screenshot({ path: "../../.vitest-screenshots/btw-theme-dark-desktop.png" });
    await switchTheme("darkClaude");
    await expect.poll(colors).toEqual(palette(darkClaudeTheme));
    expect(darkClaudeTheme.colors.accent).not.toBe(lightTheme.colors.accent);
    await switchTheme("light");
    await expect.poll(colors).toEqual(palette(lightTheme));
    expect(document.querySelector<HTMLTextAreaElement>("textarea")!.value).toBe(
      "How does the branch keep context?",
    );
    await page.getByRole("button", { name: "Continue main conversation" }).click();
    await page.getByTestId("side-question-compose").hover();
    const compose = document.querySelector('[data-testid="side-question-compose"]')!;
    await expect
      .poll(() => getComputedStyle(compose).borderColor)
      .toBe(cssColor(lightTheme.colors.borderAccent));
    expect(getComputedStyle(compose).outlineWidth).toBe("0px");
    await page.getByTestId("side-question-input").click();
    await expect.poll(() => getComputedStyle(compose).outlineWidth).toBe("2px");
  },
);

it.skipIf(!hasThemeRuntime)(
  "uses live app typography for the question, answer, input and expanded preview",
  async () => {
    await mount(390, 844);
    await act(async () => {
      applyAppearance({
        uiFontFamily: "monospace",
        monoFontFamily: "",
        uiBaseFontSize: 18,
        contentFontSize: 19,
        codeFontSize: lightTheme.fontSize.code,
        contentMaxWidth: lightTheme.contentMaxWidth,
        syntaxTheme: "one",
      });
    });
    const font = (element: Element) => {
      const style = getComputedStyle(element);
      return { size: style.fontSize, family: style.fontFamily };
    };
    await expect
      .poll(() => font(page.getByText("BTW", { exact: true }).element()))
      .toEqual({ size: "18px", family: "monospace" });
    await expect
      .poll(() => font(document.querySelector('[data-testid="side-question-user-text"]')!))
      .toEqual({ size: "19px", family: "monospace" });
    await expect
      .poll(() => font(document.querySelector('[data-testid="side-question-input"]')!))
      .toEqual({ size: "19px", family: "monospace" });
    await expect
      .poll(() => font(document.querySelector('[data-testid="side-question-transcript"] p')!))
      .toEqual({ size: "19px", family: "monospace" });
    await page.getByTestId("side-question-expand").click();
    await expect
      .poll(() => font(document.querySelector('[data-testid="side-question-user-text"]')!))
      .toEqual({ size: "19px", family: "monospace" });
  },
);
