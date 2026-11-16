import React from "react";
import { flushSync } from "react-dom";
import { createRoot, type Root } from "react-dom/client";
import { Pressable, Text } from "react-native";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { Tooltip, TooltipContent, TooltipTrigger } from "./tooltip";

vi.mock("@/constants/layout", () => ({ useIsCompactFormFactor: () => false }));

function PreviewTooltip({ open }: { open: boolean }) {
  return (
    <Tooltip open={open}>
      <TooltipTrigger asChild>
        <Pressable>
          <Text>Preview</Text>
        </Pressable>
      </TooltipTrigger>
      <TooltipContent testID="preview-tooltip">
        <Text>Markdown preview</Text>
      </TooltipContent>
    </Tooltip>
  );
}

describe("web tooltip cleanup", () => {
  let container: HTMLDivElement;
  let root: Root;
  let animationOverride: HTMLStyleElement;

  beforeEach(() => {
    vi.stubGlobal("React", React);
    container = document.createElement("div");
    document.body.append(container);
    root = createRoot(container);
    // Cleanup must not depend on the browser delivering animation events.
    animationOverride = document.createElement("style");
    animationOverride.textContent =
      '[data-testid="preview-tooltip"] { animation: none !important; }';
    document.head.append(animationOverride);
  });

  afterEach(() => {
    flushSync(() => root.unmount());
    container.remove();
    document.getElementById("overlay-root")?.remove();
    animationOverride.remove();
    vi.unstubAllGlobals();
  });

  async function openTooltip() {
    flushSync(() => root.render(<PreviewTooltip open />));
    await expect
      .poll(
        () =>
          document.querySelector('[data-testid="preview-tooltip"]')?.getBoundingClientRect().top,
      )
      .toBeGreaterThanOrEqual(0);
  }

  for (const kind of ["DOM", "React Native"] as const) {
    it(`positions content using a ${kind} trigger`, async () => {
      const triggerStyle = {
        position: "absolute" as const,
        left: 100,
        top: 80,
        width: 60,
        height: 20,
      };
      flushSync(() =>
        root.render(
          <Tooltip open>
            <TooltipTrigger asChild>
              {kind === "DOM" ? (
                <span style={triggerStyle}>File link</span>
              ) : (
                <Pressable style={triggerStyle}>
                  <Text>File link</Text>
                </Pressable>
              )}
            </TooltipTrigger>
            <TooltipContent testID="preview-tooltip" side="bottom" align="start">
              <Text>File path</Text>
            </TooltipContent>
          </Tooltip>,
        ),
      );
      await expect
        .poll(() => {
          const rect = document
            .querySelector('[data-testid="preview-tooltip"]')
            ?.getBoundingClientRect();
          return { left: rect?.left, top: rect?.top };
        })
        .toEqual({ left: 100, top: 106 });
    });
  }

  it("removes the entire surface when closed without animation events", async () => {
    await openTooltip();
    flushSync(() => root.render(<PreviewTooltip open={false} />));
    await expect.poll(() => document.getElementById("overlay-root")?.childElementCount).toBe(0);
  });

  it("removes the entire surface when its trigger is unmounted", async () => {
    await openTooltip();
    flushSync(() => root.render(null));
    await expect.poll(() => document.getElementById("overlay-root")?.childElementCount).toBe(0);
  });

  it("can reopen without accumulating surfaces", async () => {
    animationOverride.remove();
    for (let index = 0; index < 3; index += 1) {
      await openTooltip();
      const surfaces = document.querySelectorAll('[data-testid="preview-tooltip"]');
      expect(surfaces).toHaveLength(1);
      expect(surfaces[0].textContent).toBe("Markdown preview");
      expect(getComputedStyle(surfaces[0]).visibility).toBe("visible");
      flushSync(() => root.render(<PreviewTooltip open={false} />));
      await expect.poll(() => document.getElementById("overlay-root")?.childElementCount).toBe(0);
    }
  });

  it("stays removed when closed before positioning finishes", async () => {
    flushSync(() => root.render(<PreviewTooltip open />));
    flushSync(() => root.render(<PreviewTooltip open={false} />));
    await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
    expect(document.getElementById("overlay-root")?.childElementCount).toBe(0);
  });
});
