import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { page, userEvent } from "vitest/browser";
import { ProviderControlDock } from "./provider-control-dock";
import { ProviderVibe } from "./provider-vibe";

vi.mock("react-i18next", async () => {
  const { zhCN } = await import("@/i18n/resources/zh-CN");
  return {
    useTranslation: () => ({
      t: (key: string) =>
        key === "providerControl.vibe.title"
          ? zhCN.providerControl.vibe.title
          : zhCN.providerControl.runtimeControls,
    }),
  };
});

vi.mock("react-native-unistyles", async () => {
  const { lightTheme } = await import("@/styles/theme");
  return {
    StyleSheet: { create: (factory: (theme: typeof lightTheme) => unknown) => factory(lightTheme) },
  };
});

Object.defineProperty(globalThis, "IS_REACT_ACT_ENVIRONMENT", { value: true, configurable: true });
let root: Root | null = null;
afterEach(async () => {
  await act(async () => root?.unmount());
  root = null;
  document.body.replaceChildren();
});

it.each([390, 1024])(
  "keeps Vibe informational beside keyboard-operable pills at %i px",
  async (width) => {
    await page.viewport(width, 300);
    const host = document.createElement("div");
    host.style.cssText = "padding:16px;box-sizing:border-box;background:#fafafa";
    document.body.append(host);
    root = createRoot(host);
    const render = (enabled: boolean) => (
      <ProviderControlDock
        owner={`browser-${width}`}
        status={enabled ? <ProviderVibe state={{ enabled, workers: [] }} /> : null}
        items={[
          { id: "goal", label: "Goal · Active", content: <p>Current goal</p> },
          {
            id: "btw",
            label: "BTW",
            content: (
              <textarea
                aria-label="BTW question"
                defaultValue="Draft"
                style={{
                  display: "block",
                  width: "100%",
                  height: 58,
                  boxSizing: "border-box",
                  outline: "2px solid green",
                  outlineOffset: 2,
                }}
              />
            ),
          },
        ]}
      />
    );
    await act(async () => root!.render(render(true)));
    const status = host.querySelector<HTMLElement>('[data-testid="provider-vibe"]')!;
    const goal = host.querySelector<HTMLElement>('[data-testid="provider-control-toggle-goal"]')!;
    const btw = host.querySelector<HTMLElement>('[data-testid="provider-control-toggle-btw"]')!;
    expect(status.textContent).toBe("Vibe ON");
    expect(status.querySelector("button, [role=button], [tabindex]")).toBeNull();
    expect(status.hasAttribute("tabindex")).toBe(false);
    expect(status.hasAttribute("aria-expanded")).toBe(false);
    expect(status.getBoundingClientRect().height).toBe(goal.getBoundingClientRect().height);
    expect(status.getBoundingClientRect().y).toBe(btw.getBoundingClientRect().y);
    expect(getComputedStyle(status).borderRadius).toBe(getComputedStyle(goal).borderRadius);
    expect(host.scrollWidth).toBeLessThanOrEqual(width);
    await page.getByTestId("provider-vibe").click();
    expect(goal.getAttribute("aria-expanded")).toBe("false");
    expect(btw.getAttribute("aria-expanded")).toBe("false");
    await page.getByTestId("provider-control-dock").screenshot();

    goal.focus();
    await userEvent.keyboard("{Tab}");
    expect(document.activeElement).toBe(btw);
    await userEvent.keyboard("{Enter}");
    expect(btw.getAttribute("aria-expanded")).toBe("true");
    const input = host.querySelector<HTMLTextAreaElement>("textarea")!;
    input.focus();
    const bounds = input.getBoundingClientRect();
    for (
      let parent = input.parentElement;
      parent && parent !== host;
      parent = parent.parentElement
    ) {
      const style = getComputedStyle(parent);
      const clip = parent.getBoundingClientRect();
      if (/hidden|auto|scroll|clip/.test(style.overflowX)) {
        expect(bounds.left - 4).toBeGreaterThanOrEqual(clip.left - 0.5);
        expect(bounds.right + 4).toBeLessThanOrEqual(clip.right + 0.5);
      }
      if (/hidden|auto|scroll|clip/.test(style.overflowY)) {
        expect(bounds.top - 4).toBeGreaterThanOrEqual(clip.top - 0.5);
        expect(bounds.bottom + 4).toBeLessThanOrEqual(clip.bottom + 0.5);
      }
    }
    btw.focus();
    await userEvent.keyboard("{Enter}");
    expect(btw.getAttribute("aria-expanded")).toBe("false");
    expect(status.textContent).toBe("Vibe ON");
    await act(async () => root!.render(render(false)));
    expect(host.querySelector('[data-testid="provider-vibe"]')).toBeNull();
    expect(btw.getAttribute("aria-expanded")).toBe("false");
  },
);

it("reveals the complete follow-up input and focus ring below a long BTW answer", async () => {
  await page.viewport(390, 844);
  const host = document.createElement("div");
  host.style.cssText = "padding:16px;box-sizing:border-box";
  document.body.append(host);
  root = createRoot(host);
  await act(async () =>
    root!.render(
      <ProviderControlDock
        owner="btw-follow-up-scroll"
        initial="btw"
        items={[
          {
            id: "btw",
            label: "BTW",
            content: ({ revealEnd }) => (
              <div>
                <div style={{ height: 240 }}>Completed BTW answer</div>
                <textarea
                  aria-label="Follow up"
                  onFocus={() => requestAnimationFrame(revealEnd)}
                  style={{
                    display: "block",
                    width: "100%",
                    height: 58,
                    boxSizing: "border-box",
                    outline: "2px solid green",
                    outlineOffset: 2,
                  }}
                />
              </div>
            ),
          },
        ]}
      />,
    ),
  );
  const input = host.querySelector("textarea")!;
  input.focus();
  await new Promise<void>((resolve) =>
    requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
  );
  const bounds = input.getBoundingClientRect();
  for (let parent = input.parentElement; parent && parent !== host; parent = parent.parentElement) {
    if (/hidden|auto|scroll|clip/.test(getComputedStyle(parent).overflowY)) {
      const clip = parent.getBoundingClientRect();
      expect(bounds.top - 4).toBeGreaterThanOrEqual(clip.top - 0.5);
      expect(bounds.bottom + 4).toBeLessThanOrEqual(clip.bottom + 0.5);
    }
  }
});
