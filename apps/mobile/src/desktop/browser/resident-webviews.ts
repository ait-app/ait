import {
  getDesktopHost,
  type DesktopBrowserViewBounds,
  type DesktopBrowserViewEvent,
  type DesktopAttachedBrowserRegistration,
  type DesktopBrowserBridge,
} from "@/desktop/host";
import type { BrowserViewport } from "@/desktop/browser/store";
import { WEB_SURFACE_PLANE } from "@/lib/overlay-root";
import { trimNonEmpty } from "@/utils/string";

const RESIDENT_BROWSER_HOST_ID = "paseo-browser-resident-webviews";
const BROWSER_ID_ATTRIBUTE = "data-paseo-browser-id";
const BROWSER_SURFACE_ATTRIBUTE = "data-paseo-browser-surface";
const RESIDENT_VIEWPORT_WIDTH = 1280;
const RESIDENT_VIEWPORT_HEIGHT = 800;

const residentWebviewsByBrowserId = new Map<string, HTMLElement>();
const residentSurfacesByBrowserId = new Map<string, HTMLElement>();
const residentWebviewSizesByBrowserId = new Map<string, { width: number; height: number }>();

interface BrowserWebviewElement extends HTMLElement {
  src: string;
  getWebContentsId(): number;
  canGoBack?: () => boolean;
  canGoForward?: () => boolean;
  goBack?: () => void;
  goForward?: () => void;
  reload?: () => void;
  stop?: () => void;
  loadURL?: (url: string) => Promise<void>;
  getURL?: () => string;
  isLoading?: () => boolean;
  executeJavaScript?: (code: string) => Promise<unknown>;
}

interface BrowserViewProxyState {
  browserId: string;
  workspaceId: string;
  url: string;
  loading: boolean;
  canGoBack: boolean;
  canGoForward: boolean;
  destroyed: boolean;
  unsubscribe?: () => void;
  unsubscribePromise?: Promise<() => void>;
}

const BROWSER_VIEW_PROXY = Symbol("browser-view-proxy");

type BrowserViewProxyElement = BrowserWebviewElement & {
  [BROWSER_VIEW_PROXY]?: BrowserViewProxyState;
};

interface BrowserWebviewIdentity {
  browserId: string;
  workspaceId: string;
}

export interface BrowserWebviewProfileHost {
  profilePartition: string;
  registerAttachedBrowser(input: DesktopAttachedBrowserRegistration): Promise<void>;
}

function isAttachedBrowserBridge(
  browser: DesktopBrowserBridge | undefined,
): browser is BrowserWebviewProfileHost {
  return (
    browser !== undefined &&
    typeof browser.profilePartition === "string" &&
    browser.profilePartition.startsWith("persist:") &&
    typeof browser.registerAttachedBrowser === "function"
  );
}

function getBrowserBridge(override?: BrowserWebviewProfileHost): BrowserWebviewProfileHost {
  if (override) {
    return override;
  }
  const browser = getDesktopHost()?.browser;
  if (!isAttachedBrowserBridge(browser)) {
    throw new Error("Electron browser profile bridge is unavailable");
  }
  return browser;
}

function getNativeBrowserBridge(): DesktopBrowserBridge | null {
  const browser = getDesktopHost()?.browser;
  return browser && typeof browser.createView === "function" ? browser : null;
}

function dispatchBrowserViewEvent(
  element: BrowserViewProxyElement,
  event: DesktopBrowserViewEvent,
): void {
  const browserEvent = new Event(event.type);
  for (const [key, value] of Object.entries(event)) {
    if (key === "browserId" || value === undefined) {
      continue;
    }
    Object.defineProperty(browserEvent, key, { configurable: true, enumerable: true, value });
  }
  element.dispatchEvent(browserEvent);
}

