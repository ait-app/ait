// @vitest-environment jsdom
import { cleanup, render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { i18n } from "@/i18n/i18next";
import type { BuiltinSidebarNavId, SidebarNavItem } from "@/sidebar-nav/model";
import { SidebarNavRows } from "./sidebar-nav-rows";

const state = vi.hoisted(() => ({
  items: [] as { kind: "builtin"; key: string; id: string; visible: boolean }[],
  hasAnyShares: false,
}));

vi.mock("@/sidebar-nav/use-sidebar-nav-items", () => ({
  useSidebarNavItems: () => ({ items: state.items as SidebarNavItem[] }),
}));
vi.mock("@/sharing/share-lists", () => ({ useHasAnyShares: () => state.hasAnyShares }));
vi.mock("expo-router", () => ({ router: { push: vi.fn() }, usePathname: () => "/" }));
vi.mock("@/usage/sidebar-item", () => ({
  useIsUsageSettingsActive: () => false,
  useOpenUsageSettings: () => () => {},
}));
vi.mock("@/task-board/sidebar-task-counts", () => ({ SidebarTaskCounts: () => null }));
vi.mock("@react-native-async-storage/async-storage", () => ({
  default: { getItem: async () => null, setItem: async () => {}, removeItem: async () => {} },
}));

function show(...entries: [BuiltinSidebarNavId, boolean][]) {
  state.items = entries.map(([id, visible]) => ({ kind: "builtin", key: id, id, visible }));
}

beforeEach(async () => {
  state.hasAnyShares = false;
  await i18n.changeLanguage("en");
});
afterEach(cleanup);

describe("SidebarNavRows", () => {
  it("leaves the Shared row out while both share lists are empty", () => {
    show(["history", true], ["shared", true]);
    const view = render(<SidebarNavRows />);

    expect(view.getByTestId("sidebar-sessions")).toBeTruthy();
    expect(view.queryByTestId("sidebar-shared-with-me")).toBeNull();
  });

  it("shows the Shared row once either share list has an entry", () => {
    show(["history", true], ["shared", true]);
    state.hasAnyShares = true;
    const view = render(<SidebarNavRows />);

    expect(view.getByTestId("sidebar-shared-with-me")).toBeTruthy();
  });

  it("keeps the Shared row hidden when the user turned it off", () => {
    show(["history", true], ["shared", false]);
    state.hasAnyShares = true;
    const view = render(<SidebarNavRows />);

    expect(view.queryByTestId("sidebar-shared-with-me")).toBeNull();
  });

  it("renders no group when the Shared row was the only one and has nothing to show", () => {
    show(["history", false], ["shared", true]);
    const view = render(<SidebarNavRows />);

    expect(view.container.childElementCount).toBe(0);
  });
});
