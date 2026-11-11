import { supportsBrowserAccount } from "@/runtime/browser-account";
import { downloadBrowserAccountFile } from "@/runtime/daemon-transport/browser-account-relay";
import { create } from "zustand";
import { getDesktopHost } from "@/desktop/host";
import { File as FSFile, Paths } from "expo-file-system";
import * as LegacyFileSystem from "expo-file-system/legacy";
import * as Sharing from "expo-sharing";
import type { HostProfile } from "@/types/host-connection";
import { buildDaemonWebSocketUrl } from "@/utils/daemon-endpoints";
import { openExternalUrl } from "@/utils/open-external-url";
import { isWeb } from "@/constants/platform";
import { i18n } from "@/i18n/i18next";
import { Platform } from "react-native";
import { streamNativeAccountDownload } from "@/runtime/daemon-transport/native-account-download";
import type {
  AgentTimelineExportResult,
  AgentNativeHtmlExportResult,
} from "@ait/protocol/messages";
import { downloadFetch } from "@/runtime/daemon-transport/direct-download";

interface DownloadProgress {
  percent: number;
  bytesWritten: number;
  totalBytes: number;
  speed: number;
  eta: number;
}

export interface Download {
  id: string;
  serverId: string;
  scopeId: string;
  fileName: string;
  status: "downloading" | "complete" | "browser_handoff" | "error";
  message?: string;
  progress?: DownloadProgress;
  startedAt: number;
  cancel?: () => void;
}

interface DownloadState {
  downloads: Map<string, Download>;
  activeDownloadId: string | null;

  startTimelineExport: (params: {
    serverId: string;
    agentId: string;
    daemonProfile: HostProfile | undefined;
    activeConnectionId?: string | null;
    generate: () => Promise<AgentTimelineExportResult>;
    release: (token: string) => Promise<unknown>;
  }) => Promise<void>;
  startNativeHtmlExport: (params: {
    serverId: string;
    agentId: string;
    daemonProfile: HostProfile | undefined;
    activeConnectionId?: string | null;
    generate: () => Promise<AgentNativeHtmlExportResult>;
    release: (token: string) => Promise<unknown>;
  }) => Promise<void>;
  startDownload: (params: {
    serverId: string;
    scopeId: string;
    fileName: string;
    path: string;
    daemonProfile: HostProfile | undefined;
    activeConnectionId?: string | null;
    requestFileDownloadToken: (path: string) => Promise<{
      token: string | null;
      fileName: string | null;
      mimeType: string | null;
      error: string | null;
    }>;
  }) => Promise<void>;

  updateProgress: (id: string, progress: DownloadProgress) => void;
  completeDownload: (id: string) => void;
  failDownload: (id: string, message: string) => void;
  dismissDownload: (id: string) => void;
  dismissAllCompleted: () => void;
}

function generateDownloadId(): string {
  return `${Date.now()}-${Math.random().toString(36).slice(2, 9)}`;
}

