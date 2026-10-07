import { afterEach, expect, it, vi } from "vitest";
import { ClientMessageChunks } from "./chunks";
import type { Transport } from "./types";

afterEach(() => vi.useRealTimers());

it("reassembles acknowledged UTF-8 chunks without changing bytes", () => {
  const sent: Uint8Array[] = [];
  const channel = { send: vi.fn((frame: Uint8Array) => sent.push(frame)) } as unknown as Transport;
  const sender = new ClientMessageChunks([channel], vi.fn());
  const text = JSON.stringify({ text: "图片".repeat(200_000) });
  sender.send(0, text, true);
  expect(sent).toHaveLength(1);
  const pieces: Uint8Array[] = [];
  let offset = 0;
  while (sent.length) {
    const frame = sent.shift()!;
    const header = new DataView(frame.buffer);
    expect(header.getUint32(9)).toBe(offset);
    expect(frame.length).toBeLessThanOrEqual(256 * 1024 + 13);
    pieces.push(frame.subarray(13));
    offset += frame.length - 13;
    sender.acknowledge(0, header.getUint32(1), offset);
  }
  const assembled = new Uint8Array(offset);
  let position = 0;
  for (const piece of pieces) {
    assembled.set(piece, position);
    position += piece.length;
  }
  expect(new TextDecoder().decode(assembled)).toBe(text);
  sender.dispose();
});

it("rejects unsupported Hosts and concurrent uploads without sending, then cleans up", () => {
  vi.useFakeTimers();
  const channel = { send: vi.fn() } as unknown as Transport;
  const fail = vi.fn();
  const sender = new ClientMessageChunks([channel], fail);
  const wire = "x".repeat(1024 * 1024 + 1);
  expect(() => sender.send(0, wire, false)).toThrow("Update the Host");
  expect(channel.send).not.toHaveBeenCalled();
  sender.send(0, wire, true);
  expect(() => sender.send(0, wire, true)).toThrow("still uploading");
  expect(() => sender.acknowledge(0, 999, 1)).toThrow("acknowledgement");
  sender.dispose();
  vi.advanceTimersByTime(20_000);
  expect(fail).not.toHaveBeenCalled();
});

it("reports stalled uploads and preserves small messages", () => {
  vi.useFakeTimers();
  const channel = { send: vi.fn() } as unknown as Transport;
  const fail = vi.fn();
  const sender = new ClientMessageChunks([channel], fail);
  sender.send(0, "small", false);
  expect(channel.send).toHaveBeenCalledWith("small");
  sender.send(0, "x".repeat(1024 * 1024 + 1), true);
  vi.advanceTimersByTime(15_000);
  expect(fail).toHaveBeenCalledOnce();
  sender.dispose();
});
