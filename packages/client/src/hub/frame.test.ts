import { describe, expect, it } from "vitest";
import { decodeHubFrame, encodeHubFrame, HUB_MAX_PAYLOAD } from "./frame.js";

function raw(header: string, payload: number[] = []): Uint8Array {
  const json = new TextEncoder().encode(header);
  return new Uint8Array([json.byteLength >> 8, json.byteLength & 0xff, ...json, ...payload]);
}

describe("ait-hub-v1 frame codec", () => {
  it("prefixes the JSON header with its big-endian length and keeps payload bytes", () => {
    const frame = encodeHubFrame(
      { t: "msg", seq: 7, s: "session", pt: "binary" },
      new Uint8Array([0, 255, 1]),
    );
    const length = (frame[0] << 8) | frame[1];
    expect(JSON.parse(new TextDecoder().decode(frame.subarray(2, 2 + length)))).toEqual({
      t: "msg",
      seq: 7,
      s: "session",
      pt: "binary",
    });
    expect([...frame.subarray(2 + length)]).toEqual([0, 255, 1]);
    expect(decodeHubFrame(frame.buffer as ArrayBuffer).payload).toEqual(
      new Uint8Array([0, 255, 1]),
    );
  });

  it("decodes center frames from ArrayBuffers", () => {
    const frame = raw('{"t":"accept","seq":2,"s":"session"}', [104, 105]);
    const decoded = decodeHubFrame(frame.buffer as ArrayBuffer);
    expect(decoded.header).toEqual({ t: "accept", seq: 2, s: "session" });
    expect(new TextDecoder().decode(decoded.payload)).toBe("hi");
  });

  it("ignores unknown frame kinds but rejects malformed known ones", () => {
    expect(decodeHubFrame(raw('{"t":"future","x":1}')).header.t).toBe("future");
    expect(() => decodeHubFrame(new Uint8Array([0]))).toThrow("Truncated");
    expect(() => decodeHubFrame(raw("[]"))).toThrow("Invalid hub frame header");
    expect(() => decodeHubFrame(raw('{"t":"msg","seq":-1,"s":"x","pt":"text"}'))).toThrow();
    expect(() => decodeHubFrame(raw('{"t":"msg","seq":1,"s":"x","pt":"json"}'))).toThrow();
    expect(() => decodeHubFrame(raw('{"t":"welcome","link":"l"}'))).toThrow();
    const oversized = new Uint8Array(5002);
    oversized.set([0x13, 0x88]);
    expect(() => decodeHubFrame(oversized)).toThrow("header length");
  });

  it("refuses payloads above the center limit", () => {
    expect(() =>
      encodeHubFrame(
        { t: "msg", seq: 1, s: "s", pt: "binary" },
        new Uint8Array(HUB_MAX_PAYLOAD + 1),
      ),
    ).toThrow("size limit");
  });
});