function attachNativeBrowserView(
  element: BrowserViewProxyElement,
  input: BrowserWebviewIdentity & { initialUrl: string; profilePartition: string },
): void {
  const browser = getNativeBrowserBridge();
  if (!browser?.createView) {
    return;
  }
  const state: BrowserViewProxyState = {
    browserId: input.browserId,
    workspaceId: input.workspaceId,
    url: input.initialUrl,
    loading: false,
    canGoBack: false,
    canGoForward: false,
    destroyed: false,
  };
  element[BROWSER_VIEW_PROXY] = state;

  const handleEvent = (payload: unknown) => {
    if (!payload || typeof payload !== "object") {
      return;
    }
    const event = payload as Partial<DesktopBrowserViewEvent>;
    if (event.browserId !== state.browserId || typeof event.type !== "string") {
      return;
    }
    if (event.type === "did-start-loading") {
      state.loading = true;
    } else if (event.type === "did-stop-loading" || event.type === "did-fail-load") {
      state.loading = false;
    } else if (
      (event.type === "did-navigate" || event.type === "did-navigate-in-page") &&
      typeof event.url === "string"
    ) {
      state.url = event.url;
    } else if (event.type === "destroyed") {
      state.destroyed = true;
      state.unsubscribe?.();
    }
    if (typeof event.canGoBack === "boolean") {
      state.canGoBack = event.canGoBack;
    }
    if (typeof event.canGoForward === "boolean") {
      state.canGoForward = event.canGoForward;
    }
    if (typeof event.isLoading === "boolean") {
      state.loading = event.isLoading;
    }
    dispatchBrowserViewEvent(element, event as DesktopBrowserViewEvent);
  };
  const events = getDesktopHost()?.events;
  const unsubscribe = events?.on?.("browser-view", handleEvent);
  state.unsubscribePromise = unsubscribe ? Promise.resolve(unsubscribe) : undefined;
  void state.unsubscribePromise?.then((unsubscribe) => {
    state.unsubscribe = unsubscribe;
    if (state.destroyed) {
      unsubscribe();
    }
  });

  const command = (name: "focus" | "go-back" | "go-forward" | "reload" | "stop") => {
    void browser.commandView?.(state.browserId, name);
  };
  element.src = input.initialUrl;
  element.getWebContentsId = () => -1;
  element.canGoBack = () => state.canGoBack;
  element.canGoForward = () => state.canGoForward;
  element.goBack = () => command("go-back");
  element.goForward = () => command("go-forward");
  element.reload = () => command("reload");
  element.stop = () => command("stop");
  element.loadURL = (url) => {
    state.url = url;
    return browser.navigateView?.(state.browserId, url) ?? Promise.resolve();
  };
  element.getURL = () => state.url;
  element.isLoading = () => state.loading;
  element.executeJavaScript = (code) =>
    browser.executeViewJavaScript?.(state.browserId, code) ?? Promise.resolve(null);
  element.focus = () => command("focus");

  void browser
    .createView({
      browserId: input.browserId,
      workspaceId: input.workspaceId,
      profilePartition: input.profilePartition,
      initialUrl: input.initialUrl,
    })
    .catch((error: unknown) => {
      state.loading = false;
      dispatchBrowserViewEvent(element, {
        browserId: state.browserId,
        type: "did-fail-load",
        errorCode: -2,
        errorDescription: error instanceof Error ? error.message : "Failed to create browser view",
        validatedURL: input.initialUrl,
        isMainFrame: true,
      });
    });
}

function nativeBrowserViewBounds(
  element: BrowserViewProxyElement,
): ((bounds: DesktopBrowserViewBounds) => void) | null {
  const browser = getNativeBrowserBridge();
  const state = element[BROWSER_VIEW_PROXY];
  if (!browser?.setViewBounds || !state) {
    return null;
  }
  return (bounds) => {
    void browser.setViewBounds?.(state.browserId, bounds);
  };
}

