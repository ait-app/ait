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
              source: "global",
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
vi.mock("@/agent-controls/preset-context", () => ({
  useAgentPresetContext: () => ({ data: undefined, error: null }),
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
  "renders provider presets above default models for immediate access and keeps Profiles separate (%s)",
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
    const presetHeading = screen.getByText("modelSelector.modelPresets");
    const defaultHeading = screen.getByText("modelSelector.defaultModelHeading");
    const defaultModel = screen.getByText("Default large");
    expect(
      presetHeading.compareDocumentPosition(defaultHeading) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    expect(
      defaultHeading.compareDocumentPosition(defaultModel) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    expect(screen.getByText("modelSelector.profiles")).toBeTruthy();
    fireEvent.click(screen.getByTestId("provider-model-preset-omp-Fast preset"));
    expect(apply).toHaveBeenCalledExactlyOnceWith(
      "omp",
      {
        modelId: "fast",
        thinkingOptionId: null,
        featureValues: { "modelRole.worker": "fast" },
      },
      expect.objectContaining({ name: "Fast preset" }),
    );
    expect(applyProfile).not.toHaveBeenCalled();
  },
);

it("preserves the original model layout when no presets exist", () => {
  available.presets = false;
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
});

it("still renders presets when the provider has no role features", () => {
  available.roles = false;
  render(
    <ModelBrowser
      state={state}
      onSelect={vi.fn()}
      modelPresets={{ cwd: "/project", apply: vi.fn() }}
    />,
  );
  expect(screen.getByText("modelSelector.modelPresets")).toBeTruthy();
  expect(screen.getByText("modelSelector.defaultModelHeading")).toBeTruthy();
});

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

it("marks only the chosen preset even when its model matches the default row", () => {
  const onSelect = vi.fn();
  const sameModelState: ModelBrowserState = {
    ...state,
    selectedModel: "fast",
    providers: state.providers.map((provider) => ({
      ...provider,
      modelSelection: {
        kind: "models",
        rows: [
          {
            favoriteKey: "omp:fast",
            provider: "omp",
            providerLabel: "OMP",
            modelId: "fast",
            modelLabel: "Fast",
          },
        ],
      },
    })),
  };
  const controls = {
    cwd: "/project",
    selected: { provider: "omp", name: "Fast preset", source: "global" as const },
    apply: vi.fn(),
  };
  const { rerender } = render(
    <ModelBrowser state={sameModelState} onSelect={onSelect} modelPresets={controls} />,
  );
  expect(
    screen.getByTestId("provider-model-preset-omp-Fast preset").getAttribute("aria-selected"),
  ).toBe("true");
  expect(screen.getByTestId("model-row-omp-fast").getAttribute("aria-selected")).toBe("false");
  fireEvent.click(screen.getByTestId("model-row-omp-fast"));
  expect(onSelect).toHaveBeenCalledExactlyOnceWith("omp", "fast");
  rerender(
    <ModelBrowser
      state={sameModelState}
      onSelect={onSelect}
      modelPresets={{ ...controls, selected: null }}
    />,
  );
  expect(
    screen.getByTestId("provider-model-preset-omp-Fast preset").getAttribute("aria-selected"),
  ).toBe("false");
  expect(screen.getByTestId("model-row-omp-fast").getAttribute("aria-selected")).toBe("true");
});

it("does not expose an owner's preset identity or select a default row to a guest", () => {
  const apply = vi.fn();
  render(
    <ModelBrowser
      state={state}
      onSelect={vi.fn()}
      modelPresets={{ cwd: "/project", permissionDenied: true, presetActive: true, apply }}
    />,
  );
  expect(screen.getByText("modelSelector.presetOwnerOnly")).toBeTruthy();
  expect(screen.getByTestId("model-row-omp-large").getAttribute("aria-selected")).toBe("false");
  expect(screen.queryByTestId("provider-model-preset-omp-Fast preset")).toBeNull();
  expect(apply).not.toHaveBeenCalled();
});