export const useDownloadStore = create<DownloadState>()((set, get) => ({
  downloads: new Map(),
  activeDownloadId: null,

  startTimelineExport: (params) =>
    startGeneratedExport(set, get, { ...params, title: i18n.t("timelineExport.title") }),
  startNativeHtmlExport: (params) =>
    startGeneratedExport(set, get, { ...params, title: i18n.t("nativeHtmlExport.title") }),
  startDownload: async ({
    serverId,
    scopeId,
    fileName,
    path,
    daemonProfile,
    activeConnectionId,
    requestFileDownloadToken,
  }) => {
    const id = generateDownloadId();
    const download: Download = {
      id,
      serverId,
      scopeId,
      fileName,
      status: "downloading",
      startedAt: Date.now(),
    };

    set((state) => ({
      downloads: new Map(state.downloads).set(id, download),
      activeDownloadId: id,
    }));

    try {
      const connections = activeConnectionId
        ? daemonProfile?.connections.filter((connection) => connection.id === activeConnectionId)
        : daemonProfile?.connections;
      const relay = connections?.find((connection) => connection.type === "accountRelay");
      if (relay?.type === "accountRelay") {
        if (!getDesktopHost()?.invoke && supportsBrowserAccount()) {
          const result = await requestFileDownloadToken(path);
          if (result.error || !result.token)
            throw new Error(result.error ?? i18n.t("downloads.requestTokenFailed"));
          const started = Date.now();
          await downloadBrowserAccountFile({
            hostId: relay.hostId,
            center: relay.center,
            token: result.token,
            fileName: result.fileName ?? fileName,
            mimeType: result.mimeType,
            progress(bytesWritten, totalBytes) {
              const speed = bytesWritten / Math.max((Date.now() - started) / 1000, 0.001);
              get().updateProgress(id, {
                bytesWritten,
                totalBytes,
                percent: totalBytes > 0 ? bytesWritten / totalBytes : 0,
                speed,
                eta: speed > 0 ? Math.max(0, totalBytes - bytesWritten) / speed : 0,
              });
            },
          });
          get().completeDownload(id);
          return;
        }
        if (Platform.OS === "android" || Platform.OS === "ios") {
          const tokenResponse = await requestFileDownloadToken(path);
          if (tokenResponse.error || !tokenResponse.token) {
            throw new Error(tokenResponse.error ?? i18n.t("downloads.requestTokenFailed"));
          }
          const target = reserveDownloadTargetFile(tokenResponse.fileName ?? fileName);
          const temporary = new FSFile(`${target.uri}.${id}.part`);
          let complete = false;
          try {
            temporary.create();
            const handle = temporary.open();
            try {
              const started = Date.now();
              await streamNativeAccountDownload({
                hostId: relay.hostId,
                center: relay.center,
                token: tokenResponse.token,
                write: (bytes) => handle.writeBytes(bytes),
                progress: (bytesWritten, totalBytes) => {
                  const speed = bytesWritten / Math.max((Date.now() - started) / 1000, 0.001);
                  get().updateProgress(id, {
                    bytesWritten,
                    totalBytes,
                    percent: totalBytes > 0 ? bytesWritten / totalBytes : 0,
                    speed,
                    eta: speed > 0 ? Math.max(0, totalBytes - bytesWritten) / speed : 0,
                  });
                },
              });
            } finally {
              handle.close();
            }
            temporary.move(target);
            complete = true;
          } finally {
            if (!complete) {
              if (temporary.exists) temporary.delete();
            }
            reservedDownloadTargets.delete(target.uri);
          }
          get().completeDownload(id);
          if (await Sharing.isAvailableAsync()) {
            await Sharing.shareAsync(target.uri, {
              mimeType: tokenResponse.mimeType ?? undefined,
              dialogTitle: i18n.t("downloads.shareFileNamed", {
                fileName: tokenResponse.fileName ?? fileName,
              }),
            });
          }
          return;
        }
        const desktop = getDesktopHost();
        if (!desktop?.invoke) throw new Error("Online relay downloads require the desktop app.");
        const preparationId = await desktop.invoke("account_download_prepare", {
          hostId: relay.hostId,
          center: relay.center,
          fileName,
          downloadId: id,
        });
        if (typeof preparationId !== "string") throw new Error("Invalid download preparation.");
        try {
          const tokenResponse = await requestFileDownloadToken(path);
          if (tokenResponse.error || !tokenResponse.token) {
            throw new Error(tokenResponse.error ?? i18n.t("downloads.requestTokenFailed"));
          }
          const startedAt = Date.now();
          const remove = await desktop.events?.on?.("account-download-progress", (raw) => {
            const event = raw as { id: string; bytesWritten: number; totalBytes: number };
            if (event.id !== id) return;
            const elapsed = (Date.now() - startedAt) / 1000;
            const speed = elapsed > 0 ? event.bytesWritten / elapsed : 0;
            get().updateProgress(id, {
              bytesWritten: event.bytesWritten,
              totalBytes: event.totalBytes,
              percent: event.totalBytes > 0 ? event.bytesWritten / event.totalBytes : 0,
              speed,
              eta:
                speed > 0 && event.totalBytes > 0
                  ? (event.totalBytes - event.bytesWritten) / speed
                  : 0,
            });
          });
          try {
            await desktop.invoke("account_download", {
              preparationId,
              token: tokenResponse.token,
            });
            get().completeDownload(id);
          } finally {
            remove?.();
          }
        } finally {
          await desktop.invoke("account_download_cancel", { preparationId }).catch(() => undefined);
        }
        return;
      }

      const tokenResponse = await requestFileDownloadToken(path);
      if (tokenResponse.error || !tokenResponse.token) {
        throw new Error(tokenResponse.error ?? i18n.t("downloads.requestTokenFailed"));
      }

      const downloadTarget = resolveDaemonDownloadTarget(
        daemonProfile && connections ? { ...daemonProfile, connections } : daemonProfile,
      );
      if (!downloadTarget.baseUrl) {
        throw new Error(i18n.t("downloads.hostUnavailable"));
      }

      const resolvedFileName = tokenResponse.fileName ?? fileName;
      const downloadUrl = buildDownloadUrl(
        downloadTarget.baseUrl,
        tokenResponse.token,
        isWeb ? downloadTarget.authCredentials : null,
      );

      if (isWeb) {
        triggerBrowserDownload(downloadUrl, resolvedFileName);
        get().completeDownload(id);
        return;
      }

      const downloadStartTime = Date.now();
      const targetFile = reserveDownloadTargetFile(resolvedFileName);
      const downloadResumable = LegacyFileSystem.createDownloadResumable(
        downloadUrl,
        targetFile.uri,
        downloadTarget.authHeader
          ? { headers: { Authorization: downloadTarget.authHeader } }
          : undefined,
        (data) => {
          const now = Date.now();
          const { totalBytesWritten, totalBytesExpectedToWrite } = data;

          if (totalBytesExpectedToWrite <= 0) {
            return;
          }

          const percent = totalBytesWritten / totalBytesExpectedToWrite;
          const elapsed = (now - downloadStartTime) / 1000;
          const speed = elapsed > 0 ? totalBytesWritten / elapsed : 0;
          const remaining = totalBytesExpectedToWrite - totalBytesWritten;
          const eta = speed > 0 ? remaining / speed : 0;

          get().updateProgress(id, {
            percent,
            bytesWritten: totalBytesWritten,
            totalBytes: totalBytesExpectedToWrite,
            speed,
            eta,
          });
        },
      );

      let result: Awaited<ReturnType<typeof downloadResumable.downloadAsync>>;
      try {
        result = await downloadResumable.downloadAsync();
        if (!result) {
          throw new Error(i18n.t("downloads.cancelled"));
        }
        // The legacy downloader resolves on HTTP errors and saves the error body as the file.
        if (result.status < 200 || result.status >= 300) {
          if (targetFile.exists) targetFile.delete();
          throw new Error(`${i18n.t("downloads.failed")} (HTTP ${result.status})`);
        }
      } finally {
        reservedDownloadTargets.delete(targetFile.uri);
      }

      get().completeDownload(id);

      if (await Sharing.isAvailableAsync()) {
        await Sharing.shareAsync(result.uri, {
          mimeType: tokenResponse.mimeType ?? undefined,
          dialogTitle: resolvedFileName
            ? i18n.t("downloads.shareFileNamed", { fileName: resolvedFileName })
            : i18n.t("downloads.shareFile"),
        });
      }
    } catch (error) {
      const message = error instanceof Error ? error.message : i18n.t("downloads.failed");
      if (isWeb) {
        console.warn("[DownloadStore] Download failed:", message);
        get().failDownload(id, message);
        return;
      }
      get().failDownload(id, message);
    }
  },

  updateProgress: (id, progress) => {
    set((state) => {
      const download = state.downloads.get(id);
      if (!download || download.status !== "downloading") {
        return state;
      }
      const updated = new Map(state.downloads);
      updated.set(id, { ...download, progress });
      return { downloads: updated };
    });
  },

  completeDownload: (id) => {
    set((state) => {
      const download = state.downloads.get(id);
      if (!download) {
        return state;
      }
      const updated = new Map(state.downloads);
      updated.set(id, { ...download, status: "complete" });
      return { downloads: updated };
    });
  },

  failDownload: (id, message) => {
    set((state) => {
      const download = state.downloads.get(id);
      if (!download) {
        return state;
      }
      const updated = new Map(state.downloads);
      updated.set(id, { ...download, status: "error", message });
      return { downloads: updated };
    });
  },

  dismissDownload: (id) => {
    set((state) => {
      const updated = new Map(state.downloads);
      updated.delete(id);
      const newActiveId =
        state.activeDownloadId === id ? findMostRecentDownloadId(updated) : state.activeDownloadId;
      return { downloads: updated, activeDownloadId: newActiveId };
    });
  },

  dismissAllCompleted: () => {
    set((state) => {
      const updated = new Map(state.downloads);
      for (const [id, download] of updated) {
        if (download.status !== "downloading") {
          updated.delete(id);
        }
      }
      let newActiveId: string | null;
      if (!state.activeDownloadId) newActiveId = null;
      else if (updated.has(state.activeDownloadId)) newActiveId = state.activeDownloadId;
      else newActiveId = findMostRecentDownloadId(updated);
      return { downloads: updated, activeDownloadId: newActiveId };
    });
  },
}));