function destroyNativeBrowserView(element: BrowserViewProxyElement): void {
  const state = element[BROWSER_VIEW_PROXY];
  if (!state || state.destroyed) {
    return;
  }
  state.destroyed = true;
  state.unsubscribe?.();
  void state.unsubscribePromise?.then((unsubscribe) => unsubscribe());
  void getNativeBrowserBridge()?.destroyView?.(state.browserId);
}

function registerBrowserWhenAttached(
  webview: BrowserWebviewElement,
  identity: BrowserWebviewIdentity,
  browser: BrowserWebviewProfileHost,
): void {
  // Reparenting a webview can replace its guest WebContents without replacing
  // this DOM element, so every attachment needs a fresh main-process registration.
  webview.addEventListener("did-attach", () => {
    const webContentsId = webview.getWebContentsId();
    void browser
      .registerAttachedBrowser({
        browserId: identity.browserId,
        workspaceId: identity.workspaceId,
        webContentsId,
      })
      .catch((error) => {
        console.error("[browser-webview] attached registration failed", error);
      });
  });
}

function readDocument(): Document | null {
  return typeof document === "undefined" ? null : document;
}

function applyResidentHostParkingStyle(host: HTMLElement): void {
  // The host is permanent. Individual browser surfaces switch between their
  // pane bounds and the proven paintable 1x1 parking geometry.
  host.removeAttribute("aria-hidden");
  host.style.position = "fixed";
  host.style.left = "0";
  host.style.top = "0";
  host.style.width = "100vw";
  host.style.height = "100vh";
  host.style.overflow = "visible";
  host.style.opacity = "1";
  host.style.pointerEvents = "none";
  host.style.display = "block";
  host.style.zIndex = String(WEB_SURFACE_PLANE.browser);
  host.style.clipPath = "";
  host.style.visibility = "visible";
  host.style.transform = "";
}

function applyParkedBrowserSurfaceStyle(surface: HTMLElement): void {
  surface.setAttribute("aria-hidden", "true");
  surface.style.position = "fixed";
  surface.style.left = "0";
  surface.style.top = "0";
  surface.style.width = "1px";
  surface.style.height = "1px";
  surface.style.overflow = "hidden";
  surface.style.opacity = "1";
  surface.style.pointerEvents = "none";
  surface.style.display = "block";
  surface.style.visibility = "visible";
  surface.style.transform = "";
}

function getBrowserSurface(browserId: string, ownerDocument: Document): HTMLElement {
  const existing = residentSurfacesByBrowserId.get(browserId);
  if (existing?.isConnected) {
    return existing;
  }
  const surface = ownerDocument.createElement("div");
  surface.setAttribute(BROWSER_SURFACE_ATTRIBUTE, browserId);
  applyParkedBrowserSurfaceStyle(surface);
  getResidentBrowserHost(ownerDocument).appendChild(surface);
  residentSurfacesByBrowserId.set(browserId, surface);
  return surface;
}

function getResidentBrowserHost(ownerDocument: Document): HTMLElement {
  const existing = ownerDocument.getElementById(RESIDENT_BROWSER_HOST_ID);
  if (existing) {
    applyResidentHostParkingStyle(existing);
    return existing;
  }

  const host = ownerDocument.createElement("div");
  host.id = RESIDENT_BROWSER_HOST_ID;
  applyResidentHostParkingStyle(host);
  ownerDocument.body.appendChild(host);
  return host;
}

function findBrowserWebview(browserId: string, ownerDocument: Document): HTMLElement | null {
  for (const element of ownerDocument.querySelectorAll(`[${BROWSER_ID_ATTRIBUTE}]`)) {
    if (!(element instanceof HTMLElement)) {
      continue;
    }
    if (element.getAttribute(BROWSER_ID_ATTRIBUTE) === browserId) {
      return element;
    }
  }
  return null;
}

function dimensionsForBrowser(browserId: string | null): { width: number; height: number } {
  return (
    (browserId ? residentWebviewSizesByBrowserId.get(browserId) : undefined) ?? {
      width: RESIDENT_VIEWPORT_WIDTH,
      height: RESIDENT_VIEWPORT_HEIGHT,
    }
  );
}

