/** @vitest-environment jsdom */
import React, { type ReactNode } from "react";
import { cleanup, fireEvent, render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type * as ProviderPresetData from "@/data/provider-presets";
import type * as OverlayRoot from "@/lib/overlay-root";
import { i18n } from "@/i18n/i18next";
import { DraftAgentControls } from "./index";

const open = vi.fn();
let currentLayer = 7;
let presetsSnapshot: unknown = null;

vi.mock("@/agent-profiles", () => ({
  useAgentProfilePicker: () => null,
  useAgentProfileEditor: () => ({ element: null }),
}));
vi.mock("./model-sheet", () => ({
  CompactModelSheet: ({ children }: { children: ReactNode }) => <>{children}</>,
}));
vi.mock("@/components/combined-model-selector", () => ({
  CombinedModelSelector: ({ modelConfiguration }: { modelConfiguration?: ReactNode }) => (
    <div data-testid="model-picker-configuration">{modelConfiguration}</div>
  ),
}));
vi.mock("@/components/ui/combobox", () => ({
  Combobox: () => null,
  ComboboxItem: () => null,
}));
vi.mock("@/hooks/use-shortcut-keys", () => ({ useShortcutKeys: () => [] }));
vi.mock("@/hooks/use-keyboard-action-handler", () => ({
  useKeyboardActionHandler: () => undefined,
}));
vi.mock("@/hooks/use-providers-snapshot", () => ({
  useProvidersSnapshot: () => ({ entries: [] }),
}));
vi.mock("@/hooks/use-form-preferences", () => ({
  useFormPreferences: () => ({ preferences: {}, updatePreferences: vi.fn() }),
}));
vi.mock("@/command-center/agent-control-registration", () => ({
  useAgentControlCommandCenterActions: () => undefined,
}));
// The empty state reads its own store; this suite supplies the answer directly.
vi.mock("@/data/provider-presets", async (importOriginal) => {
  const actual = await importOriginal<typeof ProviderPresetData>();
  return {
    ...actual,
    useProviderPresets: () =>
      presetsSnapshot === null
        ? { snapshot: null, isLoading: false, error: null }
        : { snapshot: presetsSnapshot, isLoading: false, error: null },
  };
});
vi.mock("@/lib/overlay-root", async (importOriginal) => {
  const actual = await importOriginal<typeof OverlayRoot>();
  return { ...actual, useCurrentOverlayLayer: () => currentLayer };
});
vi.mock("@/stores/provider-settings-store", () => ({
  useProviderSettingsStore: (selector: (state: { open: typeof open }) => unknown) =>
    selector({ open }),
}));

beforeEach(async () => {
  vi.stubGlobal("React", React);
  open.mockReset();
  currentLayer = 7;
  presetsSnapshot = null;
  await i18n.changeLanguage("en");
});
afterEach(async () => {
  cleanup();
  vi.unstubAllGlobals();
  await i18n.changeLanguage("en");
});

function emptyStore() {
  return {
    descriptor: {
      backend: "ait" as const,
      canCreate: true,
      canEdit: true,
      canDelete: true,
      canRename: true,
    },
    presets: [],
    error: null,
  };
}

function controls(overrides: Partial<React.ComponentProps<typeof DraftAgentControls>> = {}) {
  return (
    <DraftAgentControls
      selectedProvider="codex"
      providerDefinitions={[]}
      modeOptions={[]}
      selectedMode=""
      onSelectMode={vi.fn()}
      models={[{ provider: "codex", id: "gpt-6", label: "GPT-6" }]}
      selectedModel="gpt-6"
      onSelectModel={vi.fn()}
      isModelLoading={false}
      isAllModelsLoading={false}
      onSelectProviderAndModel={vi.fn()}
      onApplyAgentProfile={vi.fn()}
      modelSelectorProviders={[]}
      thinkingOptions={[]}
      selectedThinkingOptionId=""
      onSelectThinkingOption={vi.fn()}
      isCompactLayout={false}
      modelSelectorServerId="host-a"
      workingDir="/tmp/workspace"
      {...overrides}
    />
  );
}

describe("composer preset empty state", () => {
  it("points a provider with no presets at its editor, above the current overlay", () => {
    presetsSnapshot = emptyStore();
    const view = render(controls());

    const row = view.getByTestId("model-preset-empty-state");
    expect(row).toBeTruthy();
    fireEvent.click(view.getByTestId("model-preset-create"));
    expect(open).toHaveBeenCalledWith({
      serverId: "host-a",
      provider: "codex",
      overlayParentLayer: 7,
      cwd: "/tmp/workspace",
    });
    expect(open).toHaveBeenCalledOnce();
  });

  it("stays out of the way once the provider has presets", () => {
    presetsSnapshot = {
      ...emptyStore(),
      presets: [{ name: "team", model: "gpt-6" }],
    };
    const view = render(controls());

    expect(view.queryByTestId("model-preset-empty-state")).toBeNull();
    expect(view.queryByTestId("model-preset-create")).toBeNull();
  });

  it("scopes the editor it opens to the draft's working directory", () => {
    presetsSnapshot = emptyStore();
    const view = render(controls({ workingDir: "/tmp/other" }));

    fireEvent.click(view.getByTestId("model-preset-create"));
    expect(open).toHaveBeenCalledWith({
      serverId: "host-a",
      provider: "codex",
      overlayParentLayer: 7,
      // The editor must read the same store the composer just showed.
      cwd: "/tmp/other",
    });
  });

  it("renders nothing when there is no host to open", () => {
    presetsSnapshot = emptyStore();
    const view = render(controls({ modelSelectorServerId: null }));

    expect(view.queryByTestId("model-preset-empty-state")).toBeNull();
  });
});
