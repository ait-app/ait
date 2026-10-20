// @vitest-environment jsdom
import { act, type ComponentProps, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { Bot } from "lucide-react-native";
import { CompactModelSheet } from "./model-sheet";

const fixture = vi.hoisted(() => ({
  rowIcon: null as unknown,
  providerIcon: () => null,
}));
vi.mock("react-i18next", async (importOriginal) => ({
  ...(await importOriginal<typeof import("react-i18next")>()),
  useTranslation: () => ({ t: (key: string) => key }),
}));
vi.mock("@/constants/layout", () => ({ useIsCompactFormFactor: () => true }));
vi.mock("@/components/provider-icons", () => ({ getProviderIcon: () => fixture.providerIcon }));
vi.mock("@/components/adaptive-modal-sheet", () => ({
  AdaptiveModalSheet: ({ children }: { children: ReactNode }) => children,
}));
vi.mock("@/components/model-browser", () => ({
  useModelBrowser: () => ({
    selectedModelLabel: "Select model",
    triggerLabel: "Select model",
    header: {},
  }),
  ModelBrowser: ({ rootBrowseContent }: { rootBrowseContent?: ReactNode }) => rootBrowseContent,
}));
vi.mock("./control", () => ({
  AgentControlTrigger: ({ icon }: { icon: unknown }) => {
    fixture.rowIcon = icon;
    return null;
  },
}));

beforeEach(() => vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true));
afterEach(() => vi.unstubAllGlobals());

function renderModelRow(props: Partial<ComponentProps<typeof CompactModelSheet>>) {
  const root = createRoot(document.createElement("div"));
  try {
    act(() =>
      root.render(
        <CompactModelSheet
          providers={[]}
          selectedProvider="opencode"
          selectedModel=""
          thinkingLabel={null}
          onSelect={vi.fn()}
          isLoading={false}
          glyphSize={16}
          canSwitchProvider
          {...props}
        >
          {null}
        </CompactModelSheet>,
      ),
    );
    return fixture.rowIcon;
  } finally {
    act(() => root.unmount());
  }
}

it("uses a neutral model entry when a new session inherits the previous provider", () => {
  expect(renderModelRow({})).toBe(Bot);
  expect(renderModelRow({ selectedModel: "previous-model" })).toBe(Bot);
});

it("uses the selected provider for an existing session with a fixed provider", () => {
  expect(renderModelRow({ canSwitchProvider: false, selectedModel: "current-model" })).toBe(
    fixture.providerIcon,
  );
});