function applyResidentWebviewStyle(webview: HTMLElement, browserId: string | null): void {
  const dimensions = dimensionsForBrowser(browserId);
  webview.style.display = "inline-flex";
  webview.style.flex = "0 0 auto";
  webview.style.width = `${dimensions.width}px`;
  webview.style.height = `${dimensions.height}px`;
  webview.style.border = "0";
  webview.style.background = "transparent";
  webview.style.position = "absolute";
  webview.style.left = "0";
  webview.style.top = "0";
  webview.style.marginTop = "0";
  webview.style.zIndex = "0";
}

function clearResidentWebviewParkingStyle(webview: HTMLElement): void {
  webview.style.position = "";
  webview.style.left = "";
  webview.style.top = "";
  webview.style.marginTop = "";
  webview.style.zIndex = "";
}

export function rememberBrowserWebviewSize(input: {
  browserId: string;
  width: number;
  height: number;
}): { width: number; height: number } | null {
  const browserId = trimNonEmpty(input.browserId);
  if (!browserId || input.width <= 0 || input.height <= 0) {
    return null;
  }
  const dimensions = {
    width: Math.max(1, Math.round(input.width)),
    height: Math.max(1, Math.round(input.height)),
  };
  residentWebviewSizesByBrowserId.set(browserId, dimensions);
  return dimensions;
}

function applyBrowserWebviewDimensions(
  webview: HTMLElement,
  dimensions: { width: number; height: number },
): void {
  webview.style.display = "flex";
  webview.style.border = "0";
  webview.style.background = "transparent";
  webview.style.flex = "0 0 auto";
  webview.style.width = `${Math.max(1, Math.round(dimensions.width))}px`;
  webview.style.height = `${Math.max(1, Math.round(dimensions.height))}px`;
}

export function applyInactiveBrowserWebviewViewport(
  browserId: string,
  webview: HTMLElement,
  viewport: BrowserViewport,
): void {
  if (viewport.mode === "fixed") {
    rememberBrowserWebviewSize({ browserId, width: viewport.width, height: viewport.height });
  }
  applyResidentWebviewStyle(webview, trimNonEmpty(browserId));
  nativeBrowserViewBounds(webview as BrowserViewProxyElement)?.({
    container: { x: 0, y: 0, width: 1, height: 1 },
    contents: { x: 0, y: 0, width: 1, height: 1 },
    visible: false,
  });
}

