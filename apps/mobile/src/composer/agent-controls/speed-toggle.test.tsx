/** @vitest-environment jsdom */
import React from "react";
import { act, cleanup, fireEvent, render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AgentFeature } from "@ait/protocol/agent-types";
import { i18n } from "@/i18n/i18next";
import { SpeedToggle } from "./speed-toggle";

vi.mock("react-native-unistyles", async (importOriginal) => {
  const actual = await importOriginal<typeof import("react-native-unistyles")>();
  return {
    ...actual,
    useUnistyles: () => {
      const current = actual.useUnistyles();
      return {
        ...current,
        theme: {
          ...current.theme,
          colors: {
            ...current.theme.colors,
            palette: { ...current.theme.colors.palette, yellow: { 400: "#facc15" } },
          },
        },
      };
    },
  };
});

beforeEach(async () => {
  vi.stubGlobal("React", React);
  await i18n.changeLanguage("en");
});
afterEach(async () => {
  cleanup();
  await i18n.changeLanguage("en");
});

function speed(value: string): Extract<AgentFeature, { type: "select" }> {
  return {
    id: "service_tier",
    type: "select",
    label: "Speed",
    icon: "zap",
    value,
    options: [
      { id: "default", label: "Normal", isDefault: true },
      { id: "fast", label: "Fast" },
    ],
  };
}

describe("SpeedToggle", () => {
  it.each([
    ["toolbar", "en", "Speed: Normal", "Speed: Fast"],
    ["sheet", "en", "Speed: Normal", "Speed: Fast"],
    ["toolbar", "zh-CN", "速度：标准", "速度：快速"],
    ["sheet", "zh-CN", "速度：标准", "速度：快速"],
  ] as const)(
    "switches Normal and Fast directly on the %s in %s without opening a list",
    async (surface, language, normalLabel, fastLabel) => {
      await i18n.changeLanguage(language);
      const onSetFeature = vi.fn();
      const onActionComplete = vi.fn();
      const view = render(
        <SpeedToggle
          feature={speed("default")}
          surface={surface}
          disabled={false}
          onSetFeature={onSetFeature}
          onActionComplete={onActionComplete}
        />,
      );

      fireEvent.click(view.getByRole("button", { name: normalLabel }));

      expect(onSetFeature).toHaveBeenLastCalledWith("service_tier", "fast");
      expect(onActionComplete).toHaveBeenCalledTimes(1);
      expect(view.queryByRole("listbox")).toBeNull();
      expect(view.queryByRole("combobox")).toBeNull();
      view.rerender(
        <SpeedToggle
          feature={speed("fast")}
          surface={surface}
          disabled={false}
          onSetFeature={onSetFeature}
        />,
      );

      fireEvent.click(view.getByRole("button", { name: fastLabel }));

      expect(onSetFeature).toHaveBeenLastCalledWith("service_tier", "default");
      expect(view.queryByRole("listbox")).toBeNull();
      await act(async () => {
        await i18n.changeLanguage(language === "en" ? "zh-CN" : "en");
      });
      expect(
        view.getByRole("button", { name: language === "en" ? "速度：快速" : "Speed: Fast" }),
      ).toBeTruthy();
    },
  );

  it("treats a legacy priority selection as enabled and can switch it off", () => {
    const onSetFeature = vi.fn();
    const feature = speed("priority");
    feature.options.push({ id: "priority", label: "Fast" });
    const view = render(
      <SpeedToggle
        feature={feature}
        surface="sheet"
        disabled={false}
        onSetFeature={onSetFeature}
      />,
    );

    fireEvent.click(view.getByRole("button", { name: "Speed: Fast" }));

    expect(onSetFeature).toHaveBeenCalledWith("service_tier", "default");
    expect(view.getAllByRole("button")).toHaveLength(1);
  });

  it("cannot toggle a disabled control", () => {
    const onSetFeature = vi.fn();
    const view = render(
      <SpeedToggle
        feature={speed("default")}
        surface="sheet"
        disabled
        onSetFeature={onSetFeature}
      />,
    );

    fireEvent.click(view.getByRole("button", { name: "Speed: Normal" }));

    expect(onSetFeature).not.toHaveBeenCalled();
  });
});
