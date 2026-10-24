/** @vitest-environment jsdom */
import React, { type ComponentProps } from "react";
import { cleanup, fireEvent, render, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { i18n } from "@/i18n/i18next";
import { buildSelectableProviderSelectorProviders } from "@/provider-selection/provider-selection";
import { CompactModelSheet } from "@/composer/agent-controls/model-sheet";
import { CombinedModelSelector } from "./combined-model-selector";

const layout = vi.hoisted(() => ({ compact: false }));
vi.mock("@/constants/layout", () => ({ useIsCompactFormFactor: () => layout.compact }));
vi.mock("@/agent-profiles", () => ({ AgentProfileGlyph: () => null }));

// Keep the selector and model browser real; replace only overlay presentation.
vi.mock("@/components/ui/combobox", () => ({
  Combobox: ({ open, children }: ComponentProps<typeof import("./ui/combobox").Combobox>) =>
    open ? <div role="dialog">{children}</div> : null,
}));
vi.mock("@/components/adaptive-modal-sheet", () => ({
  AdaptiveModalSheet: ({
    visible,
    children,
    testID,
  }: ComponentProps<typeof import("./adaptive-modal-sheet").AdaptiveModalSheet>) =>
    visible ? <div data-testid={testID}>{children}</div> : null,
}));

beforeEach(async () => {
  vi.stubGlobal("React", React);
  await i18n.changeLanguage("en");
});
afterEach(cleanup);

function providers(ompReady = false) {
  return buildSelectableProviderSelectorProviders([
    {
      provider: "omp",
      label: "OMP",
      enabled: true,
      status: ompReady ? "ready" : "loading",
      models: ompReady
        ? [{ provider: "omp", id: "remembered-model", label: "Remembered model" }]
        : [],
    },
    {
      provider: "codex",
      label: "Codex",
      enabled: true,
      status: "ready",
      models: [{ provider: "codex", id: "gpt-5.4", label: "GPT-5.4" }],
    },
  ]);
}

describe.each(["combined", "compact"] as const)("%s model selector with slow OMP", (surface) => {
  it.each([false, true])("can switch providers with OMP completed = %s", (ompReady) => {
    layout.compact = surface === "compact";
    const onSelect = vi.fn();
    function Selector({ ompReady = false }: { ompReady?: boolean }) {
      const props = {
        providers: providers(ompReady),
        selectedProvider: "omp",
        selectedModel: "remembered-model",
        isLoading: !ompReady,
        onSelect,
      };
      return surface === "compact" ? (
        <CompactModelSheet {...props} thinkingLabel={null} glyphSize={20} canSwitchProvider>
          {null}
        </CompactModelSheet>
      ) : (
        <CombinedModelSelector {...props} />
      );
    }

    const view = render(<Selector />);
    const trigger = view.getByTestId("combined-model-selector");
    expect(trigger.textContent).toBe("Select model");
    expect(trigger.getAttribute("aria-disabled")).not.toBe("true");
    fireEvent.click(trigger);
    if (surface === "compact") {
      fireEvent.click(view.getByTestId("agent-controls-model"));
    }

    expect(within(view.getByTestId("model-provider-omp")).getByText("Loading")).toBeTruthy();
    expect(within(view.getByTestId("model-provider-codex")).getByText("1 model")).toBeTruthy();
    fireEvent.click(view.getByTestId("model-provider-codex"));
    expect(view.getByText("GPT-5.4")).toBeTruthy();

    // OMP finishing in the background must not pull the browser away from Codex.
    if (ompReady) view.rerender(<Selector ompReady />);
    fireEvent.click(view.getByText("GPT-5.4"));
    expect(onSelect).toHaveBeenCalledExactlyOnceWith("codex", "gpt-5.4");
  });
});
