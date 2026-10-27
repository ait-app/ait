import { useEffect } from "react";
import { View } from "react-native";
import { StyleSheet } from "react-native-unistyles";
import { useAppSettings } from "../hooks/use-settings";
import { resolveTerminalRendererSelection } from "../terminal/native-renderer/terminal-renderer-capability";
import NativeGridTerminalEmulator from "./terminal-emulator-native-grid.native";
import WebViewTerminalEmulator from "./terminal-emulator-webview.native";
import type { TerminalEmulatorProps } from "./terminal-emulator-contract";

export type { TerminalEmulatorHandle } from "./terminal-emulator-contract";

function UnavailableNativeTerminal({ streamKey, onRendererReadyChange }: TerminalEmulatorProps) {
  useEffect(() => {
    onRendererReadyChange?.({ streamKey, isReady: false });
  }, [onRendererReadyChange, streamKey]);

  // TerminalPane owns connection and capability status messages.
  return <View style={styles.unavailableContainer} />;
}

export default function TerminalEmulator(props: TerminalEmulatorProps) {
  const { settings } = useAppSettings();
  const { useLegacyTerminalRenderer } = settings;
  const rendererSelection = resolveTerminalRendererSelection({
    useLegacyTerminalRenderer,
    supportsTerminalInputModeReplay: props.supportsTerminalInputModeReplay,
  });

  if (rendererSelection === "update-host") {
    return <UnavailableNativeTerminal {...props} />;
  }

  const Renderer =
    rendererSelection === "webview" ? WebViewTerminalEmulator : NativeGridTerminalEmulator;

  return <Renderer key={rendererSelection} {...props} />;
}

const styles = StyleSheet.create({
  unavailableContainer: {
    flex: 1,
  },
});
