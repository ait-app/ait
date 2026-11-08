import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeAll, beforeEach, expect, it, vi } from "vitest";
import { commands } from "vitest/browser";
import { StyleSheet, UnistylesRuntime, withUnistyles } from "react-native-unistyles";
import { EnrichedMarkdownText } from "react-native-enriched-markdown";
import { parseMarkdown } from "react-native-enriched-markdown/src/web/parseMarkdown";
import { darkTheme, lightTheme, REGISTERED_THEMES, type Theme } from "@/styles/theme";
import { createMarkdownStyles } from "@/components/markdown/styles";
// Only the fault injection uses internal APIs; rendering and recovery use public APIs.
import { services } from "../../../../node_modules/react-native-unistyles/src/web/services";
import { subscribeToSystemTheme } from "./system-theme-sync.web";

declare module "vitest/browser" {
  interface BrowserCommands {
    setSystemColorScheme(colorScheme: "light" | "dark"): Promise<void>;
  }
}

Object.defineProperty(globalThis, "IS_REACT_ACT_ENVIRONMENT", { value: true, configurable: true });
const mapping = (theme: Theme) => createMarkdownStyles(theme, { blockGap: 12 });
const Markdown = withUnistyles(EnrichedMarkdownText, mapping);
const source = "Paragraph with `code`.\n\n| Head |\n| --- |\n| Cell |";
let host: HTMLDivElement;
let root: Root;
let unsubscribe: () => void;

beforeAll(async () => {
  StyleSheet.configure({
    themes: REGISTERED_THEMES,
    settings: { adaptiveThemes: true },
  });
  await parseMarkdown(source, { latexMath: false });
});

async function switchSystemTheme(colorScheme: "light" | "dark") {
  await act(async () => {
    await commands.setSystemColorScheme(colorScheme);
    // Media-query change events are delivered as part of the browser's rendering cycle.
    await new Promise(requestAnimationFrame);
    await new Promise(requestAnimationFrame);
  });
  expect(window.matchMedia("(prefers-color-scheme: dark)").matches).toBe(colorScheme === "dark");
}

beforeEach(async () => {
  UnistylesRuntime.setAdaptiveThemes(true);
  await switchSystemTheme("light");
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  unsubscribe = subscribeToSystemTheme();
  await act(async () =>
    root.render(<Markdown markdown={source} md4cFlags={{ latexMath: false }} />),
  );
});

afterEach(async () => {
  vi.restoreAllMocks();
  unsubscribe();
  await act(async () => root.unmount());
  host.remove();
});

function colors() {
  const style = (selector: string) => getComputedStyle(host.querySelector(selector)!);
  return {
    text: style("p").color,
    code: style("code").backgroundColor,
    headerText: style("th").color,
    header: style("th").backgroundColor,
    row: style("tbody tr").backgroundColor,
  };
}

function palette(theme: Theme) {
  const rgb = (color: string) => {
    const element = document.createElement("span");
    element.style.color = color;
    return element.style.color;
  };
  return {
    text: rgb(theme.colors.foreground),
    code: rgb(theme.colors.surface2),
    headerText: rgb(theme.colors.foreground),
    header: rgb(theme.colors.surface2),
    row: rgb(theme.colors.surface0),
  };
}

it("updates mounted prose, inline code and tables in both directions", async () => {
  expect(colors()).toEqual(palette(lightTheme));
  await switchSystemTheme("dark");
  expect(colors()).toEqual(palette(darkTheme));
  await switchSystemTheme("light");
  expect(colors()).toEqual(palette(lightTheme));
});

it.each(["focus", "pageshow", "visibilitychange"])(
  "repairs stale dark Markdown on %s after a hidden round trip to light",
  async (event) => {
    vi.spyOn(document, "visibilityState", "get").mockReturnValue("hidden");
    await switchSystemTheme("dark");
    expect(colors()).toEqual(palette(darkTheme));

    // Simulate losing just the return notification. The synchronizer's last visible
    // name remains light, but React consumers have received the intermediate dark theme.
    const notification = vi.spyOn(services.listener, "emitChanges").mockImplementation(() => {});
    await switchSystemTheme("light");
    expect(UnistylesRuntime.themeName).toBe("light");
    expect(colors()).toEqual(palette(darkTheme));
    notification.mockRestore();
    vi.spyOn(document, "visibilityState", "get").mockReturnValue("visible");

    await act(async () => {
      (event === "visibilitychange" ? document : window).dispatchEvent(new Event(event));
    });
    expect(colors()).toEqual(palette(lightTheme));
  },
);

it("keeps a fixed palette when the system changes", async () => {
  await act(async () => {
    UnistylesRuntime.setAdaptiveThemes(false);
    UnistylesRuntime.setTheme("light");
  });
  await switchSystemTheme("dark");
  await act(async () => window.dispatchEvent(new Event("focus")));
  expect(colors()).toEqual(palette(lightTheme));
});
