// @vitest-environment jsdom
import React from "react";
import { afterEach, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import type { ProviderUiState } from "@ait/protocol/messages";
import { ProviderVibe } from "./provider-vibe";

type Vibe = NonNullable<ProviderUiState["vibe"]>;

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

afterEach(cleanup);

const running: Vibe = {
  enabled: true,
  workers: [
    {
      id: "worker-fast",
      parentId: "parent",
      status: "running",
      description: "Fast worker: draft the change list",
      model: "deepseek-v4.1-flash",
    },
  ],
};

it("shows director controls during worker execution without duplicating the shared subagent list", () => {
  render(<ProviderVibe state={running} />);
  expect(screen.getByTestId("provider-vibe")).toBeTruthy();
  expect(screen.getByText("providerControl.vibe.enabled")).toBeTruthy();
  expect(screen.queryByText("worker-fast")).toBeNull();
  expect(screen.queryByText("Fast worker: draft the change list")).toBeNull();
});

it.each([false, true])("hides an empty Vibe mode, enabled = %s", (enabled) => {
  const view = render(<ProviderVibe state={{ enabled, workers: [] }} />);
  expect(view.container.innerHTML).toBe("");
});

it.each(["completed", "failed", "canceled"] as const)(
  "hides the director indicator when all workers are %s",
  (status) => {
    const view = render(
      <ProviderVibe
        state={{ enabled: true, workers: running.workers.map((worker) => ({ ...worker, status })) }}
      />,
    );
    expect(view.container.innerHTML).toBe("");
  },
);

it("hides disabled mode even when its worker history remains", () => {
  const view = render(<ProviderVibe state={{ ...running, enabled: false }} />);
  expect(view.container.innerHTML).toBe("");
});

it("keeps active controls until exit is acknowledged and permits retry on failure", async () => {
  let reject!: (error: Error) => void;
  const onExit = vi.fn(
    () =>
      new Promise<void>((_resolve, fail) => {
        reject = fail;
      }),
  );
  const view = render(<ProviderVibe state={running} onExit={onExit} />);
  fireEvent.click(screen.getByTestId("provider-vibe-exit"));
  expect(onExit).toHaveBeenCalledOnce();
  expect(screen.getByText("providerControl.vibe.enabled")).toBeTruthy();
  await act(async () => {
    reject(new Error("Cannot exit"));
  });
  expect(screen.getByText("Cannot exit")).toBeTruthy();
  view.rerender(<ProviderVibe state={{ ...running, enabled: false }} onExit={onExit} />);
  expect(screen.queryByTestId("provider-vibe")).toBeNull();
});

it("explains a disabled exit without calling its handler", () => {
  const onExit = vi.fn();
  render(
    <ProviderVibe
      state={running}
      disabled
      disabledReason="Finish the main task first"
      onExit={onExit}
    />,
  );
  expect(screen.getByText("Finish the main task first")).toBeTruthy();
  fireEvent.click(screen.getByTestId("provider-vibe-exit"));
  expect(onExit).not.toHaveBeenCalled();
});
