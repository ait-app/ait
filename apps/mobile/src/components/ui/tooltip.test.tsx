import React from "react";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { JSDOM } from "jsdom";
import { Dimensions, Pressable, Text, type LayoutChangeEvent } from "react-native";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { Tooltip, TooltipContent, TooltipTrigger } from "./tooltip";
import { FloatingSurface } from "./floating";

vi.mock("./floating", () => ({
  FloatingSurface: vi.fn(({ children }: { children?: React.ReactNode }) => <div>{children}</div>),
}));

vi.mock("@/constants/platform", () => ({
  isWeb: true,
  isNative: false,
}));

vi.mock("@/constants/layout", () => ({
  useIsCompactFormFactor: () => false,
}));

vi.mock("@gorhom/portal", () => ({
  Portal: ({ children }: { children: React.ReactNode }) => children,
}));

vi.mock("@gorhom/bottom-sheet", () => ({
  useBottomSheetModalInternal: () => null,
}));

vi.mock("react-native-reanimated", () => ({
  default: {
    View: "div",
  },
  FadeIn: { duration: vi.fn() },
  FadeOut: { duration: vi.fn() },
}));

vi.mock("react-native-unistyles", () => ({
  StyleSheet: {
    create: (styles: unknown) => styles,
  },
}));

let root: Root | null = null;
let container: HTMLElement | null = null;

beforeEach(() => {
  const dom = new JSDOM("<!doctype html><html><body></body></html>");
  vi.stubGlobal("React", React);
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal("window", dom.window);
  vi.stubGlobal("document", dom.window.document);
  vi.stubGlobal("HTMLElement", dom.window.HTMLElement);
  vi.stubGlobal("Node", dom.window.Node);
  vi.stubGlobal("navigator", dom.window.navigator);

  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  vi.mocked(FloatingSurface).mockClear();
});

afterEach(() => {
  if (root) {
    act(() => {
      root?.unmount();
    });
  }
  root = null;
  container = null;
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

function renderTrigger({
  childDisabled,
  onPress,
}: {
  childDisabled: boolean;
  onPress: () => void;
}): void {
  act(() => {
    root?.render(
      <Tooltip>
        <TooltipTrigger asChild>
          <Pressable disabled={childDisabled} onPress={onPress} testID="trigger">
            <Text>Send</Text>
          </Pressable>
        </TooltipTrigger>
      </Tooltip>,
    );
  });
}

function pressTrigger(): void {
  const trigger = container?.querySelector('[data-testid="trigger"]');
  expect(trigger).not.toBeNull();

  act(() => {
    trigger?.dispatchEvent(new window.MouseEvent("click", { bubbles: true }));
  });
}

describe("TooltipTrigger", () => {
  it("keeps an asChild trigger disabled when the child is disabled", () => {
    const onPress = vi.fn();

    renderTrigger({ childDisabled: true, onPress });
    pressTrigger();

    expect(onPress).not.toHaveBeenCalled();
  });

  it("keeps an asChild trigger interactive when the child is not disabled", () => {
    const onPress = vi.fn();

    renderTrigger({ childDisabled: false, onPress });
    pressTrigger();

    expect(onPress).toHaveBeenCalledTimes(1);
  });
});

// Markdown file links use an inline DOM span rather than a React Native View.
const InlineTrigger = React.forwardRef<HTMLSpanElement, React.ComponentProps<"span">>(
  function InlineTrigger({ children }, ref) {
    return <span ref={ref}>{children}</span>;
  },
);

describe("TooltipContent measurement", () => {
  for (const kind of ["DOM", "React Native"] as const) {
    it(`positions content using a ${kind} trigger`, async () => {
      vi.spyOn(Dimensions, "get").mockReturnValue({
        width: 1024,
        height: 768,
        scale: 1,
        fontScale: 1,
      });
      const measure = vi.fn();
      if (kind === "DOM") {
        measure.mockReturnValue({ left: 100, top: 80, width: 60, height: 20 });
      } else {
        measure.mockImplementation((callback) => callback(100, 80, 60, 20));
      }
      const setRef = (node: unknown) => {
        if (node) {
          Object.defineProperty(
            node,
            kind === "DOM" ? "getBoundingClientRect" : "measureInWindow",
            {
              configurable: true,
              writable: true,
              value: measure,
            },
          );
        }
      };
      const render = (open: boolean) => (
        <Tooltip open={open}>
          <TooltipTrigger asChild>
            {kind === "DOM" ? (
              <InlineTrigger ref={setRef}>File link</InlineTrigger>
            ) : (
              <Pressable ref={setRef}>
                <Text>File link</Text>
              </Pressable>
            )}
          </TooltipTrigger>
          <TooltipContent side="bottom" align="start">
            File path
          </TooltipContent>
        </Tooltip>
      );
      await act(async () => root?.render(render(false)));

      await act(async () => root?.render(render(true)));
      expect(measure).toHaveBeenCalledOnce();

      const onLayout = vi.mocked(FloatingSurface).mock.lastCall![0].onLayout;
      expect(onLayout).toBeTypeOf("function");
      await act(async () => {
        if (typeof onLayout === "function") {
          onLayout({
            nativeEvent: { layout: { x: 0, y: 0, width: 80, height: 30 } },
          } as LayoutChangeEvent);
        }
      });
      expect(vi.mocked(FloatingSurface).mock.lastCall![0].frameStyle).toEqual([
        { position: "absolute", left: 100, top: 106, maxWidth: 280 },
      ]);
    });
  }
});