export function presentBrowserWebview(
  browserId: string,
  webview: HTMLElement,
  anchor: HTMLElement,
  clip: HTMLElement,
  viewport: BrowserViewport,
): void {
  const normalizedBrowserId = trimNonEmpty(browserId);
  if (!normalizedBrowserId) {
    return;
  }
  const ownerDocument = readDocument();
  if (!ownerDocument) {
    return;
  }
  const surface = getBrowserSurface(normalizedBrowserId, ownerDocument);
  if (webview.parentElement !== surface) {
    surface.appendChild(webview);
  }
  const anchorBounds = anchor.getBoundingClientRect();
  const clipBounds = clip.getBoundingClientRect();
  const left = Math.max(anchorBounds.left, clipBounds.left);
  const top = Math.max(anchorBounds.top, clipBounds.top);
  const right = Math.min(
    anchorBounds.left + anchorBounds.width,
    clipBounds.left + clipBounds.width,
  );
  const bottom = Math.min(
    anchorBounds.top + anchorBounds.height,
    clipBounds.top + clipBounds.height,
  );
  const surfaceLeft = Math.ceil(left);
  const surfaceTop = Math.ceil(top);
  const surfaceRight = Math.floor(right);
  const surfaceBottom = Math.floor(bottom);
  const hasVisibleArea = surfaceRight > surfaceLeft && surfaceBottom > surfaceTop;
  surface.setAttribute("aria-hidden", "false");
  surface.style.position = "fixed";
  surface.style.left = `${surfaceLeft}px`;
  surface.style.top = `${surfaceTop}px`;
  surface.style.width = `${Math.max(0, surfaceRight - surfaceLeft)}px`;
  surface.style.height = `${Math.max(0, surfaceBottom - surfaceTop)}px`;
  surface.style.overflow = "hidden";
  surface.style.opacity = "1";
  surface.style.pointerEvents = hasVisibleArea ? "auto" : "none";
  surface.style.display = "flex";
  surface.style.visibility = "visible";
  clearResidentWebviewParkingStyle(webview);
  applyBrowserWebviewDimensions(
    webview,
    viewport.mode === "responsive"
      ? { width: anchorBounds.width, height: anchorBounds.height }
      : viewport,
  );
  webview.style.position = "absolute";
  webview.style.left = `${Math.round(anchorBounds.left - surfaceLeft)}px`;
  webview.style.top = `${Math.round(anchorBounds.top - surfaceTop)}px`;
  nativeBrowserViewBounds(webview as BrowserViewProxyElement)?.({
    container: {
      x: surfaceLeft,
      y: surfaceTop,
      width: Math.max(1, surfaceRight - surfaceLeft),
      height: Math.max(1, surfaceBottom - surfaceTop),
    },
    contents: {
      x: Math.round(anchorBounds.left - surfaceLeft),
      y: Math.round(anchorBounds.top - surfaceTop),
      width: Math.max(1, viewport.mode === "responsive" ? anchorBounds.width : viewport.width),
      height: Math.max(1, viewport.mode === "responsive" ? anchorBounds.height : viewport.height),
    },
    visible: hasVisibleArea,
  });
}

export function prepareBrowserWebview(
  webview: HTMLElement,
  input: {
    browserId: string;
    workspaceId: string;
    initialUrl?: string | null;
    profileHost?: BrowserWebviewProfileHost;
  },
): void {
  const browser = getBrowserBridge(input.profileHost);
  webview.setAttribute(BROWSER_ID_ATTRIBUTE, input.browserId);
  webview.setAttribute("partition", browser.profilePartition);
  webview.setAttribute("allowpopups", "true");
  webview.setAttribute("spellcheck", "false");
  webview.setAttribute("autosize", "on");
  const nativeBrowser = getNativeBrowserBridge();
  if (nativeBrowser) {
    attachNativeBrowserView(webview as BrowserViewProxyElement, {
      browserId: input.browserId,
      workspaceId: input.workspaceId,
      initialUrl: input.initialUrl ?? "about:blank",
      profilePartition: browser.profilePartition,
    });
    return;
  }
  if (input.initialUrl) {
    (webview as BrowserWebviewElement).src = input.initialUrl;
  }
  registerBrowserWhenAttached(webview as BrowserWebviewElement, input, browser);
}

export function createBrowserWebviewElement(ownerDocument: Document): HTMLElement {
  return ownerDocument.createElement(getNativeBrowserBridge() ? "div" : "webview");
}

export function ensureResidentBrowserWebview(input: {
  browserId: string;
  workspaceId: string;
  url: string;
  profileHost?: BrowserWebviewProfileHost;
}): HTMLElement | null {
  const browserId = trimNonEmpty(input.browserId);
  if (!browserId) {
    return null;
  }
  const ownerDocument = readDocument();
  if (!ownerDocument) {
    return null;
  }

  const resident = residentWebviewsByBrowserId.get(browserId) ?? null;
  if (resident?.isConnected) {
    releaseResidentBrowserWebview(browserId, resident);
    return resident;
  }

  const existing = findBrowserWebview(browserId, ownerDocument);
  if (existing) {
    if (existing.parentElement?.id === RESIDENT_BROWSER_HOST_ID) {
      releaseResidentBrowserWebview(browserId, existing);
    }
    return existing;
  }

  const webview = createBrowserWebviewElement(ownerDocument) as BrowserWebviewElement;
  prepareBrowserWebview(webview, {
    browserId,
    workspaceId: input.workspaceId,
    initialUrl: input.url,
    profileHost: input.profileHost,
  });
  releaseResidentBrowserWebview(browserId, webview);
  return webview;
}

