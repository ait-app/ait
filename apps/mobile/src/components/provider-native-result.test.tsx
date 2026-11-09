// @vitest-environment jsdom
import React from "react";
import { afterEach, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { ProviderNativeResult } from "./provider-native-result";
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock("@/components/ui/button", () => ({
  Button: ({
    children,
    onPress,
    testID,
  }: {
    children: React.ReactNode;
    onPress: () => void;
    testID: string;
  }) => (
    <button onClick={onPress} data-testid={testID}>
      {children}
    </button>
  ),
}));
afterEach(cleanup);
it("summarizes statistics and keeps raw paths and precision behind details", () => {
  const text = JSON.stringify({
    tokens: { total: 1250, input: 1200, output: 50 },
    cost: 0.123456789,
    contextUsage: { percent: 12.3456 },
    sessionFile: "/private/long/path/session.jsonl",
  });
  render(<ProviderNativeResult text={text} />);
  expect(screen.getByText("$0.1235")).toBeTruthy();
  expect(screen.getByText("12.3%")).toBeTruthy();
  expect(screen.queryByTestId("provider-result-raw")).toBeNull();
  expect(screen.queryByText(/private/)).toBeNull();
  fireEvent.click(screen.getByTestId("provider-result-details"));
  expect(screen.getByTestId("provider-result-raw").textContent).toBe(text);
});
it("shows an exported file name without a raw host path", () => {
  render(<ProviderNativeResult text='{"path":"/private/session.html"}' fileName="session.html" />);
  expect(screen.getByText("session.html")).toBeTruthy();
  expect(screen.queryByText(/private/)).toBeNull();
});
it("keeps native plain-text errors readable without expanding details", () => {
  render(<ProviderNativeResult text="Provider rejected the operation" />);
  expect(screen.getByText("Provider rejected the operation")).toBeTruthy();
  expect(screen.queryByTestId("provider-result-details")).toBeNull();
});
