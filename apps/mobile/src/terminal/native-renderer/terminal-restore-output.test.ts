import { describe, expect, it } from "vitest";
import { TerminalInputModeTracker } from "@ait/protocol/terminal-input-mode";
import { decodeNativeTerminalRestore } from "./terminal-restore-output";

const encoder = new TextEncoder();

function restore(decoder: TextDecoder, tracker: TerminalInputModeTracker, text: string): string {
  return decodeNativeTerminalRestore({
    decoder,
    inputModeTracker: tracker,
    data: encoder.encode(text),
  });
}

describe("decodeNativeTerminalRestore", () => {
  it("re-seeds input modes from the restore frame instead of leaving them reset", () => {
    const decoder = new TextDecoder();
    const tracker = new TerminalInputModeTracker();

    // Daemon restore frames append the screen's input modes after the screen contents.
    const text = restore(decoder, tracker, "restored screen\x1b[?1h\x1b[?2004h");

    expect(text).toBe("restored screen\x1b[?1h\x1b[?2004h");
    expect(tracker.getState()).toMatchObject({
      applicationCursorKeys: true,
      bracketedPaste: true,
    });
  });

  it("clears modes from before the restore when the frame does not set them", () => {
    const decoder = new TextDecoder();
    const tracker = new TerminalInputModeTracker();
    tracker.feed("\x1b[?1h\x1b[?2004h\x1b[>1u");

    restore(decoder, tracker, "plain screen");

    expect(tracker.getState()).toMatchObject({
      applicationCursorKeys: false,
      bracketedPaste: false,
      kittyKeyboardFlags: 0,
    });
  });

  it("drops a partial sequence left by the interrupted output stream", () => {
    const decoder = new TextDecoder();
    const tracker = new TerminalInputModeTracker();
    // Half of a UTF-8 "€" and half of a bracketed-paste sequence from the live stream.
    decoder.decode(new Uint8Array([0xe2, 0x82]), { stream: true });
    tracker.feed("\x1b[?20");

    const text = restore(decoder, tracker, "04h ok");

    expect(text).toBe("04h ok");
    expect(tracker.getState().bracketedPaste).toBe(false);
  });
});
