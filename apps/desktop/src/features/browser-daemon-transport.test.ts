import { EventEmitter, once } from "node:events";
import { afterEach, expect, test, vi } from "vitest";
import { WebSocketServer } from "ws";
import { BrowserDaemonTransport } from "./browser-daemon-transport.js";

const broadcast = vi.hoisted(() => vi.fn());
vi.mock("electron", () => ({ BrowserWindow: { getAllWindows: broadcast } }));

const cleanup: (() => void)[] = [];
afterEach(() => {
  for (const close of cleanup.splice(0)) close();
  vi.clearAllMocks();
});

test("authenticates direct daemon traffic and keeps responses in the main process", async () => {
  const server = new WebSocketServer({ host: "127.0.0.1", port: 0 });
  await once(server, "listening");
  cleanup.push(() => server.close());
  const address = server.address();
  if (typeof address === "string") throw new Error("Expected TCP listener");
  const token = "fixture-token".repeat(3);
  const transport = new BrowserDaemonTransport({
    target: { transportType: "rustTcp", url: `ws://127.0.0.1:${address.port}/v1/ws` },
    bearerToken: token,
  });
  const events = new EventEmitter();
  const owner = Object.assign(new EventEmitter(), {
    isDestroyed: () => false,
    send: (_channel: string, payload: Record<string, unknown>) =>
      events.emit(String(payload.kind), payload),
  });
  cleanup.push(() => transport.close(owner, "browser-test"));
  const connected = once(server, "connection");
  const opened = once(events, "open");
  await transport.open(owner, "browser-test");
  const [socket, request] = await connected;
  cleanup.push(() => socket.terminate());
  await opened;
  expect(request.headers.authorization).toBe(`Bearer ${token}`);
  expect(request.url).toBe("/v1/ws");
  const received = once(socket, "message");
  await transport.send(owner, "browser-test", { text: "hello" });
  expect((await received)[0].toString()).toBe("hello");
  const response = once(events, "message");
  socket.send("daemon-response");
  expect((await response)[0].text).toBe("daemon-response");
  expect(broadcast).not.toHaveBeenCalled();
  const closed = once(socket, "close");
  transport.close(owner, "browser-test");
  await closed;
});
