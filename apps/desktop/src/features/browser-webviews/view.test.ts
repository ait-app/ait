import { describe, expect, test, vi } from "vitest";
import type { BrowserWindow, WebContents } from "electron";
import { PaseoBrowserWebviewRegistry } from "./registry.js";
import { PaseoBrowserWebContentsViewManager, type BrowserViewBounds } from "./view.js";

const fakeElectron = vi.hoisted(() => {
  class FakeView {
    public readonly addChildView = vi.fn();
    public readonly setBounds = vi.fn();
    public readonly setVisible = vi.fn();
  }

  class FakeGuestContents {
    public readonly id = 900;
    public zoomLevel = 0;
    public readonly setZoomLevel = vi.fn((level: number) => {
      this.zoomLevel = level;
    });

    public getZoomLevel(): number {
      return this.zoomLevel;
    }

    public isDestroyed(): boolean {
      return false;
    }

    public on(): this {
      return this;
    }

    public once(): this {
      return this;
    }

    public loadURL(): Promise<void> {
      return Promise.resolve();
    }
  }

  class FakeWebContentsView extends FakeView {
    public readonly webContents = new FakeGuestContents();

    public constructor(public readonly options: { webPreferences: Record<string, unknown> }) {
      super();
    }
  }

  return { FakeView, FakeWebContentsView };
});

vi.mock("electron", () => ({
  View: fakeElectron.FakeView,
  WebContentsView: fakeElectron.FakeWebContentsView,
  session: { fromPartition: vi.fn(() => ({})) },
}));

vi.mock("../browser-profile.js", () => ({ applyBrowserGuestPermissionPolicy: vi.fn() }));

type FakeView = InstanceType<typeof fakeElectron.FakeView>;
type FakeWebContentsView = InstanceType<typeof fakeElectron.FakeWebContentsView>;

const BROWSER_ID = "browser-a";

// Chromium relates a zoom level to its factor with factor = 1.2 ^ level.
function zoomLevelFor(zoomFactor: number): number {
  return Math.log(zoomFactor) / Math.log(1.2);
}

class FakeHostContents {
  public destroyed = false;
  public readonly send = vi.fn();

  public constructor(
    public readonly id: number,
    public zoomFactor: number,
  ) {}

  public getZoomFactor(): number {
    return this.zoomFactor;
  }

  public getZoomLevel(): number {
    return zoomLevelFor(this.zoomFactor);
  }

  public isDestroyed(): boolean {
    return this.destroyed;
  }
}

async function openBrowser(zoomFactor: number) {
  const host = new FakeHostContents(7, zoomFactor);
  const hostWindow = {
    isDestroyed: () => false,
    contentView: {
      addChildView: vi.fn<(child: FakeView) => void>(),
      removeChildView: vi.fn(),
    },
  };
  const manager = new PaseoBrowserWebContentsViewManager({
    registry: new PaseoBrowserWebviewRegistry(),
    keyboardPreloadPath: "/tmp/keyboard-preload.js",
    registerNavigationGuards: vi.fn(),
    installWindowOpenHandler: vi.fn(),
    installContextMenu: vi.fn(),
    attachKeyboard: vi.fn(),
  });

  await manager.create({
    browserId: BROWSER_ID,
    workspaceId: "workspace-a",
    profilePartition: "persist:ait-browser",
    initialUrl: "https://example.com/",
    hostContents: host as unknown as WebContents,
    hostWindow: hostWindow as unknown as BrowserWindow,
  });

  const container = hostWindow.contentView.addChildView.mock.calls[0][0];
  const view = container.addChildView.mock.calls[0][0] as FakeWebContentsView;
  // Forget the 1x1 parking calls create() makes so tests only see what setBounds does.
  container.setBounds.mockClear();
  container.setVisible.mockClear();
  view.setBounds.mockClear();
  return {
    host,
    container,
    view,
    setBounds: (bounds: BrowserViewBounds) => manager.setBounds(host.id, BROWSER_ID, bounds),
  };
}