/** Download fields shared by saved-Timeline and provider-native generated artifacts. */
type GeneratedExportArtifact = Pick<
  AgentTimelineExportResult,
  "token" | "fileName" | "mimeType" | "size"
>;

type StoreSet = (
  partial:
    | Partial<DownloadState>
    | ((state: DownloadState) => Partial<DownloadState>),
) => void;

/** One bounded, cancellable, single-use generated download; no automatic regeneration. */
async function startGeneratedExport(
  set: StoreSet,
  get: () => DownloadState,
  params: {
    serverId: string;
    agentId: string;
    daemonProfile: HostProfile | undefined;
    activeConnectionId?: string | null;
    title: string;
    generate: () => Promise<GeneratedExportArtifact>;
    release: (token: string) => Promise<unknown>;
  },
): Promise<void> {
  const { serverId, agentId, daemonProfile, activeConnectionId, title, generate, release } = params;
  const id = generateDownloadId();
  const controller = new AbortController();
  const { signal } = controller;
  set((state) => ({
    downloads: new Map(state.downloads).set(id, {
      id,
      serverId,
      scopeId: agentId,
      fileName: title,
      status: "downloading",
      startedAt: Date.now(),
      cancel: () => controller.abort(),
    }),
    activeDownloadId: id,
  }));
  let artifact: GeneratedExportArtifact | undefined;
  let transferred = false;
  let releasing: Promise<unknown> | undefined;
  const releaseArtifact = () => {
    if (artifact && !transferred) releasing ??= release(artifact.token).catch(() => undefined);
  };
  // Release immediately even if Electron is still waiting on its save dialog.
  signal.addEventListener("abort", releaseArtifact, { once: true });
  try {
    // Exactly one generation per explicit click, including failures and expired tokens.
    artifact = await generate();
    signal.throwIfAborted();
    if (artifact.size > GENERATED_EXPORT_MAX_BYTES)
      throw new Error(i18n.t("timelineExport.tooLarge"));
    set((state) => {
      const download = state.downloads.get(id);
      return download
        ? {
            downloads: new Map(state.downloads).set(id, {
              ...download,
              fileName: artifact!.fileName,
            }),
          }
        : state;
    });
    const started = Date.now();
    const progress = (bytesWritten: number, totalBytes: number) => {
      const speed = bytesWritten / Math.max((Date.now() - started) / 1000, 0.001);
      get().updateProgress(id, {
        bytesWritten,
        totalBytes,
        speed,
        percent: totalBytes > 0 ? bytesWritten / totalBytes : 0,
        eta: speed > 0 ? Math.max(0, totalBytes - bytesWritten) / speed : 0,
      });
    };
    const outcome = await downloadGeneratedArtifact({
      id,
      artifact,
      daemonProfile,
      activeConnectionId,
      signal,
      progress,
    });
    signal.throwIfAborted();
    transferred = true;
    set((state) => {
      const download = state.downloads.get(id);
      return download
        ? { downloads: new Map(state.downloads).set(id, { ...download, status: outcome }) }
        : state;
    });
  } catch (error) {
    get().failDownload(
      id,
      signal.aborted
        ? i18n.t("downloads.cancelled")
        : error instanceof Error
          ? error.message
          : i18n.t("downloads.failed"),
    );
  } finally {
    signal.removeEventListener("abort", releaseArtifact);
    if (!transferred) releaseArtifact();
    await releasing;
  }
}