export function getResidentBrowserWebview(browserId: string): HTMLElement | null {
  const normalizedBrowserId = trimNonEmpty(browserId);
  if (!normalizedBrowserId) {
    return null;
  }
  const resident = residentWebviewsByBrowserId.get(normalizedBrowserId) ?? null;
  if (resident?.isConnected) {
    return resident;
  }
  const ownerDocument = readDocument();
  return ownerDocument ? findBrowserWebview(normalizedBrowserId, ownerDocument) : null;
}

export function takeResidentBrowserWebview(browserId: string): HTMLElement | null {
  const normalizedBrowserId = trimNonEmpty(browserId);
  if (!normalizedBrowserId) {
    return null;
  }

  return residentWebviewsByBrowserId.get(normalizedBrowserId) ?? null;
}

export function releaseResidentBrowserWebview(browserId: string, webview: HTMLElement): void {
  const normalizedBrowserId = trimNonEmpty(browserId);
  if (!normalizedBrowserId) {
    webview.remove();
    return;
  }
  const ownerDocument = readDocument();
  if (!ownerDocument) {
    return;
  }

  residentWebviewsByBrowserId.set(normalizedBrowserId, webview);
  applyResidentWebviewStyle(webview, normalizedBrowserId);
  const surface = getBrowserSurface(normalizedBrowserId, ownerDocument);
  applyParkedBrowserSurfaceStyle(surface);
  if (webview.parentElement !== surface) {
    surface.appendChild(webview);
  }
  nativeBrowserViewBounds(webview as BrowserViewProxyElement)?.({
    container: { x: 0, y: 0, width: 1, height: 1 },
    contents: { x: 0, y: 0, width: 1, height: 1 },
    visible: false,
  });
}

export function resizeResidentBrowserWebview(input: {
  browserId: string;
  width: number;
  height: number;
}): { width: number; height: number } | null {
  const normalizedBrowserId = trimNonEmpty(input.browserId);
  if (!normalizedBrowserId) {
    return null;
  }
  const dimensions = rememberBrowserWebviewSize(input);
  if (!dimensions) {
    return null;
  }

  const ownerDocument = readDocument();
  const webview = ownerDocument ? findBrowserWebview(normalizedBrowserId, ownerDocument) : null;
  if (webview) {
    applyBrowserWebviewDimensions(webview, dimensions);
  }

  return dimensions;
}

export function removeResidentBrowserWebview(browserId: string): void {
  const normalizedBrowserId = trimNonEmpty(browserId);
  if (!normalizedBrowserId) {
    return;
  }

  const resident = residentWebviewsByBrowserId.get(normalizedBrowserId) ?? null;
  const surface = residentSurfacesByBrowserId.get(normalizedBrowserId) ?? null;
  residentWebviewsByBrowserId.delete(normalizedBrowserId);
  residentSurfacesByBrowserId.delete(normalizedBrowserId);
  residentWebviewSizesByBrowserId.delete(normalizedBrowserId);
  if (resident) {
    destroyNativeBrowserView(resident as BrowserViewProxyElement);
    resident.remove();
  }
  surface?.remove();
}

export function clearResidentBrowserWebviewsForTests(): void {
  for (const webview of residentWebviewsByBrowserId.values()) {
    destroyNativeBrowserView(webview as BrowserViewProxyElement);
    webview.remove();
  }
  residentWebviewsByBrowserId.clear();
  residentSurfacesByBrowserId.clear();
  residentWebviewSizesByBrowserId.clear();
  readDocument()?.getElementById(RESIDENT_BROWSER_HOST_ID)?.remove();
}
