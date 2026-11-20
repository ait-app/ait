import {
  BrowserWindow,
  View,
  WebContentsView,
  session,
  type Rectangle,
  type WebContents,
} from "electron";
import { applyBrowserGuestPermissionPolicy } from "../browser-profile.js";
import type { PaseoBrowserWebviewRegistry } from "./registry.js";

const BROWSER_VIEW_EVENT = "paseo:event:browser-view";
const ZOOM_LEVEL_EPSILON = 0.001;

export type BrowserViewEventType =
  | "did-fail-load"
  | "did-navigate"
  | "did-navigate-in-page"
  | "did-start-loading"
  | "did-stop-loading"
  | "dom-ready"
  | "focus"
  | "page-favicon-updated"
  | "page-title-updated"
  | "blur"
  | "destroyed";

export interface BrowserViewEvent {
  browserId: string;
  type: BrowserViewEventType;
  url?: string;
  title?: string;
  favicons?: string[];
  errorCode?: number;
  errorDescription?: string;
  validatedURL?: string;
  isMainFrame?: boolean;
  canGoBack?: boolean;
  canGoForward?: boolean;
  isLoading?: boolean;
}

export interface BrowserViewBounds {
  container: Rectangle;
  contents: Rectangle;
  visible: boolean;
}

export interface CreateBrowserViewInput {
  browserId: string;
  workspaceId: string;
  profilePartition: string;
  initialUrl: string;
  hostContents: WebContents;
  hostWindow: BrowserWindow;
}

interface BrowserViewEntry {
  browserId: string;
  workspaceId: string;
  hostContents: WebContents;
  hostWindow: BrowserWindow;
  container: View;
  view: WebContentsView;
  contents: WebContents;
}

interface BrowserViewManagerDependencies {
  registry: PaseoBrowserWebviewRegistry;
  keyboardPreloadPath: string;
  registerNavigationGuards: (contents: WebContents) => void;
  installWindowOpenHandler: (input: {
    contents: WebContents;
    sourceContents: WebContents;
    mainWindow: BrowserWindow;
  }) => void;
  installContextMenu: (input: { contents: WebContents; mainWindow: BrowserWindow }) => void;
  attachKeyboard: (input: { contents: WebContents; hostContents: WebContents }) => void;
}

export type BrowserViewCommand = "focus" | "go-back" | "go-forward" | "reload" | "stop";

export class PaseoBrowserWebContentsViewManager {
  private readonly entries = new Map<string, BrowserViewEntry>();

  public constructor(private readonly dependencies: BrowserViewManagerDependencies) {}