const GENERATED_EXPORT_MAX_BYTES = 16 * 1024 * 1024;

/** Browser handoff confirms received bytes, not the browser's eventual disk save. */
type GeneratedDownloadOutcome = "complete" | "browser_handoff";

async function downloadGeneratedArtifact(input: {
  id: string;
  artifact: GeneratedExportArtifact;
  daemonProfile: HostProfile | undefined;
  activeConnectionId?: string | null;
  signal: AbortSignal;
  progress(bytesWritten: number, totalBytes: number): void;
}): Promise<GeneratedDownloadOutcome> {
  const { artifact, signal, progress } = input;
  const connections = input.activeConnectionId
    ? input.daemonProfile?.connections.filter(
        (connection) => connection.id === input.activeConnectionId,
      )
    : input.daemonProfile?.connections;
  const relay = connections?.find((connection) => connection.type === "accountRelay");
  const direct = resolveDaemonDownloadTarget(
    input.daemonProfile && connections
      ? { ...input.daemonProfile, connections }
      : input.daemonProfile,
  );
  const desktop = getDesktopHost();
  const invoke = desktop?.invoke;
  if (desktop && invoke) {
    const preparationId = await invoke("account_download_prepare", {
      hostId: relay?.type === "accountRelay" ? relay.hostId : "",
      center: relay?.type === "accountRelay" ? relay.center : "",
      ...(relay ? {} : { direct: { baseUrl: direct.baseUrl, authHeader: direct.authHeader } }),
      fileName: artifact.fileName,
      downloadId: input.id,
      maxBytes: GENERATED_EXPORT_MAX_BYTES,
      expectedSize: artifact.size,
    });
    if (typeof preparationId !== "string") throw new Error("Invalid download preparation.");
    const cancel = () => {
      void invoke("account_download_cancel", { preparationId }).catch(() => undefined);
    };
    signal.addEventListener("abort", cancel, { once: true });
    let remove: (() => void) | undefined;
    try {
      remove = await desktop.events?.on?.("account-download-progress", (raw) => {
        const event = raw as { id: string; bytesWritten: number; totalBytes: number };
        if (event.id === input.id) progress(event.bytesWritten, event.totalBytes);
      });
      signal.throwIfAborted();
      await invoke("account_download", { preparationId, token: artifact.token });
    } finally {
      signal.removeEventListener("abort", cancel);
      remove?.();
      await invoke("account_download_cancel", { preparationId }).catch(() => undefined);
    }
    return "complete";
  }
  if (relay?.type === "accountRelay" && supportsBrowserAccount()) {
    await downloadBrowserAccountFile({
      hostId: relay.hostId,
      center: relay.center,
      token: artifact.token,
      fileName: artifact.fileName,
      mimeType: artifact.mimeType,
      signal,
      maxBytes: GENERATED_EXPORT_MAX_BYTES,
      expectedSize: artifact.size,
      progress,
    });
    return "browser_handoff";
  }
  const chunks: Uint8Array<ArrayBuffer>[] = [];
  const target = isWeb ? null : reserveDownloadTargetFile(artifact.fileName);
  const temporary = target ? new FSFile(`${target.uri}.${input.id}.part`) : null;
  let saved = false;
  let temporaryCreated = false;
  let moved = false;
  try {
    if (temporary) {
      temporary.create();
      temporaryCreated = true;
    }
    const handle = temporary?.open();
    try {
      const write = (bytes: Uint8Array) => {
        if (handle) handle.writeBytes(bytes);
        else
          chunks.push(
            bytes.buffer instanceof ArrayBuffer
              ? new Uint8Array(bytes.buffer, bytes.byteOffset, bytes.byteLength)
              : Uint8Array.from(bytes),
          );
      };
      if (relay?.type === "accountRelay") {
        await streamNativeAccountDownload({
          hostId: relay.hostId,
          center: relay.center,
          token: artifact.token,
          signal,
          maxBytes: GENERATED_EXPORT_MAX_BYTES,
          expectedSize: artifact.size,
          write,
          progress,
        });
      } else {
        if (!direct.baseUrl) throw new Error(i18n.t("downloads.hostUnavailable"));
        await streamGeneratedDownload(
          buildDownloadUrl(direct.baseUrl, artifact.token, null),
          direct.authHeader,
          signal,
          artifact.size,
          write,
          progress,
        );
      }
    } finally {
      handle?.close();
    }
    signal.throwIfAborted();
    if (temporary && target) {
      if (target.exists) throw new Error(i18n.t("downloads.failed"));
      temporary.move(target);
      moved = true;
      if (await Sharing.isAvailableAsync()) {
        signal.throwIfAborted();
        await Sharing.shareAsync(target.uri, {
          mimeType: artifact.mimeType,
          dialogTitle: i18n.t("downloads.shareFileNamed", { fileName: artifact.fileName }),
        });
      }
    } else {
      const url = URL.createObjectURL(new Blob(chunks, { type: artifact.mimeType }));
      try {
        triggerBrowserDownload(url, artifact.fileName);
      } finally {
        setTimeout(() => URL.revokeObjectURL(url), 60_000);
      }
    }
    signal.throwIfAborted();
    saved = true;
    return target ? "complete" : "browser_handoff";
  } finally {
    if (!saved) {
      if (moved && target?.exists) target.delete();
      else if (temporaryCreated && temporary?.exists) temporary.delete();
    }
    if (target) {
      reservedDownloadTargets.delete(target.uri);
    }
  }
}

