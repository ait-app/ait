/** @vitest-environment jsdom */
import React, { type ComponentProps, type ReactNode } from "react";
import { act, cleanup, fireEvent, render, within } from "@testing-library/react";
import type { ComboboxProps } from "@/components/ui/combobox";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { i18n } from "@/i18n/i18next";
import { buildProviderDefinitions } from "@/utils/provider-definitions";
import { DraftAgentControls } from "./index";

vi.mock("@/agent-profiles", () => ({
  useAgentProfilePicker: () => null,
  useAgentProfileEditor: () => ({ element: null }),
}));

// Keep the actual draft, mode and feature controls; replace the surrounding model browser.
vi.mock("./model-sheet", () => ({
  CompactModelSheet: ({
    children,
    providerSettings,
  }: {
    children: ReactNode;
    providerSettings?: ReactNode;
  }) => {
    const [open, setOpen] = React.useState(false);
    return (
      <>
        {children}
        <button onClick={() => setOpen(true)}>Provider settings</button>
        {open ? providerSettings : null}
      </>
    );
  },
}));
vi.mock("@/components/combined-model-selector", () => ({
  CombinedModelSelector: ({ providerSettings }: { providerSettings?: ReactNode }) => {
    const [open, setOpen] = React.useState(false);
    return (
      <>
        <button onClick={() => setOpen(true)}>Provider settings</button>
        {open ? providerSettings : null}
      </>
    );
  },
}));
vi.mock("@/components/ui/combobox", () => ({
  Combobox: ({
    open,
    options,
    value,
    onSelect,
  }: Pick<ComboboxProps, "open" | "options" | "value" | "onSelect">) =>
    open ? (
      <select
        aria-label="Role model"
        value={value}
        onChange={(event) => onSelect(event.target.value)}
      >
        {options.map((option) => (
          <option key={option.id} value={option.id}>
            {option.label}
          </option>
        ))}
      </select>
    ) : null,
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

vi.mock("lucide-react-native", async (importOriginal) => {
  const actual = await importOriginal<typeof import("lucide-react-native")>();
  return {
    ...actual,
    Minimize2: () => <span data-testid="icon-compaction" />,
    RotateCcw: () => <span data-testid="icon-retry" />,
    Zap: () => <span data-testid="icon-fast" />,
    Turtle: () => <span data-testid="icon-slow" />,
    ShieldOff: () => <span data-testid="icon-full-access" />,
  };
});

beforeEach(async () => {
  vi.stubGlobal("React", React);
  await i18n.changeLanguage("zh-CN");
});
afterEach(async () => {
  cleanup();
  await i18n.changeLanguage("en");
});

describe("OMP model settings presentation", () => {
  it.each([true, false])(
    "groups native roles in model settings and preserves independent edits (compact: %s)",
    (isCompactLayout) => {
      const onSetFeature = vi.fn();
      const onModelSelectorOpen = vi.fn();
      const features = [
        "default",
        "smol",
        "vision",
        "advisor",
        "plan",
        "commit",
        "slow",
        "tiny",
        "memory",
        "task",
        "widget",
        "image",
        "web",
        "speech",
        "dictation",
        "judge",
      ].map((role) => ({
        type: "select" as const,
        id: `modelRole.${role}`,
        label: `${role} model`,
        value: "native",
        options: [
          { id: "native", label: "Use OMP setting", isDefault: true },
          { id: "test/model", label: "Fixture model" },
        ],
      }));
      const view = render(
        <DraftAgentControls
          selectedProvider="omp"
          providerDefinitions={[]}
          modeOptions={[]}
          selectedMode=""
          onSelectMode={vi.fn()}
          models={[{ provider: "omp", id: "main", label: "Main" }]}
          selectedModel="main"
          onSelectModel={vi.fn()}
          isModelLoading={false}
          isAllModelsLoading={false}
          onSelectProviderAndModel={vi.fn()}
          onApplyAgentProfile={vi.fn()}
          modelSelectorProviders={[]}
          isCompactLayout={isCompactLayout}
          thinkingOptions={[]}
          selectedThinkingOptionId=""
          onSelectThinkingOption={vi.fn()}
          features={[
            {
              id: "modelPreset",
              type: "select",
              label: "Preset",
              value: "__custom__",
              options: [
                { id: "__custom__", label: "Custom" },
                {
                  id: "work",
                  label: "Work",
                  metadata: {
                    configuration: { modelId: "main" },
                  },
                },
              ],
            },
            ...features,
          ]}
          onSetFeature={onSetFeature}
          onModelSelectorOpen={onModelSelectorOpen}
        />,
      );
      expect(view.queryByTestId("agent-feature-modelPreset")).toBeNull();
      expect(view.queryByTestId("model-role-settings")).toBeNull();
      fireEvent.click(view.getByRole("button", { name: "Provider settings" }));
      expect(view.getAllByTestId("agent-feature-modelPreset")).toHaveLength(1);
      expect(view.queryByTestId("agent-controls-features")).toBeNull();
      expect(view.queryByTestId("agent-feature-modelRole.default")).toBeNull();
      fireEvent.click(view.getByTestId("model-role-settings"));
      for (const role of ["default", "smol", "vision", "advisor"]) {
        fireEvent.click(view.getByTestId(`agent-feature-modelRole.${role}`));
        fireEvent.change(view.getByRole("combobox", { name: "Role model" }), {
          target: { value: "test/model" },
        });
        expect(onSetFeature).toHaveBeenLastCalledWith(`modelRole.${role}`, "test/model");
        fireEvent.change(view.getByRole("combobox", { name: "Role model" }), {
          target: { value: "native" },
        });
        expect(onSetFeature).toHaveBeenLastCalledWith(`modelRole.${role}`, "native");
      }
      for (const feature of features)
        expect(view.getByTestId(`agent-feature-${feature.id}`)).toBeTruthy();
      expect(onSetFeature).toHaveBeenCalledTimes(8);
      // Opening model roles and each existing role select starts background access work.
      expect(onModelSelectorOpen).toHaveBeenCalledTimes(13);
    },
  );

  it("renders localized labels and semantic icons, then updates the existing controls on language change", async () => {
    const modes = [{ id: "full", label: "Full Access" }];
    const features = [
      {
        id: "autoCompaction",
        label: "Automatic compaction",
        value: true,
      },
      { id: "autoRetry", label: "Automatic retry", value: true },
      { id: "fastMode", label: "Fast service tier", value: false },
      { id: "slowMode", label: "Slow service tier", value: false },
    ].map((feature) => ({ ...feature, type: "toggle" as const }));
    const props: ComponentProps<typeof DraftAgentControls> = {
      selectedProvider: "omp",
      providerDefinitions: buildProviderDefinitions([
        { provider: "omp", status: "ready", enabled: true, modes },
      ]),
      modeOptions: modes,
      selectedMode: "full",
      onSelectMode: vi.fn(),
      models: [{ provider: "omp", id: "model", label: "DeepSeek" }],
      selectedModel: "model",
      onSelectModel: vi.fn(),
      isModelLoading: false,
      isAllModelsLoading: false,
      onSelectProviderAndModel: vi.fn(),
      onApplyAgentProfile: vi.fn(),
      modelSelectorProviders: [],
      isCompactLayout: true,
      thinkingOptions: [
        { id: "off", label: "off" },
        { id: "max", label: "max" },
      ],
      selectedThinkingOptionId: "max",
      onSelectThinkingOption: vi.fn(),
      features,
      onSetFeature: vi.fn(),
    };
    const view = render(<DraftAgentControls {...props} />);
    for (const [id, label, icon] of [
      ["autoCompaction", "自动压缩上下文", "compaction"],
      ["autoRetry", "自动重试", "retry"],
      ["fastMode", "快速服务档位", "fast"],
      ["slowMode", "慢速服务档位", "slow"],
    ]) {
      const row = within(view.getByTestId(`agent-feature-${id}`));
      expect(row.getByText(label)).toBeTruthy();
      expect(row.getByTestId(`icon-${icon}`)).toBeTruthy();
    }
    expect(view.getByRole("button", { name: "选择推理强度（最高）" })).toBeTruthy();
    expect(view.getByRole("button", { name: "选择智能体模式（完全访问）" })).toBeTruthy();
    expect(view.getByTestId("icon-full-access")).toBeTruthy();
    await act(async () => {
      await i18n.changeLanguage("en");
    });
    expect(view.getByText("Automatic compaction")).toBeTruthy();
    expect(view.getByRole("button", { name: "Select thinking option (Max)" })).toBeTruthy();
    expect(
      view.getByRole("button", {
        name: "Select agent mode (Full access)",
      }),
    ).toBeTruthy();
    expect(view.queryByText("自动压缩上下文")).toBeNull();
    expect(features[0].label).toBe("Automatic compaction");
  });
});
