import { decodeFileTransferFrame } from "@ait/protocol/binary-frames/index";
import type { Transport } from "./types";

/** Hold the uploader until the daemon has processed each file frame. */
export class UploadAcknowledgements {
  private pending: {
    requestId: string;
    opcode: number;
    promise: Promise<void>;
    resolve: () => void;
    reject: (error: Error) => void;
    timer: ReturnType<typeof setTimeout>;
  } | null = null;

  constructor(private readonly fail: (error: Error) => void) {}

  send(transport: Transport, bytes: Uint8Array): void {
    if (this.pending) throw new Error("A file upload frame is still awaiting acknowledgement");
    const frame = decodeFileTransferFrame(bytes);
    if (!frame) throw new Error("Invalid upload frame");
    let resolve!: () => void;
    let reject!: (error: Error) => void;
    const promise = new Promise<void>((yes, no) => {
      resolve = yes;
      reject = no;
    });
    void promise.catch(() => {});
    const timer = setTimeout(
      () => this.fail(new Error("File upload acknowledgement timed out")),
      30_000,
    );
    this.pending = {
      requestId: frame.requestId,
      opcode: frame.opcode,
      promise,
      resolve,
      reject,
      timer,
    };
    const wire = bytes.slice();
    wire[0] += 0x30;
    try {
      transport.send(wire);
    } catch (error) {
      this.dispose();
      throw error;
    }
  }

  acknowledge(requestId: unknown, opcode: unknown): void {
    const pending = this.pending;
    if (!pending || pending.requestId !== requestId || pending.opcode !== opcode)
      throw new Error("Invalid upload acknowledgement");
    clearTimeout(pending.timer);
    this.pending = null;
    pending.resolve();
  }

  drain(): Promise<void> {
    return this.pending?.promise ?? Promise.resolve();
  }

  dispose(): void {
    if (!this.pending) return;
    clearTimeout(this.pending.timer);
    this.pending.reject(new Error("Connection closed during file upload"));
    this.pending = null;
  }
}
