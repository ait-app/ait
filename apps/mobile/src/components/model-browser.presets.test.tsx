// @vitest-environment jsdom
import React, { type ReactNode } from "react";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { ModelBrowser, type ModelBrowserState } from "./model-browser";

const available = vi.hoisted(() => ({ presets: true, roles: true, unresolved: false }));
vi.mock("@/data/provider-presets", () => ({
  useProviderPresets: () => ({
    isSupported: true,
    error: null,
    snapshot: {
      presets: available.presets
        ? [
            {
              name: "Fast preset",
              model: available.unresolved ? "retired" : "fast",
              featureValues: { "modelRole.worker": "fast" },
            },
          ]
        : [],
    },
  }),
  useProviderPresetFeatures: () => ({
    data: available.roles
      ? [
          {
            type: "select",
            id: "modelRole.worker",
            label: "Worker",
            value: "native",
            options: [{ id: "native", label: "Inherit", isDefault: true }],
          },
        ]
      : [],
  }),
}));
vi.mock("@/lib/overlay-root", () => ({ useCurrentOverlayLayer: () => 0 }));
vi.mock("@/agent-profiles", () => ({ AgentProfileGlyph: () => null }));
vi.mock("@/constants/layout", () => ({ useIsCompactFormFactor: () => false }));
vi.mock("@/components/provider-icons", () => ({ getProviderIcon: () => () => null }));
vi.mock("@/components/ui/tooltip", () => ({
  Tooltip: ({ children }: { children: ReactNode }) => children,
  TooltipTrigger: ({ children }: { children: ReactNode }) => children,
  TooltipContent: () => null,
}));
vi.mock("@/components/ui/scroll-view", async () => {
  const native = await import("react-native");
  return { FlatList: native.FlatList, ScrollView: native.ScrollView };
});
vi.mock("react-native-gesture-handler", () => {
  const gesture: unknown = new Proxy({}, { get: () => () => gesture });
  return {
    Gesture: { Native: () => gesture, Tap: () => gesture },
    GestureDetector: ({ children }: { children: ReactNode }) => children,
  };
});
vi.mock("@/i18n/i18next", () => ({ i18n: { t: (key: string) => key } }));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
afterEach(() => {
  cleanup();
  available.presets = true;
  available.roles = true;
  available.unresolved = false;
});

const state: ModelBrowserState = {
  serverId: "host",
  providers: [
    {
      id: "omp",
      label: "OMP",
      modelDefinitions: [
        { provider: "omp", id: "large", label: "Default large" },
        { provider: "omp", id: "fast", label: "Fast" },
      ],
      modelSelection: {
        kind: "models",
        rows: [
          {
            favoriteKey: "omp:large",
            provider: "omp",
            providerLabel: "OMP",
            modelId: "large",
            modelLabel: "Default large",
          },
        ],
      },
    },
  ],
  selectedProvider: "omp",
  selectedModel: "large",
  profiles: {
    rows: [
      {
        id: "profile",
        provider: "omp",
        modelId: "large",
        name: "Existing profile",
        summary: "",
        icon: "",
        color: "",
      },
    ],
    applyProfile: vi.fn(),
  },
  view: { kind: "provider", providerId: "omp", providerLabel: "OMP" },
  searchQuery: "",
  isSearchFocused: false,
  header: { title: "OMP" },
  selectedModelLabel: "Default large",
  triggerLabel: "Default large",
  desktopFixedHeight: 400,
  isProviderView: true,
  prepareToOpen: vi.fn(),
  showAll: vi.fn(),
  reset: vi.fn(),
  drillDown: vi.fn(),
};

it.each(["sheet", "independent"] as const)(
  "renders actual provider presets below default models and keeps Profiles separate (%s)",
  (scrolling) => {
    const apply = vi.fn();
    const applyProfile = vi.fn();
    render(
      <ModelBrowser
        state={state}
        onSelect={vi.fn()}
        modelPresets={{ cwd: "/project", apply }}
        onApplyProfile={applyProfile}
        scrolling={scrolling}
      />,
    );
    const defaultHeading = screen.getByText("modelSelector.defaultModelHeading");
    const defaultModel = screen.getByText("Default large");
    const presetHeading = screen.getByText("modelSelector.modelPresets");
    expect(
      defaultHeading.compareDocumentPosition(defaultModel) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    expect(
      defaultModel.compareDocumentPosition(presetHeading) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    expect(screen.getByText("modelSelector.profiles")).toBeTruthy();
    fireEvent.click(screen.getByTestId("provider-model-preset-omp-Fast preset"));
    expect(apply).toHaveBeenCalledExactlyOnceWith("omp", {
      modelId: "fast",
      thinkingOptionId: null,
      featureValues: { "modelRole.worker": "fast" },
    });
    expect(applyProfile).not.toHaveBeenCalled();
  },
);

it.each(["presets", "roles"] as const)(
  "preserves the original model layout when %s are unavailable",
  (missing) => {
    available[missing] = false;
    render(
      <ModelBrowser
        state={state}
        onSelect={vi.fn()}
        modelPresets={{ cwd: "/project", apply: vi.fn() }}
      />,
    );
    expect(screen.queryByText("modelSelector.defaultModelHeading")).toBeNull();
    expect(screen.queryByText("modelSelector.modelPresets")).toBeNull();
    expect(screen.getByText("Default large")).toBeTruthy();
    expect(screen.getByText("modelSelector.profiles")).toBeTruthy();
  },
);

it("renders an unresolved model preset as disabled in the actual model browser row", () => {
  available.unresolved = true;
  const apply = vi.fn();
  render(
    <ModelBrowser state={state} onSelect={vi.fn()} modelPresets={{ cwd: "/project", apply }} />,
  );
  const row = screen.getByTestId("provider-model-preset-omp-Fast preset");
  expect(row.getAttribute("aria-disabled")).toBe("true");
  expect(row.textContent).toContain("providerSelection.unavailable");
  fireEvent.click(row);
  expect(apply).not.toHaveBeenCalled();
});