  public async create(input: CreateBrowserViewInput): Promise<void> {
    const browserId = input.browserId.trim();
    const workspaceId = input.workspaceId.trim();
    const profilePartition = input.profilePartition.trim();
    if (!browserId || !workspaceId || !profilePartition) {
      throw new Error("Invalid browser view identity");
    }

    const key = this.key(input.hostContents.id, browserId);
    if (this.entries.has(key)) {
      return;
    }

    const profileSession = session.fromPartition(profilePartition);
    applyBrowserGuestPermissionPolicy(profileSession);
    const view = new WebContentsView({
      webPreferences: {
        session: profileSession,
        nodeIntegration: false,
        nodeIntegrationInSubFrames: true,
        nodeIntegrationInWorker: false,
        contextIsolation: true,
        sandbox: true,
        webSecurity: true,
        webviewTag: false,
        allowRunningInsecureContent: false,
        preload: this.dependencies.keyboardPreloadPath,
        // The page follows the host window zoom like a <webview> guest did. Isolated mode
        // keeps the level per view across navigations, and the factor seeds the first load.
        zoomMode: "isolated",
        zoomFactor: input.hostContents.getZoomFactor(),
      },
    });
    // WebContentsView may release its webContents property during destruction.
    // Keep the original object for lifecycle callbacks that run during teardown.
    const contents = view.webContents;
    const container = new View();
    const entry: BrowserViewEntry = {
      browserId,
      workspaceId,
      hostContents: input.hostContents,
      hostWindow: input.hostWindow,
      container,
      view,
      contents,
    };
    this.entries.set(key, entry);
    input.hostWindow.contentView.addChildView(container);
    container.addChildView(view);
    container.setBounds({ x: 0, y: 0, width: 1, height: 1 });
    container.setVisible(false);
    view.setBounds({ x: 0, y: 0, width: 1, height: 1 });

    this.dependencies.registry.registerWebContents({
      webContentsId: contents.id,
      browserId,
      hostWebContentsId: input.hostContents.id,
    });
    this.dependencies.registry.registerWorkspace({ browserId, workspaceId });
    this.dependencies.registerNavigationGuards(contents);
    this.dependencies.installWindowOpenHandler({
      contents,
      sourceContents: contents,
      mainWindow: input.hostWindow,
    });
    this.dependencies.installContextMenu({
      contents,
      mainWindow: input.hostWindow,
    });
    this.dependencies.attachKeyboard({
      contents,
      hostContents: input.hostContents,
    });
    this.installEventForwarding(entry);
    contents.once("destroyed", () => {
      this.removeEntry(entry);
      this.send(entry, { type: "destroyed" });
    });

    try {
      await contents.loadURL(input.initialUrl);
    } catch (error) {
      this.send(entry, {
        type: "did-fail-load",
        errorCode: -2,
        errorDescription: error instanceof Error ? error.message : "Failed to load browser URL",
        validatedURL: input.initialUrl,
        isMainFrame: true,
      });
    }
  }

  public async navigate(hostContentsId: number, browserId: string, url: string): Promise<void> {
    const entry = this.entryFor(hostContentsId, browserId);
    if (!entry) {
      throw new Error("Browser view not found");
    }
    await entry.contents.loadURL(url);
  }

  public command(hostContentsId: number, browserId: string, command: BrowserViewCommand): void {
    const entry = this.entryFor(hostContentsId, browserId);
    if (!entry || entry.contents.isDestroyed()) {
      return;
    }
    switch (command) {
      case "focus":
        entry.contents.focus();
        return;
      case "go-back":
        entry.contents.goBack();
        return;
      case "go-forward":
        entry.contents.goForward();
        return;
      case "reload":
        entry.contents.reload();
        return;
      case "stop":
        entry.contents.stop();
        return;
    }
  }

  public executeJavaScript(
    hostContentsId: number,
    browserId: string,
    code: string,
  ): Promise<unknown> {
    const entry = this.entryFor(hostContentsId, browserId);
    if (!entry || entry.contents.isDestroyed()) {
      return Promise.reject(new Error("Browser view not found"));
    }
    return entry.contents.executeJavaScript(code);
  }

  public setBounds(hostContentsId: number, browserId: string, bounds: BrowserViewBounds): void {
    const entry = this.entryFor(hostContentsId, browserId);
    if (!entry || entry.hostContents.isDestroyed() || entry.contents.isDestroyed()) {
      return;
    }
    const zoomFactor = entry.hostContents.getZoomFactor();
    entry.container.setBounds(normalizeRectangle(cssPixelsToDips(bounds.container, zoomFactor)));
    entry.view.setBounds(normalizeRectangle(cssPixelsToDips(bounds.contents, zoomFactor)));
    entry.container.setVisible(bounds.visible);
    followHostZoom(entry.contents, entry.hostContents);
  }

  public destroy(hostContentsId: number, browserId: string): void {
    const entry = this.entryFor(hostContentsId, browserId);
    if (!entry) {
      return;
    }
    this.removeEntry(entry);
    if (!entry.contents.isDestroyed()) {
      entry.contents.close();
    }
  }

  public destroyHost(hostContentsId: number): void {
    for (const entry of this.entries.values()) {
      if (entry.hostContents.id === hostContentsId) {
        this.destroy(hostContentsId, entry.browserId);
      }
    }
  }

