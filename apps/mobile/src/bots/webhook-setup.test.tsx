// @vitest-environment jsdom
import React from "react";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { CopyField } from "./webhook-setup";

const { setClipboard } = vi.hoisted(() => ({
  setClipboard: vi.fn(async (_text: string) => true),
}));

vi.mock("expo-clipboard", () => ({ setStringAsync: setClipboard }));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock("./styles", () => ({ botStyles: {} }));
vi.mock("@/runtime/host-runtime", () => ({ useHostRuntimeClient: () => null }));
vi.mock("@/utils/confirm-dialog", () => ({ confirmDialog: vi.fn() }));
vi.mock("@/components/ui/button", () => ({
  Button: ({
    children,
    onPress,
    testID,
  }: {
    children: React.ReactNode;
    onPress: () => void;
    testID?: string;
  }) => (
    <button data-testid={testID} onClick={onPress}>
      {children}
    </button>
  ),
}));

beforeEach(() => {
  vi.useFakeTimers();
  setClipboard.mockClear();
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

it("returns the copy button to its idle label shortly after copying", async () => {
  render(<CopyField label="Secret" value="s3cret-1" testID="secret" />);
  const button = screen.getByTestId("secret-copy");

  await act(async () => {
    fireEvent.click(button);
  });
  expect(setClipboard).toHaveBeenCalledWith("s3cret-1");
  expect(button.textContent).toBe("bots.webhookCopied");

  act(() => {
    vi.advanceTimersByTime(1500);
  });
  expect(button.textContent).toBe("bots.webhookCopy");
});
