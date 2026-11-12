import { describe, expect, it } from "vitest";
import {
  canDesktopAppSidebarShare,
  shouldYieldExplorerToNavigation,
  resolveDesktopAppChromeLayout,
  resolveDesktopAppContentMinimum,
  resolveDesktopSidebarVisibility,
  resolveDesktopSidebarWidth,
} from "@/components/desktop-sidebar-layout";

describe("desktop sidebar layout", () => {
  it("keeps a retained sidebar hidden while app chrome is suppressed", () => {
    expect(
      resolveDesktopSidebarVisibility({
        chromeEnabled: false,
        isCompactLayout: false,
        isMounted: true,
        isOpen: true,
        canShare: true,
      }),
    ).toBe(false);
  });

  it("keeps the sidebar toggle window-owned beside left window controls", () => {
    expect(
      resolveDesktopAppChromeLayout({
        desktopSidebarRendered: true,
        hasTopLeftWindowControls: true,
        sidebarControlsEnabled: true,
      }),
    ).toEqual({
      sidebarCorners: "top-left",
      contentCorners: "top-right",
      sidebarToggleOwner: "window",
    });
    expect(
      resolveDesktopAppChromeLayout({
        desktopSidebarRendered: true,
        hasTopLeftWindowControls: false,
        sidebarControlsEnabled: true,
      }),
    ).toEqual({
      sidebarCorners: "none",
      contentCorners: "both",
      sidebarToggleOwner: "content",
    });
    expect(
      resolveDesktopAppChromeLayout({
        desktopSidebarRendered: false,
        hasTopLeftWindowControls: true,
        sidebarControlsEnabled: true,
      }),
    ).toEqual({
      sidebarCorners: "none",
      contentCorners: "both",
      sidebarToggleOwner: "window",
    });
  });

  it("hides the window-owned sidebar toggle when app chrome is suppressed", () => {
    expect(
      resolveDesktopAppChromeLayout({
        desktopSidebarRendered: false,
        hasTopLeftWindowControls: true,
        sidebarControlsEnabled: false,
      }).sidebarToggleOwner,
    ).toBe("none");
  });

  it("clamps a persisted wide sidebar to preserve the center pane", () => {
    const atHalfScreen = resolveDesktopSidebarWidth({ requestedWidth: 600, viewportWidth: 751 });
    expect(atHalfScreen).toBe(351);
    expect(751 - atHalfScreen).toBe(400);

    const atBreakpoint = resolveDesktopSidebarWidth({ requestedWidth: 600, viewportWidth: 720 });
    expect(atBreakpoint).toBe(320);
    expect(720 - atBreakpoint).toBe(400);

    expect(resolveDesktopSidebarWidth({ requestedWidth: 600, viewportWidth: 1440 })).toBe(600);
  });

  it("yields app navigation when settings needs the shell width", () => {
    const settingsMinimum = resolveDesktopAppContentMinimum({ isSettingsRoute: true });
    expect(settingsMinimum).toBe(720);
    expect(
      canDesktopAppSidebarShare({
        contentMinimumWidth: settingsMinimum,
        requestedSidebarWidth: 320,
        viewportWidth: 751,
      }),
    ).toBe(false);
  });

  it("yields native navigation when two sidebars would squeeze the conversation", () => {
    const minimum = resolveDesktopAppContentMinimum({
      isSettingsRoute: false,
      nativeExplorerOpen: true,
    });
    expect(minimum).toBe(640);
    expect(
      canDesktopAppSidebarShare({
        contentMinimumWidth: minimum,
        requestedSidebarWidth: 320,
        viewportWidth: 852,
      }),
    ).toBe(false);
    expect(
      canDesktopAppSidebarShare({
        contentMinimumWidth: minimum,
        requestedSidebarWidth: 320,
        viewportWidth: 960,
      }),
    ).toBe(true);
  });

  it("imposes no content minimum outside settings", () => {
    expect(resolveDesktopAppContentMinimum({ isSettingsRoute: false })).toBe(0);
    expect(
      canDesktopAppSidebarShare({
        contentMinimumWidth: resolveDesktopAppContentMinimum({ isSettingsRoute: false }),
        requestedSidebarWidth: 320,
        viewportWidth: 751,
      }),
    ).toBe(true);
  });
});

describe("explicit navigation beside native explorer", () => {
  const foldedWorkspace = {
    isCompact: false,
    nativeWorkspaceActive: true,
    explorerOpen: true,
    requestedSidebarWidth: 320,
    viewportWidth: 852,
  };

  it("yields explorer so opening navigation can make it visible", () => {
    expect(shouldYieldExplorerToNavigation(foldedWorkspace)).toBe(true);
    expect(
      canDesktopAppSidebarShare({
        contentMinimumWidth: resolveDesktopAppContentMinimum({
          isSettingsRoute: false,
          nativeExplorerOpen: false,
        }),
        requestedSidebarWidth: foldedWorkspace.requestedSidebarWidth,
        viewportWidth: foldedWorkspace.viewportWidth,
      }),
    ).toBe(true);
    // Reopening explorer restores its priority over navigation at this width.
    expect(
      canDesktopAppSidebarShare({
        contentMinimumWidth: resolveDesktopAppContentMinimum({
          isSettingsRoute: false,
          nativeExplorerOpen: true,
        }),
        requestedSidebarWidth: foldedWorkspace.requestedSidebarWidth,
        viewportWidth: foldedWorkspace.viewportWidth,
      }),
    ).toBe(false);
  });

  it.each([
    { isCompact: true },
    { nativeWorkspaceActive: false },
    { explorerOpen: false },
    { viewportWidth: 960 },
  ])("retains normal toggling for %j", (override) => {
    expect(shouldYieldExplorerToNavigation({ ...foldedWorkspace, ...override })).toBe(false);
  });
});
