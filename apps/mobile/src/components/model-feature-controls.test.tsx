// @vitest-environment jsdom
import React from "react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import type { AgentFeature } from "@ait/protocol/agent-types";
import { ModelFeatureControls } from "./model-feature-controls";

vi.mock("react-native", () => ({
  View: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
}));
vi.mock("react-native-unistyles", () => ({
  StyleSheet: {
    create: (factory: (theme: { colors: Record<string, string> }) => unknown) =>
      factory({ colors: {} }),
  },
}));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));
// The empty preset row is covered by its own suite; this one only groups features.
vi.mock("@/components/model-preset-empty-state", () => ({
  ModelPresetEmptyState: () => <div data-testid="model-preset-empty-state" />,
}));
vi.mock("@/components/ui/button", () => ({
  Button: ({
    children,
    onPress,
    disabled,
    accessibilityState,
  }: {
    children: React.ReactNode;
    onPress: () => void;
    disabled?: boolean;
    accessibilityState?: { expanded?: boolean };
  }) => (
    <button disabled={disabled} onClick={onPress} aria-expanded={accessibilityState?.expanded}>
      {children}
    </button>
  ),
}));

beforeEach(() => vi.stubGlobal("React", React));
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});
const preset: AgentFeature = {
  id: "modelPreset",
  type: "select",
  label: "Preset",
  value: "work",
  options: [
    {
      id: "work",
      label: "Work",
      metadata: {
        configuration: {
          modelId: "real/model",
          featureValues: { "modelRole.default": "real/model" },
        },
      },
    },
  ],
};
const role: AgentFeature = {
  id: "modelRole.custom",
  type: "select",
  label: "Custom model",
  value: "saved/model",
  options: [],
};

it("keeps roles out of the initial picker view and forwards the original preset configuration untouched", () => {
  const access = vi.fn();
  const renderFeature = vi.fn((feature: AgentFeature) => (
    <span key={feature.id}>
      {feature.label}:{String(feature.value)}
    </span>
  ));
  const view = render(
    <ModelFeatureControls
      features={[preset, role]}
      onCatalogAccess={access}
      renderFeature={renderFeature}
    />,
  );
  expect(screen.getByText("Preset:work")).toBeTruthy();
  expect(screen.queryByText("Custom model:saved/model")).toBeNull();
  expect(renderFeature).toHaveBeenCalledWith(preset, 0);
  fireEvent.click(screen.getByRole("button", { name: "providerControl.modelRoles" }));
  expect(screen.getByText("Custom model:saved/model")).toBeTruthy();
  expect(access).toHaveBeenCalledTimes(1);
  const refreshed = {
    ...role,
    value: "new/model",
    options: [{ id: "new/model", label: "New model" }],
  };
  view.rerender(
    <ModelFeatureControls
      features={[preset, refreshed]}
      onCatalogAccess={access}
      renderFeature={renderFeature}
    />,
  );
  expect(screen.getByText("Custom model:new/model")).toBeTruthy();
  expect(preset.options[0].metadata?.configuration).toEqual({
    modelId: "real/model",
    featureValues: { "modelRole.default": "real/model" },
  });
});

it("does not expose a second entry when there are no model features and respects disabled editing", () => {
  const renderer = vi.fn(() => null);
  const view = render(<ModelFeatureControls features={[]} renderFeature={renderer} />);
  expect(view.container.textContent).toBe("");
  view.rerender(<ModelFeatureControls features={[role]} disabled renderFeature={renderer} />);
  const button = screen.getByRole("button", {
    name: "providerControl.modelRoles",
  });
  expect((button as HTMLButtonElement).disabled).toBe(true);
  fireEvent.click(button);
  expect(renderer).not.toHaveBeenCalled();
});