  private installEventForwarding(entry: BrowserViewEntry): void {
    const { contents } = entry;
    contents.on("did-start-loading", () => this.send(entry, { type: "did-start-loading" }));
    contents.on("did-stop-loading", () => this.send(entry, { type: "did-stop-loading" }));
    contents.on("dom-ready", () => this.send(entry, { type: "dom-ready" }));
    contents.on("focus", () => this.send(entry, { type: "focus" }));
    contents.on("blur", () => this.send(entry, { type: "blur" }));
    contents.on("did-navigate", (_event, url) =>
      this.send(entry, { type: "did-navigate", url, isMainFrame: true }),
    );
    contents.on("did-navigate-in-page", (_event, url, isMainFrame) =>
      this.send(entry, { type: "did-navigate-in-page", url, isMainFrame }),
    );
    contents.on("page-title-updated", (_event, title) =>
      this.send(entry, { type: "page-title-updated", title }),
    );
    contents.on("page-favicon-updated", (_event, favicons) =>
      this.send(entry, { type: "page-favicon-updated", favicons }),
    );
    contents.on("did-fail-load", (_event, errorCode, errorDescription, validatedURL, isMainFrame) =>
      this.send(entry, {
        type: "did-fail-load",
        errorCode,
        errorDescription,
        validatedURL,
        isMainFrame,
      }),
    );
  }

  private send(entry: BrowserViewEntry, event: Omit<BrowserViewEvent, "browserId">): void {
    if (!entry.hostContents.isDestroyed()) {
      const { contents } = entry;
      const destroyed = contents.isDestroyed();
      entry.hostContents.send(BROWSER_VIEW_EVENT, {
        browserId: entry.browserId,
        canGoBack: destroyed ? false : contents.canGoBack(),
        canGoForward: destroyed ? false : contents.canGoForward(),
        isLoading: destroyed ? false : contents.isLoading(),
        ...event,
      });
    }
  }

  private removeEntry(entry: BrowserViewEntry): void {
    const key = this.key(entry.hostContents.id, entry.browserId);
    if (this.entries.get(key) !== entry) {
      return;
    }
    this.entries.delete(key);
    if (!entry.hostWindow.isDestroyed()) {
      entry.hostWindow.contentView.removeChildView(entry.container);
    }
    this.dependencies.registry.unregisterWebContents(entry.contents.id);
  }

  private entryFor(hostContentsId: number, browserId: string): BrowserViewEntry | null {
    return this.entries.get(this.key(hostContentsId, browserId)) ?? null;
  }

  private key(hostContentsId: number, browserId: string): string {
    return `${hostContentsId}:${browserId}`;
  }
}

// The renderer measures the pane in host page CSS pixels, while View.setBounds takes
// device-independent pixels, so the host page zoom scales the rectangle. Edges are
// snapped so the scaled size does not drift by a pixel.
function cssPixelsToDips(rectangle: Rectangle, zoomFactor: number): Rectangle {
  const x = Math.round(rectangle.x * zoomFactor);
  const y = Math.round(rectangle.y * zoomFactor);
  return {
    x,
    y,
    width: Math.round((rectangle.x + rectangle.width) * zoomFactor) - x,
    height: Math.round((rectangle.y + rectangle.height) * zoomFactor) - y,
  };
}

function followHostZoom(guest: WebContents, host: WebContents): void {
  const hostLevel = host.getZoomLevel();
  if (Math.abs(guest.getZoomLevel() - hostLevel) > ZOOM_LEVEL_EPSILON) {
    guest.setZoomLevel(hostLevel);
  }
}

function normalizeRectangle(rectangle: Rectangle): Rectangle {
  return {
    x: Math.round(Number.isFinite(rectangle.x) ? rectangle.x : 0),
    y: Math.round(Number.isFinite(rectangle.y) ? rectangle.y : 0),
    width: Math.max(1, Math.round(Number.isFinite(rectangle.width) ? rectangle.width : 1)),
    height: Math.max(1, Math.round(Number.isFinite(rectangle.height) ? rectangle.height : 1)),
  };
}
