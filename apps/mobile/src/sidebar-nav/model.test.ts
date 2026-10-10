import { describe, expect, it } from "vitest";
import { moveSidebarNavItem, resolveSidebarNavItems, setSidebarNavItemVisible } from "./model";

describe("built-in sidebar navigation", () => {
  it("drops removed plugin entries and preserves the order and visibility of built-ins", () => {
    expect(
      resolveSidebarNavItems({
        preferences: [
          { key: "plugin:notes:inbox", visible: true },
          { key: "search", visible: false },
          { key: "history", visible: true },
        ],
      }).map(({ key, visible }) => ({ key, visible })),
    ).toEqual([
      { key: "search", visible: false },
      { key: "history", visible: true },
      { key: "new-workspace", visible: true },
      { key: "schedules", visible: true },
      { key: "usage", visible: true },
    ]);
  });
  it("updates visibility and reorders built-ins", () => {
    const items = resolveSidebarNavItems({ preferences: [] });
    const preferences = setSidebarNavItemVisible({
      items,
      key: "search",
      visible: false,
      previous: [],
    });
    const moved = moveSidebarNavItem({
      items: resolveSidebarNavItems({ preferences }),
      key: "search",
      direction: "up",
      previous: preferences,
    });
    expect(moved.map(({ key }) => key)).toEqual([
      "new-workspace",
      "search",
      "history",
      "schedules",
      "usage",
    ]);
    expect(moved.find(({ key }) => key === "search")?.visible).toBe(false);
  });
});
