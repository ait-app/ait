import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { Download } from "@/stores/download-store";
import { i18n } from "@/i18n/i18next";
import { DownloadToast } from "./download-toast";

const fixture = vi.hoisted(() => ({ downloads: new Map<string, Download>(), dismiss: vi.fn() }));
vi.mock("@/stores/download-store", () => ({
  useDownloadStore: (
    select: (state: {
      downloads: Map<string, Download>;
      activeDownloadId: string;
      dismissDownload: typeof fixture.dismiss;
    }) => unknown,
  ) =>
    select({
      downloads: fixture.downloads,
      activeDownloadId: "export",
      dismissDownload: fixture.dismiss,
    }),
  formatSpeed: () => "0 B/s",
  formatEta: () => "0s",
}));
vi.mock("react-native-safe-area-context", () => ({
  useSafeAreaInsets: () => ({ top: 0, right: 0, bottom: 0, left: 0 }),
}));
let root: Root;
let container: HTMLDivElement;
beforeEach(async () => {
  await i18n.changeLanguage("en");
  vi.stubGlobal("React", React);
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  fixture.downloads.clear();
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});
afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.clearAllMocks();
  vi.unstubAllGlobals();
});

it.each(["browser_handoff", "complete"] as const)(
  "distinguishes %s from browser-owned saving",
  async (status) => {
    fixture.downloads.set("export", {
      id: "export",
      serverId: "host",
      scopeId: "agent",
      fileName: "session.md",
      status,
      startedAt: Date.now(),
    });
    await act(async () => root.render(<DownloadToast />));
    if (status === "browser_handoff") {
      expect(container.textContent).toContain("Sent to browser");
      expect(container.textContent).toContain("finish saving");
      expect(container.textContent).not.toContain(i18n.t("common.states.downloadComplete"));
      expect(container.textContent).not.toContain(i18n.t("common.states.downloadFailed"));
    } else {
      expect(container.textContent).toContain(i18n.t("common.states.downloadComplete"));
      expect(container.textContent).not.toContain("Sent to browser");
    }
  },
);
