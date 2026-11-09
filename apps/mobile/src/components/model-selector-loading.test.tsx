/** @vitest-environment jsdom */
import React, { type ComponentProps } from "react";
import { cleanup, fireEvent, render, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useProviderSettingsStore } from "@/stores/provider-settings-store";
import { i18n } from "@/i18n/i18next";
import { buildSelectableProviderSelectorProviders } from "@/provider-selection/provider-selection";
import { ProviderSettingsAction } from "./model-browser";
import { CompactModelSheet } from "@/composer/agent-controls/model-sheet";
import { CombinedModelSelector } from "./combined-model-selector";
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
    header,
  }: ComponentProps<typeof Combobox>) =>
    open ? (
      <div role="dialog" data-min-width={desktopMinWidth} data-fixed-height={desktopFixedHeight}>
        {header?.actions}
        {children}
      </div>
    ) : null,
}));
vi.mock("@/components/adaptive-modal-sheet", () => ({
  AdaptiveModalSheet: ({
    visible,
    children,
    testID,
    header,
  }: ComponentProps<typeof AdaptiveModalSheet>) =>
    visible ? (
      <div data-testid={testID}>
        {header?.actions}
        {children}
      </div>
    ) : null,
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

describe.each(["combined", "compact"] as const)("%s provider settings entry", (surface) => {
  it("opens the gear with host, provider and project while keeping internal controls out of the picker", () => {
    layout.compact = surface === "compact";
    const props = {
      providers: providers(true),
      selectedProvider: "omp",
      selectedModel: "remembered-model",
      isLoading: false,
      onSelect: vi.fn(),
      serverId: "host-a",
      cwd: "/project/a",
      providerSettings: <span>Preset and role settings</span>,
    };
    const view = render(
      surface === "compact" ? (
        <CompactModelSheet {...props} thinkingLabel={null} glyphSize={20} canSwitchProvider>
          {null}
        </CompactModelSheet>
      ) : (
        <CombinedModelSelector {...props} />
      ),
    );
    fireEvent.click(view.getByTestId("combined-model-selector"));
    expect(view.queryByText("Preset and role settings")).toBeNull();
    fireEvent.click(view.getByTestId("selector-header-settings-omp"));
    expect(useProviderSettingsStore.getState()).toMatchObject({
      serverId: "host-a",
      provider: "omp",
      cwd: "/project/a",
      visible: true,
    });
    expect(useProviderSettingsStore.getState().configuration).toEqual(props.providerSettings);
  });
});

it("keeps settings controls current and closes their source when the project changes", () => {
  const view = render(
    <ProviderSettingsAction
      serverId="host"
      provider="omp"
      cwd="/a"
      accessibilityLabel="Settings"
      configuration={<span>Before</span>}
    />,
  );
  fireEvent.click(view.getByTestId("selector-header-settings-omp"));
  const sourceId = useProviderSettingsStore.getState().sourceId!;
  view.rerender(
    <ProviderSettingsAction
      serverId="host"
      provider="omp"
      cwd="/a"
      accessibilityLabel="Settings"
      configuration={<span>After</span>}
    />,
  );
  expect(useProviderSettingsStore.getState().configuration).toEqual(<span>After</span>);
  useProviderSettingsStore.getState().updateConfiguration("another-source", <span>Wrong</span>);
  expect(useProviderSettingsStore.getState().configuration).toEqual(<span>After</span>);
  view.rerender(
    <ProviderSettingsAction
      serverId="host"
      provider="omp"
      cwd="/b"
      accessibilityLabel="Settings"
      configuration={<span>Other project</span>}
    />,
  );
  expect(useProviderSettingsStore.getState().visible).toBe(false);
  useProviderSettingsStore.getState().updateConfiguration(sourceId, <span>Late</span>);
  expect(useProviderSettingsStore.getState().configuration).toBeNull();
});
