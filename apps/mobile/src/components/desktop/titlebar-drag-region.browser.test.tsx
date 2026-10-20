import React from "react";
import { flushSync } from "react-dom";
import { createRoot, type Root } from "react-dom/client";
import { Modal, Pressable, Text, View } from "react-native";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { TitlebarDragRegion } from "./titlebar-drag-region";

vi.mock("@/constants/layout", () => ({ getIsElectronRuntime: () => true }));
vi.mock("@/constants/platform", () => ({ isNative: false }));

function appRegion(element: Element): string {
  return getComputedStyle(element).getPropertyValue("-webkit-app-region");
}

describe("desktop titlebar drag exclusions", () => {
  let container: HTMLDivElement;
  let root: Root;
  let styles: HTMLStyleElement[];

  beforeEach(async () => {
    vi.stubGlobal("React", React);
    const response = await fetch(new URL("../../../public/index.html", import.meta.url));
    expect(response.ok).toBe(true);
    const appHtml = await response.text();
    const documentHtml = new DOMParser().parseFromString(appHtml, "text/html");
    styles = Array.from(documentHtml.querySelectorAll("style"));
    document.head.append(...styles);
    container = document.createElement("div");
    document.body.append(container);
    root = createRoot(container);
  });

  afterEach(() => {
    flushSync(() => root.unmount());
    container.remove();
    styles.forEach((style) => style.remove());
    vi.unstubAllGlobals();
  });

  it("does not let clipped chat controls subtract from the titlebar drag region", () => {
    flushSync(() => {
      root.render(
        <div style={{ position: "fixed", top: 0, left: 0, width: 800 }}>
          <div style={{ position: "relative", height: 40 }}>
            <TitlebarDragRegion />
            <button>Workspace actions</button>
          </div>
          <div data-testid="chat-scroll" style={{ height: 200, overflow: "auto" }}>
            <button data-testid="chat-control" style={{ display: "block", height: 40 }}>
              Expand tool output
            </button>
            <div style={{ height: 600 }} />
          </div>
        </div>,
      );
    });
    const scroller = container.querySelector<HTMLElement>('[data-testid="chat-scroll"]')!;
    scroller.scrollTop = 20;
    const chatControl = container.querySelector('[data-testid="chat-control"]')!;
    // The unclipped button bounds overlap the header. Electron can subtract these
    // bounds even though the scroll viewport clips the button below the header.
    expect(chatControl.getBoundingClientRect().top).toBe(20);
    expect(appRegion(chatControl)).not.toBe("no-drag");
    expect(appRegion(container.querySelector("button")!)).toBe("no-drag");
    const dragRegion = container.querySelector("[data-titlebar-drag-region]")!;
    expect(dragRegion).not.toBeNull();
    expect(appRegion(dragRegion)).toBe("drag");
  });

  it.each([
    { "data-testid": "desktop-window-controls" },
    { id: "overlay-root" },
    { "data-floating-panel-host": "true" },
  ])("keeps controls interactive in %j", (scope) => {
    flushSync(() => {
      root.render(
        <div {...scope}>
          <button>Action</button>
          <div role="menuitem" tabIndex={-1}>
            Menu item
          </div>
          <input />
        </div>,
      );
    });
    for (const control of container.querySelectorAll("button, [role=menuitem], input")) {
      expect(appRegion(control)).toBe("no-drag");
    }
    // The empty full-viewport portal host must not disable titlebar dragging.
    expect(appRegion(container.firstElementChild!)).not.toBe("no-drag");
  });

  it("excludes body-portaled modals from workspace drag regions only while mounted", async () => {
    flushSync(() => {
      root.render(
        <>
          <div style={{ position: "fixed", inset: 0 }}>
            <TitlebarDragRegion />
          </div>
          <Modal visible transparent animationType="none">
            <View>
              <Pressable accessibilityRole="button">
                <Text>Back</Text>
              </Pressable>
              <Pressable accessibilityRole="button">
                <Text>Choose host</Text>
              </Pressable>
            </View>
          </Modal>
        </>,
      );
    });
    await vi.waitFor(() => {
      const dialog = document.querySelector('[role="dialog"][aria-modal="true"]');
      expect(dialog).not.toBeNull();
      expect(dialog!.closest("#overlay-root")).toBeNull();
      expect(appRegion(dialog!)).toBe("no-drag");
    });

    flushSync(() => {
      root.render(
        <div style={{ position: "fixed", inset: 0 }}>
          <TitlebarDragRegion />
        </div>,
      );
    });
    expect(document.querySelector('[aria-modal="true"]')).toBeNull();
    expect(appRegion(container.querySelector("[data-titlebar-drag-region]")!)).toBe("drag");
  });
});
