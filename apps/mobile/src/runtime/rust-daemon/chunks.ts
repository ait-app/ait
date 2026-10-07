import type { Transport } from "./types";

const MESSAGE_BYTES = 1024 * 1024;
const ASSEMBLED_BYTES = 64 * 1024 * 1024;
const CHUNK_BYTES = 256 * 1024;

interface Transfer {
  id: number;
  bytes: Uint8Array;
  offset: number;
  timer?: ReturnType<typeof setTimeout>;
}

/** One acknowledged chunk at a time per physical channel; never replay on disconnect. */
export class ClientMessageChunks {
  private readonly transfers = new Map<number, Transfer>();
  private nextId = 1;

  constructor(
    private readonly channels: Transport[],
    private readonly fail: (error: Error) => void,
  ) {}

  send(channel: number, wire: string, supported: boolean): void {
    const bytes = new TextEncoder().encode(wire);
    if (bytes.length <= MESSAGE_BYTES) {
      this.channels[channel].send(wire);
      return;
    }
    if (!supported)
      throw new Error(
        "This Host does not support large messages. Update the Host before sending these attachments.",
      );
    if (bytes.length > ASSEMBLED_BYTES)
      throw new Error(
        "Image message exceeds 64 MiB. Send fewer images per message; upload large documents as files.",
      );
    if (this.transfers.has(channel))
      throw new Error("Another large message is still uploading on this connection.");
    const transfer: Transfer = { id: this.nextId++ >>> 0, bytes, offset: 0 };
    this.transfers.set(channel, transfer);
    try {
      this.write(channel, transfer);
    } catch (error) {
      this.clear(channel);
      throw error;
    }
  }

  acknowledge(channel: number, id: unknown, offset: unknown): void {
    const transfer = this.transfers.get(channel);
    if (!transfer || id !== transfer.id || offset !== transfer.offset)
      throw new Error("Invalid message chunk acknowledgement");
    clearTimeout(transfer.timer);
    if (transfer.offset === transfer.bytes.length) this.clear(channel);
    else this.write(channel, transfer);
  }

  dispose(): void {
    for (const channel of this.transfers.keys()) this.clear(channel);
  }

  private clear(channel: number): void {
    clearTimeout(this.transfers.get(channel)?.timer);
    this.transfers.delete(channel);
  }

  private write(channel: number, transfer: Transfer): void {
    const end = Math.min(transfer.offset + CHUNK_BYTES, transfer.bytes.length);
    const frame = new Uint8Array(13 + end - transfer.offset);
    const header = new DataView(frame.buffer);
    header.setUint8(0, 0x30);
    header.setUint32(1, transfer.id);
    header.setUint32(5, transfer.bytes.length);
    header.setUint32(9, transfer.offset);
    frame.set(transfer.bytes.subarray(transfer.offset, end), 13);
    transfer.offset = end;
    transfer.timer = setTimeout(
      () => this.fail(new Error("Message upload acknowledgement timed out")),
      15_000,
    );
    this.channels[channel].send(frame);
  }
}
