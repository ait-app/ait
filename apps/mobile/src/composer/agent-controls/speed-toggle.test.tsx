/** @vitest-environment jsdom */
import React from "react";
import { cleanup, fireEvent, render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AgentFeature } from "@ait/protocol/agent-types";
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

beforeEach(() => vi.stubGlobal("React", React));
afterEach(cleanup);

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
  it.each(["toolbar", "sheet"] as const)(
    "switches Normal and Fast directly on the %s without opening a list",
    (surface) => {
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

      fireEvent.click(view.getByRole("button", { name: "Speed: Normal" }));

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

      fireEvent.click(view.getByRole("button", { name: "Speed: Fast" }));

      expect(onSetFeature).toHaveBeenLastCalledWith("service_tier", "default");
      expect(view.queryByRole("listbox")).toBeNull();
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