const PANE: BrowserViewBounds = {
  container: { x: 100, y: 40, width: 400, height: 300 },
  contents: { x: 0, y: 0, width: 400, height: 300 },
  visible: true,
};

describe("browser WebContentsView bounds", () => {
  test("converts host CSS pixel rectangles to device-independent pixels by the host zoom", async () => {
    const { container, view, setBounds } = await openBrowser(1.25);

    setBounds(PANE);

    expect(container.setBounds).toHaveBeenLastCalledWith({
      x: 125,
      y: 50,
      width: 500,
      height: 375,
    });
    expect(view.setBounds).toHaveBeenLastCalledWith({ x: 0, y: 0, width: 500, height: 375 });
    expect(container.setVisible).toHaveBeenLastCalledWith(true);
  });

  test("passes rectangles through unchanged at 100% zoom", async () => {
    const { container, view, setBounds } = await openBrowser(1);

    setBounds({
      container: { x: 100, y: 40, width: 400.4, height: 299.6 },
      contents: { x: -12, y: 8, width: 388.2, height: 291.7 },
      visible: false,
    });

    expect(container.setBounds).toHaveBeenLastCalledWith({
      x: 100,
      y: 40,
      width: 400,
      height: 300,
    });
    expect(view.setBounds).toHaveBeenLastCalledWith({ x: -12, y: 8, width: 388, height: 292 });
    expect(container.setVisible).toHaveBeenLastCalledWith(false);
  });

  test("snaps scaled edges instead of rounding the size on its own", async () => {
    const { container, setBounds } = await openBrowser(1.25);

    // Edges land on 1.25 and 2.5 device pixels, which snap to 1 and 3.
    setBounds({ ...PANE, container: { x: 1, y: 1, width: 1, height: 1 } });

    expect(container.setBounds).toHaveBeenLastCalledWith({ x: 1, y: 1, width: 2, height: 2 });
  });

  test("keeps a one pixel minimum when zoomed far out", async () => {
    const { container, view, setBounds } = await openBrowser(0.25);

    setBounds({
      container: { x: 0, y: 0, width: 1, height: 1 },
      contents: { x: 0, y: 0, width: 1, height: 1 },
      visible: false,
    });

    expect(container.setBounds).toHaveBeenLastCalledWith({ x: 0, y: 0, width: 1, height: 1 });
    expect(view.setBounds).toHaveBeenLastCalledWith({ x: 0, y: 0, width: 1, height: 1 });
  });

  test("ignores bounds once the host contents is destroyed", async () => {
    const { host, container, view, setBounds } = await openBrowser(1.25);
    host.destroyed = true;

    setBounds(PANE);

    expect(container.setBounds).not.toHaveBeenCalled();
    expect(view.setBounds).not.toHaveBeenCalled();
    expect(container.setVisible).not.toHaveBeenCalled();
    expect(view.webContents.setZoomLevel).not.toHaveBeenCalled();
  });
});

describe("browser WebContentsView zoom", () => {
  test("creates the view with isolated zoom starting at the host zoom factor", async () => {
    const { view } = await openBrowser(1.25);

    expect(view.options.webPreferences).toMatchObject({ zoomMode: "isolated", zoomFactor: 1.25 });
  });

  test("sets the page zoom level to the host level when they differ", async () => {
    const { host, view, setBounds } = await openBrowser(1);
    host.zoomFactor = 1.25;

    setBounds(PANE);
    setBounds(PANE);

    expect(view.webContents.setZoomLevel).toHaveBeenCalledOnce();
    expect(view.webContents.setZoomLevel).toHaveBeenCalledWith(zoomLevelFor(1.25));
  });

  test.each([
    ["exactly", 0],
    ["within floating point error", 1e-9],
  ])("leaves the page zoom alone when it matches the host level %s", async (_label, drift) => {
    const { view, setBounds } = await openBrowser(1.25);
    view.webContents.zoomLevel = zoomLevelFor(1.25) + drift;

    setBounds(PANE);

    expect(view.webContents.setZoomLevel).not.toHaveBeenCalled();
  });
});
