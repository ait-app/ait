import React, { forwardRef, useImperativeHandle, useRef, type RefAttributes } from "react";
import { StyleSheet, type NativeSyntheticEvent, type ViewProps } from "react-native";
import { requireNativeViewManager } from "expo-modules-core";
import type { TerminalInputHandle, TerminalInputProps } from "./terminal-input.native";
import { resolveNativeTerminalKey } from "./terminal-key-events";

interface NativeInputHandle {
  showKeyboard(isKeyboardVisible: boolean): Promise<void>;
  blur(): Promise<void>;
}

interface NativeInputProps extends Omit<ViewProps, "style"> {
  style?: TerminalInputProps["style"];
  onInput: (event: NativeSyntheticEvent<{ data: string }>) => void;
  onTerminalKey: (event: NativeSyntheticEvent<{ key: string }>) => void;
  onFocus: () => void;
}

const NativeInput = requireNativeViewManager<NativeInputProps & RefAttributes<NativeInputHandle>>(
  "AitTerminalInput",
);

export const TerminalInput = forwardRef<TerminalInputHandle, TerminalInputProps>(
  function TerminalInput({ isKeyboardVisible, onFocus, onInput, onTerminalKey, style }, ref) {
    const inputRef = useRef<NativeInputHandle>(null);

    useImperativeHandle(
      ref,
      () => ({
        focus: () => {
          void inputRef.current?.showKeyboard(isKeyboardVisible);
        },
        showKeyboard: () => {
          void inputRef.current?.showKeyboard(isKeyboardVisible);
        },
        blur: () => {
          void inputRef.current?.blur();
        },
      }),
      [isKeyboardVisible],
    );

    return (
      <NativeInput
        ref={inputRef}
        accessibilityLabel="Terminal input"
        onFocus={() => onFocus?.()}
        onInput={(event) => onInput?.(event.nativeEvent.data)}
        onTerminalKey={(event) => {
          const key = resolveNativeTerminalKey(event.nativeEvent.key);
          if (key) onTerminalKey?.(key);
        }}
        style={[styles.input, style]}
        testID="terminal-native-input"
      />
    );
  },
);

const styles = StyleSheet.create({
  input: {
    backgroundColor: "transparent",
    height: 1,
    left: 0,
    position: "absolute",
    top: 0,
    width: 1,
  },
});
