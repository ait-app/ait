import { expect, it, vi } from "vitest";
import { encodeFileTransferFrame, FileTransferOpcode } from "@ait/protocol/binary-frames/index";
import { UploadAcknowledgements } from "./upload-acks";
import type { Transport } from "./types";

it("waits for matching acknowledgements and rejects disconnected transfers", async () => {
  const channel = { send: vi.fn() } as unknown as Transport;
  const uploads = new UploadAcknowledgements(vi.fn());
  const frame = encodeFileTransferFrame({
    opcode: FileTransferOpcode.FileChunk,
    requestId: "upload",
    payload: new Uint8Array([1, 2]),
  });
  uploads.send(channel, frame);
  expect(vi.mocked(channel.send).mock.calls[0][0]).toMatchObject({ 0: 0x41 });
  expect(frame[0]).toBe(0x11);
  let done = false;
  const waiting = uploads.drain().then(() => {
    done = true;
  });
  await Promise.resolve();
  expect(done).toBe(false);
  expect(() => uploads.acknowledge("wrong", 0x11)).toThrow("acknowledgement");
  uploads.acknowledge("upload", 0x11);
  await waiting;
  expect(done).toBe(true);
  uploads.send(channel, frame);
  const disconnected = uploads.drain();
  uploads.dispose();
  await expect(disconnected).rejects.toThrow("Connection closed");
});
