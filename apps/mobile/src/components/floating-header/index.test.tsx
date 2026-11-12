// @vitest-environment jsdom
import { act, useEffect, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import type { LayoutChangeEvent } from "react-native";
import { FloatingHeaderLayout } from "./index";

const layout = vi.hoisted(() => ({
  onLayout: undefined as ((event: LayoutChangeEvent) => void) | undefined,
}));
vi.mock("react-native", () => ({
  View: ({
    children,
    style,
    onLayout,
  }: {
    children?: ReactNode;
    style?: React.CSSProperties;
    onLayout?: (event: LayoutChangeEvent) => void;
  }) => {
    if (onLayout) layout.onLayout = onLayout;
    return <div style={style}>{children}</div>;
  },
}));
vi.mock("react-native-unistyles", () => ({
  StyleSheet: {
    create: (factory: (theme: unknown) => unknown) =>
      factory({ colors: { surfaceWorkspace: "#123456" } }),
  },
}));
vi.mock("@/components/ui/edge-fade", () => ({
  EdgeFade: ({ style, solidFraction }: { style: React.CSSProperties; solidFraction: number }) => (
    <div data-testid="fade" data-solid-fraction={solidFraction} style={style} />
  ),
}));

const container = document.createElement("div");
let root: ReturnType<typeof createRoot>;
afterEach(async () => {
  await act(async () => root.unmount());
  container.replaceChildren();
  vi.unstubAllGlobals();
});

it("keeps the whole measured native header opaque and starts the fade below it", async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  root = createRoot(container);
  let mounts = 0;
  function Transcript({ inset }: { inset: number }) {
    useEffect(() => {
      mounts += 1;
    }, []);
    return (
      <span data-testid="transcript" data-inset={inset}>
        message
      </span>
    );
  }
  const render = (floating: boolean) =>
    root.render(
      <FloatingHeaderLayout header={<span>Title and subtitle</span>} floating={floating}>
        {(inset) => <Transcript inset={inset} />}
      </FloatingHeaderLayout>,
    );
  await act(async () => render(true));
  const header = Array.from(container.querySelectorAll("div")).find(
    (node) => node.style.backgroundColor === "rgb(18, 52, 86)",
  );
  expect(header?.textContent).toBe("Title and subtitle");
  const fade = container.querySelector<HTMLElement>('[data-testid="fade"]')!;
  expect(fade.style.height).toBe("48px");
  expect(fade.style.bottom).toBe("0px");
  expect(fade.style.top).toBe("");
  expect(fade.dataset.solidFraction).toBe("0");
  await act(async () =>
    layout.onLayout?.({ nativeEvent: { layout: { height: 96.4 } } } as LayoutChangeEvent),
  );
  expect(container.querySelector('[data-testid="transcript"]')?.getAttribute("data-inset")).toBe(
    "96",
  );
  await act(async () => render(false));
  expect(container.querySelector('[data-testid="transcript"]')?.getAttribute("data-inset")).toBe(
    "0",
  );
  expect(container.querySelector('[data-testid="fade"]')).toBeNull();
  expect(mounts).toBe(1);
});
