// @vitest-environment jsdom
import React from "react";
import { cleanup, fireEvent, render } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { OpenProjectScreen } from "./open-project-screen";

const mocks = vi.hoisted(() => ({
  openUrl: vi.fn(async () => {}),
  openSidebar: vi.fn(),
  push: vi.fn(),
  accountRelay: true,
}));
vi.mock("expo-router", () => ({ useRouter: () => ({ push: mocks.push }) }));
vi.mock("@/runtime/account-state", () => ({ supportsAccountRelay: () => mocks.accountRelay }));
vi.mock("@/runtime/host-runtime", () => ({
  useHosts: () => [],
  useHostRuntimeLastError: () => null,
}));
vi.mock("@/constants/layout", () => ({
  getIsElectronRuntime: () => true,
  useIsCompactFormFactor: () => false,
  HEADER_INNER_HEIGHT: 40,
  HEADER_INNER_HEIGHT_MOBILE: 40,
  HEADER_TOP_PADDING_MOBILE: 0,
}));
vi.mock("@/constants/platform", () => ({ isWeb: true, isNative: false }));
vi.mock("@/desktop/host", () => ({
  getDesktopHost: () => ({ opener: { openUrl: mocks.openUrl } }),
}));
vi.mock("@/stores/panel-store", () => ({
  usePanelStore: (select: (state: { openDesktopAgentList: () => void }) => unknown) =>
    select({ openDesktopAgentList: mocks.openSidebar }),
}));
vi.mock("@/hooks/use-open-add-project", () => ({ useOpenAddProject: () => vi.fn() }));
vi.mock("@/components/use-import-session", () => ({
  useImportSession: () => ({ open: vi.fn(), sheet: null }),
}));
vi.mock("@/hosts/host-chooser", () => ({ useHostChooser: () => vi.fn() }));
vi.mock("@/components/headers/menu-header", () => ({ MenuHeader: () => null }));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock("lucide-react-native", () => ({
  FolderOpen: () => null,
  Inbox: () => null,
  Plug: () => null,
  LogIn: () => null,
  Mail: () => null,
  Globe: () => null,
}));

beforeEach(() => {
  vi.clearAllMocks();
  mocks.accountRelay = true;
});
afterEach(cleanup);

it("opens the online service settings from the home screen instead of a website", () => {
  const view = render(<OpenProjectScreen />);
  expect(view.queryByTestId("open-project-pair-device")).toBeNull();
  expect(view.getByText("openProject.tiles.signIn.title")).toBeTruthy();
  fireEvent.click(view.getByTestId("open-project-sign-in"));
  expect(mocks.push).toHaveBeenCalledExactlyOnceWith("/settings/online-service");
  expect(mocks.openUrl).not.toHaveBeenCalled();
});

it("hides the sign-in tile where the client has no online service account", () => {
  mocks.accountRelay = false;
  const view = render(<OpenProjectScreen />);
  expect(view.queryByTestId("open-project-sign-in")).toBeNull();
});

it.each([
  ["Star", "https://github.com/ait-app/ait"],
  ["Email", "mailto:dong@necoex.com"],
  ["Homepage", "https://ait-app.com"],
])("keeps %s outside Electron's mouse-drag behavior and opens its URL", (label, url) => {
  const view = render(<OpenProjectScreen />);
  const button = view.getByRole("button", { name: label });
  // index.html excludes controls from native dragging only inside a drag-region
  // owner. An absolutely positioned footer sibling otherwise receives no exclusion.
  const dragRegions = view.container.querySelectorAll("[data-titlebar-drag-region]");
  expect([...dragRegions].some((region) => region.parentElement?.contains(button))).toBe(true);
  fireEvent.click(button);
  expect(mocks.openUrl).toHaveBeenCalledExactlyOnceWith(url);
});
