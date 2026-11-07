/** @vitest-environment jsdom */
import React, { type ComponentProps } from "react";
import { cleanup, fireEvent, render, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { i18n } from "@/i18n/i18next";
import { buildSelectableProviderSelectorProviders } from "@/provider-selection/provider-selection";
import { CompactModelSheet } from "@/composer/agent-controls/model-sheet";
import { CombinedModelSelector, resolveModelConfigurationFrame } from "./combined-model-selector";
import type { Combobox } from "./ui/combobox";
import type { AdaptiveModalSheet } from "./adaptive-modal-sheet";

const layout = vi.hoisted(() => ({ compact: false }));
vi.mock("@/constants/layout", () => ({ useIsCompactFormFactor: () => layout.compact }));
vi.mock("@/agent-profiles", () => ({ AgentProfileGlyph: () => null }));

// Keep the selector and model browser real; replace only overlay presentation.
vi.mock("@/components/ui/combobox", () => ({
  Combobox: ({
    open,
    children,
    desktopMinWidth,
    desktopFixedHeight,
  }: ComponentProps<typeof Combobox>) =>
    open ? (
      <div role="dialog" data-min-width={desktopMinWidth} data-fixed-height={desktopFixedHeight}>
        {children}
      </div>
    ) : null,
}));
vi.mock("@/components/adaptive-modal-sheet", () => ({
  AdaptiveModalSheet: ({ visible, children, testID }: ComponentProps<typeof AdaptiveModalSheet>) =>
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
    const onCatalogAccess = vi.fn();
    function Selector({ ompReady = false }: { ompReady?: boolean }) {
      const props = {
        providers: providers(ompReady),
        selectedProvider: "omp",
        selectedModel: "remembered-model",
        isLoading: !ompReady,
        onSelect,
        onCatalogAccess,
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
    expect(onCatalogAccess).toHaveBeenCalledTimes(1);
    if (surface === "compact") {
      fireEvent.click(view.getByTestId("agent-controls-model"));
    }

    expect(within(view.getByTestId("model-provider-omp")).getByText("Loading")).toBeTruthy();
    expect(within(view.getByTestId("model-provider-codex")).getByText("1 model")).toBeTruthy();
    fireEvent.click(view.getByTestId("model-provider-codex"));
    expect(view.getByText("GPT-5.4")).toBeTruthy();

    // OMP finishing in the background must not pull the browser away from Codex.
    if (ompReady) view.rerender(<Selector ompReady />);
    const accessesBeforeSelection = onCatalogAccess.mock.calls.length;
    fireEvent.click(view.getByText("GPT-5.4"));
    expect(onCatalogAccess).toHaveBeenCalledTimes(accessesBeforeSelection + 1);
    expect(onSelect).toHaveBeenCalledExactlyOnceWith("codex", "gpt-5.4");
  });
});

it("reserves visible space for model configuration next to a scrollable model list", () => {
  layout.compact = false;
  const action = vi.fn();
  const view = render(
    <CombinedModelSelector
      providers={providers(true)}
      selectedProvider="omp"
      selectedModel="remembered-model"
      isLoading={false}
      onSelect={vi.fn()}
      modelConfiguration={<button onClick={action}>Model roles</button>}
    />,
  );
  fireEvent.click(view.getByTestId("combined-model-selector"));
  const dialog = view.getByRole("dialog");
  const configuration = within(dialog).getByText("Model roles").parentElement;
  const list = within(dialog).getByTestId("compact-model-list");

  // The configuration block lives inside a bounded scroll area, not in the list.
  expect(configuration?.closest("[data-testid='model-configuration']")).toBeTruthy();
  expect(list.contains(configuration)).toBe(false);
  expect(within(dialog).getByTestId("model-row-omp-remembered-model")).toBeTruthy();

  fireEvent.click(within(dialog).getByText("Model roles"));
  expect(action).toHaveBeenCalledOnce();
});

it("keeps the configuration panel readable and scrollable at desktop widths", () => {
  const narrow = resolveModelConfigurationFrame({
    desktopMinWidth: 360,
    browserFixedHeight: 400,
    windowWidth: 700,
    windowHeight: 900,
  });
  // Readable role labels need roughly 620 CSS px regardless of the trigger width.
  expect(narrow.desktopMinWidth).toBe(620);
  // The list keeps its own bounded height; the configuration reserve sits below it.
  expect(narrow.desktopFixedHeight).toBe(640);

  // A clamped window still leaves the roles block usable instead of hiding it.
  const clamped = resolveModelConfigurationFrame({
    desktopMinWidth: 360,
    browserFixedHeight: 400,
    windowWidth: 560,
    windowHeight: 700,
  });
  expect(clamped.desktopMinWidth).toBe(528);
  expect(clamped.desktopMinWidth).toBeGreaterThan(360);
  expect(clamped.desktopFixedHeight).toBe(604);

  // Unmeasured layout (first frame, tests) must not collapse the panel.
  const unmeasured = resolveModelConfigurationFrame({
    desktopMinWidth: 360,
    browserFixedHeight: 240,
    windowWidth: 0,
    windowHeight: 0,
  });
  expect(unmeasured.desktopMinWidth).toBe(620);
  expect(unmeasured.desktopFixedHeight).toBe(480);
});
