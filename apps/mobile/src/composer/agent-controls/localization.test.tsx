/** @vitest-environment jsdom */
import React, { type ComponentProps, type ReactNode } from "react";
import { act, cleanup, render, within } from "@testing-library/react";
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
  CompactModelSheet: ({ children }: { children: ReactNode }) => <>{children}</>,
}));
vi.mock("@/components/combined-model-selector", () => ({ CombinedModelSelector: () => null }));
vi.mock("@/components/ui/combobox", () => ({ Combobox: () => null, ComboboxItem: () => null }));
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
  it("renders localized labels and semantic icons, then updates the existing controls on language change", async () => {
    const modes = [{ id: "full", label: "Full Access" }];
    const features = [
      { id: "autoCompaction", label: "Automatic compaction", value: true },
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
    expect(view.getByRole("button", { name: "Select agent mode (Full access)" })).toBeTruthy();
    expect(view.queryByText("自动压缩上下文")).toBeNull();
    expect(features[0].label).toBe("Automatic compaction");
  });
});
