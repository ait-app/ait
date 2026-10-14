// @vitest-environment jsdom
import React, { act, createRef, type ComponentProps } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { TerminalInput } from "./terminal-input.ios";
import type { TerminalInputHandle } from "./terminal-input.native";

const native = vi.hoisted(() => ({
  props: null as null | Record<string, unknown>,
  showKeyboard: vi.fn(async (_visible: boolean) => {}),
  blur: vi.fn(async () => {}),
}));

beforeEach(() => vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true));
afterEach(() => vi.unstubAllGlobals());

vi.mock("expo-modules-core", async () => {
  const React = await import("react");
  return {
    requireNativeViewManager: (name: string) => {
      expect(name).toBe("AitTerminalInput");
      return React.forwardRef(function MockTerminalInput(props: Record<string, unknown>, ref) {
        native.props = props;
        React.useImperativeHandle(ref, () => native);
        return React.createElement("div");
      });
    },
  };
});

function withInput(
  props: ComponentProps<typeof TerminalInput>,
  test: (
    handle: TerminalInputHandle,
    render: (isKeyboardVisible: boolean) => TerminalInputHandle,
  ) => void,
) {
  const container = document.createElement("div");
  const root = createRoot(container);
  const ref = createRef<TerminalInputHandle>();
  native.showKeyboard.mockClear();
  native.blur.mockClear();
  const render = (isKeyboardVisible: boolean) => {
    act(() =>
      root.render(<TerminalInput {...props} ref={ref} isKeyboardVisible={isKeyboardVisible} />),
    );
    return ref.current!;
  };
  try {
    render(props.isKeyboardVisible);
    test(ref.current!, render);
  } finally {
    act(() => root.unmount());
  }
}

describe("iOS terminal input bridge", () => {
  it("forwards committed Unicode without the speculative keypress state", () => {
    const onInput = vi.fn();
    withInput({ isKeyboardVisible: true, onInput }, () => {
      const onNativeInput = native.props!.onInput as (event: {
        nativeEvent: { data: string };
      }) => void;
      for (const data of ["echo ", "你好🙂", "\x7f", "\r"]) {
        onNativeInput({ nativeEvent: { data } });
      }
      expect(onInput.mock.calls).toEqual([["echo "], ["你好🙂"], ["\x7f"], ["\r"]]);
    });
  });

  it("passes current keyboard visibility to focus requests without resetting on render", () => {
    withInput({ isKeyboardVisible: true }, (handle, render) => {
      handle.focus();
      expect(native.showKeyboard).toHaveBeenLastCalledWith(true);
      const updatedHandle = render(false);
      expect(native.showKeyboard).toHaveBeenCalledTimes(1);
      updatedHandle.showKeyboard();
      expect(native.showKeyboard).toHaveBeenLastCalledWith(false);
      handle.blur();
      expect(native.blur).toHaveBeenCalledOnce();
    });
  });

  it("forwards focus and semantic arrow events", () => {
    const onFocus = vi.fn();
    const onTerminalKey = vi.fn();
    withInput({ isKeyboardVisible: true, onFocus, onTerminalKey }, () => {
      (native.props!.onFocus as () => void)();
      const onNativeKey = native.props!.onTerminalKey as (event: {
        nativeEvent: { key: string };
      }) => void;
      onNativeKey({ nativeEvent: { key: "ArrowUp" } });
      onNativeKey({ nativeEvent: { key: "unknown" } });
      expect(onFocus).toHaveBeenCalledOnce();
      expect(onTerminalKey.mock.calls).toEqual([["ArrowUp"]]);
    });
  });
});
