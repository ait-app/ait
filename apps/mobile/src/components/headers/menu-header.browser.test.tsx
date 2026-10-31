import React from "react";
import { flushSync } from "react-dom";
import { createRoot, type Root } from "react-dom/client";
import { page } from "vitest/browser";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { TitlebarDragRegion } from "@/components/desktop/titlebar-drag-region";
import { HEADER_CONTROL_HEIGHT } from "@/components/ui/control-geometry";
import { HEADER_INNER_HEIGHT } from "@/constants/layout";
import { usePanelStore } from "@/stores/panel-store";
import {
  WindowChromeProvider,
  WindowChromeRegion,
  WindowChromeSafeArea,
} from "@/utils/desktop-window";
import { WindowSidebarMenuToggle } from "./menu-header";

vi.mock("@/constants/layout", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/constants/layout")>()),
  getIsElectronRuntime: () => true,
  useIsCompactFormFactor: () => false,
}));
vi.mock("@/desktop/host", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/desktop/host")>()),
  getDesktopWindowChromeMode: () => "native-mac",
}));
vi.mock("@/desktop/electron/window", () => ({
  getDesktopWindow: () => ({
    isFullscreen: async () => false,
    isMaximized: async () => false,
    onResized: async () => () => {},
  }),
}));
vi.mock("@/utils/shortcut-platform", () => ({ getShortcutOs: () => "mac" }));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

describe("macOS window sidebar toggle", () => {
  let container: HTMLDivElement;
  let root: Root;
  let styles: HTMLStyleElement[];

  beforeEach(async () => {
    vi.stubGlobal("React", React);
    const response = await fetch(new URL("../../../public/index.html", import.meta.url));
    expect(response.ok).toBe(true);
    const appHtml = new DOMParser().parseFromString(await response.text(), "text/html");
    styles = Array.from(appHtml.querySelectorAll("style"));
    document.head.append(...styles);
    container = document.createElement("div");
    document.body.append(container);
    root = createRoot(container);
    usePanelStore.getState().openDesktopAgentList();
  });

  afterEach(() => {
    flushSync(() => root.unmount());
    container.remove();
    styles.forEach((style) => style.remove());
    usePanelStore.setState(usePanelStore.getInitialState());
    vi.unstubAllGlobals();
  });

  it("accepts clicks over a sibling titlebar drag region to close and reopen the sidebar", async () => {
    flushSync(() => {
      root.render(
        <WindowChromeProvider>
          <div
            style={{ position: "fixed", top: 0, left: 0, width: 800, height: HEADER_INNER_HEIGHT }}
          >
            <div style={{ position: "relative", height: HEADER_INNER_HEIGHT }}>
              <TitlebarDragRegion />
            </div>
            <WindowChromeRegion corners="top-left">
              <WindowChromeSafeArea
                placement="inline"
                horizontalPadding={12}
                pointerEvents="box-none"
                style={{
                  position: "absolute",
                  top: 1,
                  left: 0,
                  zIndex: 20,
                  height: HEADER_INNER_HEIGHT,
                  flexDirection: "row",
                  alignItems: "center",
                }}
              >
                {/* The Unistyles test stub does not resolve breakpoint values. */}
                <WindowSidebarMenuToggle
                  style={{
                    width: HEADER_CONTROL_HEIGHT,
                    height: HEADER_CONTROL_HEIGHT,
                    marginLeft: -8,
                  }}
                />
              </WindowChromeSafeArea>
            </WindowChromeRegion>
          </div>
        </WindowChromeProvider>,
      );
    });

    const button = page.getByTestId("menu-button");
    const element = button.element();
    const bounds = element.getBoundingClientRect();
    expect(
      element.contains(
        document.elementFromPoint(bounds.x + bounds.width / 2, bounds.y + bounds.height / 2),
      ),
    ).toBe(true);
    expect(element.getAttribute("aria-expanded")).toBe("true");

    await button.click();
    expect(usePanelStore.getState().desktop.agentListOpen).toBe(false);
    await expect.element(button).toHaveAttribute("aria-expanded", "false");

    await button.click();
    expect(usePanelStore.getState().desktop.agentListOpen).toBe(true);
    await expect.element(button).toHaveAttribute("aria-expanded", "true");

    // Chromium clicks alone bypass Electron's native drag hit testing. The
    // floating button must explicitly subtract its bounds from the drag region.
    expect(getComputedStyle(element).getPropertyValue("-webkit-app-region")).toBe("no-drag");
    expect(
      getComputedStyle(element.parentElement!).getPropertyValue("-webkit-app-region"),
    ).not.toBe("no-drag");
    const dragRegion = container.querySelector("[data-titlebar-drag-region]")!;
    expect(getComputedStyle(dragRegion).getPropertyValue("-webkit-app-region")).toBe("drag");
  });
});
