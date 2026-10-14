// Metro selects terminal-input.ios.tsx / terminal-input.native.tsx on devices.
// Use the same extension so a default .ts cannot shadow the .ios.tsx file.
// Keep the default entry available to TypeScript and non-native tooling.
export { TerminalInput } from "./terminal-input.native";
export type { TerminalInputHandle } from "./terminal-input.native";
