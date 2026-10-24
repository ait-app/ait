import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { page } from "vitest/browser";
import { VoiceTranscript } from "./voice-transcript";
import { VoiceTimelineEntry } from "./voice-timeline-entry";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => (key === "voiceTimeline.started" ? "语音对话已开始" : "语音对话已结束"),
  }),
}));

vi.mock("react-native-unistyles", async () => {
  const { darkTheme } = await import("@/styles/theme");
  return {
    StyleSheet: { create: (factory: (theme: typeof darkTheme) => unknown) => factory(darkTheme) },
  };
});

Object.defineProperty(globalThis, "IS_REACT_ACT_ENVIRONMENT", { value: true, configurable: true });
let root: Root | null = null;
afterEach(async () => {
  await act(async () => root?.unmount());
  root = null;
  document.body.replaceChildren();
});

it("renders canonical speech updates and call boundaries without editable text messages", async () => {
  await page.viewport(420, 800);
  const host = document.createElement("div");
  host.style.cssText = "width:388px;padding:16px;box-sizing:border-box;background:#18181b";
  document.body.append(host);
  root = createRoot(host);
  const render = (text: string) => (
    <>
      <VoiceTimelineEntry
        item={{ type: "voice", kind: "session", id: "start", sessionId: "call", status: "started" }}
      />
      <VoiceTimelineEntry
        item={{
          type: "voice",
          kind: "transcript",
          id: "user",
          sessionId: "call",
          role: "user",
          status: "completed",
          text: "现在能听见我吗？",
        }}
      />
      <VoiceTimelineEntry
        item={{
          type: "voice",
          kind: "transcript",
          id: "assistant",
          sessionId: "call",
          role: "assistant",
          status: "completed",
          text,
        }}
      />
      <VoiceTimelineEntry
        item={{ type: "voice", kind: "session", id: "end", sessionId: "call", status: "ended" }}
      />
    </>
  );
  await act(async () => root!.render(render("我能")));
  await act(async () => root!.render(render("我能听见你，最后一句也已保留。")));
  expect(host.querySelectorAll('[data-testid="voice-session-marker"]')).toHaveLength(2);
  expect(host.querySelectorAll('[data-testid="voice-transcript-assistant"]')).toHaveLength(1);
  expect(host.textContent).toBe(
    "语音对话已开始现在能听见我吗？我能听见你，最后一句也已保留。语音对话已结束",
  );
  expect(host.querySelectorAll("button, pre, code")).toHaveLength(0);
  expect(host.scrollWidth).toBeLessThanOrEqual(388);
  await page.screenshot();
});

it.each([800, 360])(
  "renders speech as an ordered conversation at %i px without XML or duplicate input",
  async (width) => {
    await page.viewport(width + 32, 800);
    const host = document.createElement("div");
    host.style.width = `${width}px`;
    host.style.background = "#18181b";
    host.style.padding = "16px";
    host.style.boxSizing = "border-box";
    document.body.append(host);
    root = createRoot(host);
    const input =
      "我就想要试试看你能不能回我一些东西以及回我的东西能不能进到这个我们交谈的这个 thread 里面来";
    const entries = [
      { role: "user" as const, text: "试试看，现在这样子可不可以" },
      {
        role: "assistant" as const,
        text: "嗯，我这边能听到你说话。你是想测语音模式，还是要试某个功能？",
      },
      { role: "user" as const, text: input },
    ];
    await act(async () => root!.render(<VoiceTranscript entries={entries} />));
    const transcript = host.querySelector('[data-testid="voice-transcript"]')!;
    const users = [...host.querySelectorAll<HTMLElement>('[data-testid="voice-transcript-user"]')];
    const assistant = host.querySelector<HTMLElement>(
      '[data-testid="voice-transcript-assistant"]',
    )!;
    expect(transcript.textContent).toBe(entries.map((entry) => entry.text).join(""));
    expect(users).toHaveLength(2);
    expect(getComputedStyle(users[0]).alignSelf).toBe("flex-end");
    expect(getComputedStyle(assistant).alignSelf).toBe("flex-start");
    expect(host.querySelectorAll("pre, code")).toHaveLength(0);
    expect(transcript.getBoundingClientRect().width).toBeLessThanOrEqual(width);
    expect(host.scrollWidth).toBeLessThanOrEqual(width);
    expect(users[0].getBoundingClientRect().right).toBeLessThan(window.innerWidth);
    await page.getByTestId("voice-transcript").screenshot();
  },
);