async function streamGeneratedDownload(
  url: string,
  authHeader: string | null,
  signal: AbortSignal,
  expectedSize: number,
  write: (bytes: Uint8Array) => void,
  progress: (bytes: number, total: number) => void,
): Promise<void> {
  const response = await downloadFetch(url, {
    signal,
    redirect: "error",
    credentials: "omit",
    headers: authHeader ? { Authorization: authHeader } : undefined,
  });
  if (!response.ok || !response.body) {
    await response.body?.cancel();
    throw new Error(`${i18n.t("downloads.failed")} (HTTP ${response.status})`);
  }
  const reader = response.body.getReader();
  let bytes = 0;
  try {
    const length = Number(response.headers.get("content-length"));
    if (length > GENERATED_EXPORT_MAX_BYTES) throw new Error(i18n.t("timelineExport.tooLarge"));
    while (true) {
      signal.throwIfAborted();
      const chunk = await reader.read();
      if (chunk.done) break;
      bytes += chunk.value.byteLength;
      if (bytes > GENERATED_EXPORT_MAX_BYTES) throw new Error(i18n.t("timelineExport.tooLarge"));
      write(chunk.value);
      progress(bytes, expectedSize);
    }
    if (bytes !== expectedSize) throw new Error(i18n.t("downloads.failed"));
  } finally {
    await reader.cancel().catch(() => undefined);
    reader.releaseLock();
  }
}

