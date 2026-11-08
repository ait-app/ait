// @vitest-environment jsdom
import React from "react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import type { ProviderPresetDescriptor } from "@ait/protocol/messages";
import { ModelPresetEmptyState } from "./model-preset-empty-state";

const open = vi.fn();
let currentLayer = 40;

vi.mock("react-native", () => ({
  Text: ({ children, testID }: { children: React.ReactNode; testID?: string }) => (
    <span data-testid={testID}>{children}</span>
  ),
  View: ({ children, testID }: { children: React.ReactNode; testID?: string }) => (
    <div data-testid={testID}>{children}</div>
  ),
}));
vi.mock("react-native-unistyles", () => ({
  StyleSheet: {
    create: (factory: (theme: unknown) => unknown) =>
      factory({
        colors: { foregroundMuted: "#888" },
        fontSize: { sm: 12 },
        spacing: { 2: 8 },
      }),
  },
}));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));
vi.mock("@/runtime/host-runtime", () => ({
  useHostRuntimeClient: () => null,
  useHostRuntimeIsConnected: () => false,
}));
// The store query only runs when the caller passes no snapshot; these cases always pass one.
vi.mock("@/data/provider-presets", () => ({
  presetControlAvailable: (store: unknown) => store !== null && store !== undefined,
  useProviderPresets: () => ({ snapshot: null, isLoading: false, error: null }),
}));
vi.mock("@/lib/overlay-root", () => ({
  useCurrentOverlayLayer: () => currentLayer,
}));
vi.mock("@/stores/provider-settings-store", () => ({
  useProviderSettingsStore: (selector: (state: { open: typeof open }) => unknown) =>
    selector({ open }),
}));
vi.mock("@/components/ui/button", () => ({
  Button: ({
    children,
    onPress,
    leftIcon: _leftIcon,
    ...rest
  }: {
    children?: React.ReactNode;
    onPress: () => void;
    leftIcon?: unknown;
  }) => (
    <button onClick={onPress} data-testid={(rest as { testID?: string }).testID}>
      {children}
    </button>
  ),
}));

beforeEach(() => {
  vi.stubGlobal("React", React);
  open.mockReset();
});
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

function descriptor(backend: "native" | "ait"): ProviderPresetDescriptor {
  return {
    backend,
    canCreate: true,
    canEdit: true,
    canDelete: true,
    canRename: backend === "ait",
  };
}

it("offers the editor entry when the provider has no presets and opens its settings above the picker", () => {
  render(
    <ModelPresetEmptyState
      serverId="host-a"
      provider="claude"
      cwd="/workspace"
      presets={{ descriptor: descriptor("ait"), presets: [] }}
    />,
  );
  expect(screen.getByTestId("model-preset-empty-state")).toBeTruthy();
  expect(screen.getByText("providerControl.presetEmptyManaged")).toBeTruthy();
  fireEvent.click(screen.getByTestId("model-preset-create"));
  expect(open).toHaveBeenCalledWith({
    serverId: "host-a",
    provider: "claude",
    // The sheet must paint above the picker that launched it.
    overlayParentLayer: 40,
    // And it must read the same directory this row read.
    cwd: "/workspace",
  });
});

it("points a native store at its own preset ability but still opens the same editor", () => {
  render(
    <ModelPresetEmptyState
      serverId="host-a"
      provider="omp"
      cwd="/workspace"
      presets={{ descriptor: descriptor("native"), presets: [] }}
    />,
  );
  expect(screen.getByText("providerControl.presetEmptyNative")).toBeTruthy();
  fireEvent.click(screen.getByTestId("model-preset-create"));
  expect(open).toHaveBeenCalledWith({
    serverId: "host-a",
    provider: "omp",
    overlayParentLayer: 40,
    cwd: "/workspace",
  });
});

it("shows nothing once a preset exists", () => {
  render(
    <ModelPresetEmptyState
      serverId="host-a"
      provider="claude"
      cwd="/workspace"
      presets={{
        descriptor: descriptor("ait"),
        presets: [{ name: "team", model: "opus" }],
      }}
    />,
  );
  expect(screen.queryByTestId("model-preset-empty-state")).toBeNull();
});

it("shows nothing when the host does not answer the preset request", () => {
  // No store snapshot and no client: the hook resolves to null, so no dead button appears.
  render(<ModelPresetEmptyState serverId={null} provider={null} cwd={null} presets={null} />);
  expect(screen.queryByTestId("model-preset-empty-state")).toBeNull();
  expect(open).not.toHaveBeenCalled();
});

it("opens the provider editor from a profile surface, where no project directory exists", () => {
  // Profiles carry no cwd; the sheet must still open for that host and provider.
  currentLayer = 40;
  render(
    <ModelPresetEmptyState
      serverId="host-a"
      provider="claude"
      cwd={null}
      presets={{ descriptor: descriptor("ait"), presets: [] }}
    />,
  );
  fireEvent.click(screen.getByTestId("model-preset-create"));
  expect(open).toHaveBeenCalledWith({
    serverId: "host-a",
    provider: "claude",
    overlayParentLayer: 40,
    cwd: null,
  });
});
