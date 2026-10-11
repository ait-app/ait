import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { UsageCard } from "./card";
import { i18n } from "@/i18n/i18next";
import type { UsageDisplay } from "./display";
import type { UsageReportEntry } from "./types";

const refresh = vi.fn();
vi.mock("@/constants/platform", () => ({ isNative: false, isWeb: true }));
vi.mock("@/constants/layout", () => ({
  useIsCompactFormFactor: () => false,
  getIsElectronRuntimeMac: () => false,
  WORKSPACE_SECONDARY_HEADER_HEIGHT: 36,
}));
vi.mock("@/components/ui/dropdown-menu", () => ({ DropdownMenuTrigger: () => null }));
vi.mock("./queries", () => ({ useReportRefresh: () => ({ refresh, refreshState: "idle" }) }));
vi.mock("@/components/provider-icons", () => ({ getProviderIcon: () => () => null }));
vi.mock("@/hooks/use-compact-time-ago", () => ({ useCompactTimeAgo: () => "2m" }));

let root: Root | null = null;
let container: HTMLDivElement | null = null;
const togglePin = vi.fn();
const display: UsageDisplay = {
  displayAs: "used",
  setDisplayAs: vi.fn(),
  isPinned: () => false,
  togglePin,
};
const entry: UsageReportEntry = {
  id: "codex",
  sourceId: "codex",
  sourceLabel: "Codex",
  account: { label: "selected@example.test" },
  fetchedAt: "2026-10-10T00:00:00Z",
  report: {
    status: "available",
    planLabel: "Plus",
    windows: [{ id: "primary", label: "Session", usedPct: 25, remainingPct: 75 }],
  },
};
beforeEach(async () => {
  await i18n.changeLanguage("en");
  vi.stubGlobal("React", React);
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
});
function render(report = entry, mode: UsageDisplay = display) {
  if (!container) {
    container = document.createElement("div");
    document.body.append(container);
    root = createRoot(container);
  }
  act(() =>
    root?.render(<UsageCard serverId="host" entry={report} display={mode} pinnable refreshable />),
  );
  return container;
}
afterEach(async () => {
  act(() => root?.unmount());
  container?.remove();
  root = null;
  container = null;
  vi.clearAllMocks();
  vi.unstubAllGlobals();
  await i18n.changeLanguage("en");
});

describe("native usage card in Chromium", () => {
  it("updates displayed percentages and login messages when the language changes", async () => {
    const node = render(entry, { ...display, displayAs: "remaining" });
    expect(node.textContent).toContain("75% left");
    await act(async () => {
      await i18n.changeLanguage("zh-CN");
    });
    expect(node.textContent).toContain("剩余 75%");
    expect(node.querySelector('[role="checkbox"]')?.getAttribute("aria-label")).toContain(
      "固定 Codex",
    );
    render({
      ...entry,
      report: {
        status: "unavailable",
        problem: { kind: "rejected", status: 401, refreshedBy: "codex login" },
      },
    });
    expect(node.textContent).toContain("不可用");
    expect(node.textContent).toContain("登录被拒绝（HTTP 401）");
    expect(node.textContent).toContain("codex login");
    await act(async () => {
      await i18n.changeLanguage("en");
    });
    expect(node.textContent).toContain("Unavailable");
    expect(node.textContent).toContain("Login rejected (HTTP 401)");
  });
  it("shows the selected account, changes percentage meaning, and pins the chosen window", () => {
    const node = render();
    expect(node.textContent).toContain("selected@example.test");
    expect(node.textContent).toContain("25%");
    const pin = node.querySelector<HTMLElement>('[role="checkbox"]')!;
    act(() => pin.click());
    expect(togglePin).toHaveBeenCalledWith({ sourceId: "codex", windowId: "primary" });
    render(entry, { ...display, displayAs: "remaining" });
    expect(node.textContent).toContain("75% left");
  });
  it("keeps login remedies visible and refreshes only the requested account report", () => {
    const node = render({
      ...entry,
      report: {
        status: "unavailable",
        problem: { kind: "rejected", status: 401, refreshedBy: "codex login" },
      },
    });
    expect(node.textContent).toContain("HTTP 401");
    expect(node.textContent).toContain("codex login");
    act(() => node.querySelector<HTMLElement>('[data-testid="usage-refresh"]')!.click());
    expect(refresh).toHaveBeenCalledOnce();
    expect(node.querySelector('[role="checkbox"]')).toBeNull();
  });
});