function findMostRecentDownloadId(downloads: Map<string, Download>): string | null {
  let mostRecent: Download | null = null;
  for (const download of downloads.values()) {
    if (!mostRecent || download.startedAt > mostRecent.startedAt) {
      mostRecent = download;
    }
  }
  return mostRecent?.id ?? null;
}

interface DownloadTarget {
  baseUrl: string | null;
  authHeader: string | null;
  authCredentials: { username: string; password: string } | null;
}

function resolveDaemonDownloadTarget(daemon?: HostProfile): DownloadTarget {
  const connection = daemon?.connections.find((conn) => conn.type === "directTcp") ?? null;
  if (!connection) {
    return { baseUrl: null, authHeader: null, authCredentials: null };
  }

  let parsed: URL;
  try {
    parsed = new URL(
      buildDaemonWebSocketUrl(connection.endpoint, { useTls: connection.useTls ?? false }),
    );
  } catch {
    return { baseUrl: null, authHeader: null, authCredentials: null };
  }

  if (parsed.protocol === "ws:") {
    parsed.protocol = "http:";
  } else if (parsed.protocol === "wss:") {
    parsed.protocol = "https:";
  }

  let authCredentials: { username: string; password: string } | null = null;
  if (parsed.username || parsed.password) {
    authCredentials = {
      username: decodeURIComponent(parsed.username),
      password: decodeURIComponent(parsed.password),
    };
    parsed.username = "";
    parsed.password = "";
  }

  parsed.pathname = parsed.pathname.replace(/\/ws\/?$/, "/");

  const baseUrl = parsed.origin;
  const authHeader = connection.password
    ? `Bearer ${connection.password}`
    : authCredentials
      ? `Basic ${btoa(`${authCredentials.username}:${authCredentials.password}`)}`
      : null;

  return { baseUrl, authHeader, authCredentials };
}

