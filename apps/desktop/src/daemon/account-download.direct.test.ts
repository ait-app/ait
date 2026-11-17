import { EventEmitter, once } from "node:events";
import { createServer, type ServerResponse } from "node:http";
import { mkdtemp, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { dialog, type WebContents } from "electron";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { AccountDownloadManager } from "./account-download.js";
import type { AccountSessionManager } from "./account-session.js";

// Only Electron's UI boundary is substituted: HTTP and partial/final files are real.
vi.mock("electron", () => ({
  BrowserWindow: { fromWebContents: () => ({}) },
  dialog: { showSaveDialog: vi.fn() },
}));
let directory: string;
let response: ServerResponse;
let requests: Array<{ url: string | undefined; authorization: string | undefined }>;
let declaredLength: number | undefined;
const server = createServer((request, outgoing) => {
  requests.push({ url: request.url, authorization: request.headers.authorization });
  response = outgoing;
  outgoing.writeHead(200, {
    "Content-Type": "text/html",
    ...(declaredLength === undefined ? {} : { "Content-Length": declaredLength }),
  });
  outgoing.write("<p>");
});
let manager: AccountDownloadManager;
let owner: WebContents;
let preparationId: string;

beforeEach(async () => {
  requests = [];
  declaredLength = undefined;
  directory = await mkdtemp(path.join(tmpdir(), "ait-export-download-"));
  vi.mocked(dialog.showSaveDialog).mockResolvedValue({
    canceled: false,
    filePath: path.join(directory, "session.html"),
  });
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  const address = server.address();
  if (!address || typeof address === "string") throw new Error("No fixture port");
  owner = Object.assign(new EventEmitter(), {
    isDestroyed: () => false,
    send: vi.fn(),
  }) as unknown as WebContents;
  manager = new AccountDownloadManager({} as AccountSessionManager);
  preparationId = await manager.prepare(owner, {
    hostId: "",
    center: "",
    fileName: "session.html",
    downloadId: "export",
    direct: { baseUrl: `http://127.0.0.1:${address.port}`, authHeader: "Bearer private" },
    maxBytes: 16 * 1024 * 1024,
  });
});
afterEach(async () => {
  manager.closeAll();
  server.closeAllConnections();
  const closed = once(server, "close");
  server.close();
  await closed;
  await rm(directory, { recursive: true, force: true });
});

it("saves exact streamed bytes only after EOF and keeps credentials out of the URL", async () => {
  const transfer = manager.download(owner, { preparationId, token: "one-use" });
  await vi.waitFor(() => expect(owner.send).toHaveBeenCalled());
  expect(await readdir(directory)).toEqual([expect.stringMatching(/\.part$/)]);
  response.end("&lt;safe&gt;</p>");
  await transfer;
  expect(await readFile(path.join(directory, "session.html"), "utf8")).toBe("<p>&lt;safe&gt;</p>");
  expect(await readdir(directory)).toEqual(["session.html"]);
  expect(requests).toEqual([
    { url: "/api/files/download?token=one-use", authorization: "Bearer private" },
  ]);
  expect(dialog.showSaveDialog).toHaveBeenLastCalledWith(expect.anything(), {
    defaultPath: "session.html",
  });
});

it("cancels an in-flight HTTP body and removes the real partial file", async () => {
  const transfer = manager.download(owner, { preparationId, token: "one-use" });
  const failure = expect(transfer).rejects.toThrow();
  await vi.waitFor(() => expect(owner.send).toHaveBeenCalled());
  manager.cancel(owner, preparationId);
  await failure;
  expect(await readdir(directory)).toEqual([]);
  expect(requests).toHaveLength(1);
});

it("rejects a stream exceeding 16 MiB without retaining a truncated file", async () => {
  const transfer = manager.download(owner, { preparationId, token: "one-use" });
  const failure = expect(transfer).rejects.toThrow("limit");
  await vi.waitFor(() => expect(owner.send).toHaveBeenCalled());
  response.end(Buffer.alloc(16 * 1024 * 1024));
  await failure;
  expect(await readdir(directory)).toEqual([]);
  expect(requests).toHaveLength(1);
});

it.each([undefined, 3])(
  "rejects short receipt-bound HTTP with content length %s and preserves the destination",
  async (length) => {
    declaredLength = length;
    manager.cancel(owner, preparationId);
    const address = server.address();
    if (!address || typeof address === "string") throw new Error("No fixture port");
    preparationId = await manager.prepare(owner, {
      hostId: "",
      center: "",
      fileName: "session.html",
      downloadId: "export",
      direct: { baseUrl: `http://127.0.0.1:${address.port}`, authHeader: "Bearer private" },
      maxBytes: 16 * 1024 * 1024,
      expectedSize: 18,
    });
    const destination = path.join(directory, "session.html");
    await writeFile(destination, "existing");
    const transfer = manager.download(owner, { preparationId, token: "once" });
    const failed = expect(transfer).rejects.toThrow("File size mismatch");
    await vi.waitFor(() => expect(requests).toHaveLength(1));
    response.end();
    await failed;
    expect(await readFile(destination, "utf8")).toBe("existing");
    expect(await readdir(directory)).toEqual(["session.html"]);
  },
);
