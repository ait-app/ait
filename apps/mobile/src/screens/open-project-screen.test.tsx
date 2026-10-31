// @vitest-environment jsdom
import React from "react";
import { cleanup, fireEvent, render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { OpenProjectScreen } from "./open-project-screen";

const mocks = vi.hoisted(() => ({ openUrl: vi.fn(async () => {}), openSidebar: vi.fn() }));
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
vi.mock("@/hooks/use-import-session", () => ({
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

beforeEach(() => vi.clearAllMocks());
afterEach(cleanup);

it("opens the login portal from the home screen without a local daemon", () => {
  const view = render(<OpenProjectScreen />);
  expect(view.queryByTestId("open-project-pair-device")).toBeNull();
  expect(view.getByText("openProject.tiles.signIn.title")).toBeTruthy();
  fireEvent.click(view.getByTestId("open-project-sign-in"));
  expect(mocks.openUrl).toHaveBeenCalledExactlyOnceWith("https://ait-login.authing.cn/u");
});

it("shows login portal failures and clears the error on retry", async () => {
  mocks.openUrl.mockRejectedValueOnce(new Error("Unable to open browser"));
  const view = render(<OpenProjectScreen />);
  fireEvent.click(view.getByTestId("open-project-sign-in"));
  await waitFor(() =>
    expect(view.getByRole("alert").textContent).toBe("onlineService.loginPortalOpenFailed"),
  );
  fireEvent.click(view.getByTestId("open-project-sign-in"));
  await waitFor(() => expect(view.queryByRole("alert")).toBeNull());
  expect(mocks.openUrl).toHaveBeenCalledTimes(2);
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
