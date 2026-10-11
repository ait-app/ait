/** @vitest-environment jsdom */
import React from "react";
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { AgentErrorCallout } from "./agent-error-callout";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => (key === "providerSelection.unknownError" ? "Unknown error" : key),
  }),
}));

// Isolate the callout from the message module's native and Markdown dependencies.
vi.mock("@/components/message", () => ({
  Notification: ({ message }: { message: string }) => <div>{message}</div>,
}));

afterEach(cleanup);

it("shows a saved OpenCode failure on reopening and clears it when the agent resumes", () => {
  const lastError = "Internal error: Model exo-free has been deprecated.";
  const { rerender } = render(<AgentErrorCallout status="error" lastError={lastError} />);

  expect(screen.getByRole("alert").textContent).toContain(lastError);

  // A stale error in the snapshot must not remain visible during the next turn.
  rerender(<AgentErrorCallout status="running" lastError={lastError} />);
  expect(screen.queryByRole("alert")).toBeNull();
  expect(screen.queryByText(lastError)).toBeNull();
});

it("shows a fallback when a failed agent has no error details", () => {
  render(<AgentErrorCallout status="error" lastError={null} />);

  expect(screen.getByRole("alert").textContent).toContain("Unknown error");
});
