import type { TerminalInputModeTracker } from "@ait/protocol/terminal-input-mode";
import type { TerminalOutputData } from "../runtime/terminal-emulator-runtime";

/**
 * Decodes a daemon restore frame for the native grid and re-derives the input modes from it.
 *
 * A restore replaces the screen, so bytes left in the decoder by the interrupted output stream
 * are dropped, and the tracker starts over from the frame. Restore frames end with the screen's
 * input modes, so bracketed paste and cursor-key modes survive reconnects and remounts; a frame
 * without them clears modes left over from before. Matches the snapshot path of
 * `TerminalEmulatorRuntime.restoreOutput`.
 */
export function decodeNativeTerminalRestore(input: {
  decoder: TextDecoder;
  inputModeTracker: TerminalInputModeTracker;
  data: TerminalOutputData;
}): string {
  input.decoder.decode();
  const text = input.decoder.decode(input.data, { stream: false });
  input.inputModeTracker.reset();
  input.inputModeTracker.feed(text);
  return text;
}
