/**
 * @vitest-environment jsdom
 */
import React from "react";
import { fireEvent, render } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AgentControlTrigger } from "./control";

vi.mock("@/components/ui/loading-spinner", () => ({
  LoadingSpinner: () => <span data-testid="busy-spinner" />,
}));

beforeEach(() => vi.stubGlobal("React", React));

function TestIcon() {
  return null;
}

describe("AgentControlTrigger", () => {
  it("forwards interaction handlers to the rendered trigger", () => {
    const onPointerEnter = vi.fn();
    const onFocus = vi.fn();
    const onPress = vi.fn();
    const view = render(
      <AgentControlTrigger
        icon={TestIcon}
        surface="toolbar"
        label="Mode"
        onPress={onPress}
        onPointerEnter={onPointerEnter}
        onFocus={onFocus}
        accessibilityLabel="Select mode"
      />,
    );
    const trigger = view.getByRole("button", { name: "Select mode" });

    fireEvent.pointerEnter(trigger);
    fireEvent.focus(trigger);

    expect(onPointerEnter).toHaveBeenCalledTimes(1);
    expect(onFocus).toHaveBeenCalledTimes(1);
  });

  it("shows a busy spinner instead of the icon while awaiting confirmation", () => {
    const view = render(
      <AgentControlTrigger
        icon={TestIcon}
        surface="sheet"
        label="Thinking"
        busy
        onPress={vi.fn()}
        accessibilityLabel="Select thinking option"
      />,
    );

    expect(view.getByTestId("busy-spinner")).toBeTruthy();
  });
});
