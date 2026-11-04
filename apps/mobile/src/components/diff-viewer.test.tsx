import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { JSDOM } from "jsdom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { DiffLine } from "@/utils/tool-call-parsers";
import { DiffViewer } from "./diff-viewer";

vi.mock("react-native-unistyles", () => {
  const styles = new Proxy({}, { get: () => ({}) });
  return { StyleSheet: { create: () => styles } };
});

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

vi.mock("@/constants/platform", () => ({ isWeb: true, isNative: false }));

vi.mock("@/styles/syntax-token-styles", () => ({ syntaxTokenStyleFor: () => undefined }));

vi.mock("react-native-gesture-handler", () => ({ ScrollView: () => null }));

vi.mock("react-native", () => {
  const passthrough =
    (tag: string) =>
    ({ children }: { children?: React.ReactNode }) =>
      React.createElement(tag, null, children);
  return { View: passthrough("div"), Text: passthrough("span"), ScrollView: passthrough("div") };
});

let root: Root | null = null;
let container: HTMLElement | null = null;

beforeEach(() => {
  const dom = new JSDOM("<!doctype html><html><body></body></html>");
  vi.stubGlobal("window", dom.window);
  vi.stubGlobal("document", dom.window.document);
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root?.unmount());
  root = null;
  container = null;
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("DiffViewer", () => {
  it("renders repeated word-diff segments without key collisions", () => {
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => {});
    // `a b a` -> `x b x` yields two identical changed segments on each side.
    const diffLines: DiffLine[] = [
      {
        type: "add",
        content: "+x b x",
        segments: [
          { text: "x", changed: true },
          { text: " b ", changed: false },
          { text: "x", changed: true },
        ],
      },
    ];

    act(() => root?.render(<DiffViewer diffLines={diffLines} />));

    expect(container?.textContent).toBe("+x b x");
    const keyWarnings = consoleError.mock.calls.filter((call) =>
      call.some((argument) => typeof argument === "string" && argument.includes("same key")),
    );
    expect(keyWarnings).toEqual([]);
  });
});