function buildDownloadUrl(
  baseUrl: string,
  token: string,
  authCredentials: { username: string; password: string } | null,
): string {
  const url = new URL("/api/files/download", baseUrl);
  url.searchParams.set("token", token);
  if (authCredentials) {
    url.username = authCredentials.username;
    url.password = authCredentials.password;
  }
  return url.toString();
}

function triggerBrowserDownload(url: string, fileName: string) {
  if (typeof document === "undefined") {
    if (typeof window !== "undefined") {
      void openExternalUrl(url);
    }
    return;
  }

  const link = document.createElement("a");
  link.href = url;
  link.download = fileName;
  link.rel = "noopener";
  document.body.appendChild(link);
  link.click();
  link.remove();
}

/** Target paths claimed by native downloads that have not finished writing yet. */
const reservedDownloadTargets = new Set<string>();

/** Pick an unused target path and claim it until the caller releases the reservation. */
function reserveDownloadTargetFile(fileName: string): FSFile {
  const directory = Paths.cache ?? Paths.document;
  if (!directory) {
    throw new Error("No download directory available.");
  }

  const safeName = sanitizeDownloadFileName(fileName);
  const split = splitFileName(safeName);
  let targetFile = new FSFile(directory, safeName);
  let suffix = 1;

  while (targetFile.exists || reservedDownloadTargets.has(targetFile.uri)) {
    targetFile = new FSFile(directory, `${split.base} (${suffix})${split.ext}`);
    suffix += 1;
  }

  reservedDownloadTargets.add(targetFile.uri);
  return targetFile;
}

function sanitizeDownloadFileName(fileName: string): string {
  const trimmed = fileName.trim();
  if (!trimmed) {
    return "download";
  }
  return trimmed.replace(/[\\/:*?"<>|]+/g, "_");
}

function splitFileName(fileName: string): { base: string; ext: string } {
  const lastDot = fileName.lastIndexOf(".");
  if (lastDot <= 0) {
    return { base: fileName, ext: "" };
  }
  return {
    base: fileName.slice(0, lastDot),
    ext: fileName.slice(lastDot),
  };
}

export function formatSpeed(bytesPerSecond: number): string {
  if (bytesPerSecond < 1024) {
    return `${Math.round(bytesPerSecond)} B/s`;
  }
  if (bytesPerSecond < 1024 * 1024) {
    return `${(bytesPerSecond / 1024).toFixed(1)} KB/s`;
  }
  return `${(bytesPerSecond / (1024 * 1024)).toFixed(1)} MB/s`;
}

export function formatEta(seconds: number): string {
  if (seconds < 1) {
    return "< 1s";
  }
  if (seconds < 60) {
    return `${Math.round(seconds)}s`;
  }
  const mins = Math.floor(seconds / 60);
  const secs = Math.round(seconds % 60);
  return `${mins}m ${secs}s`;
}
