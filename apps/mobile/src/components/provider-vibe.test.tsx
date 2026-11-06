// @vitest-environment jsdom
import React from "react";
import { afterEach, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
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
    {
      id: "worker-good",
      parentId: "parent",
      status: "completed",
      description: null,
      model: null,
    },
  ],
};

it("lists enabled vibe workers with status, description and model", () => {
  const view = render(<ProviderVibe state={running} />);
  expect(screen.getByTestId("provider-vibe")).toBeTruthy();
  expect(screen.getByText("providerControl.vibe.enabled")).toBeTruthy();
  expect(screen.getByText("worker-fast")).toBeTruthy();
  expect(screen.getByText("providerControl.vibe.states.running")).toBeTruthy();
  expect(screen.getByText("Fast worker: draft the change list")).toBeTruthy();
  expect(screen.getByText("deepseek-v4.1-flash")).toBeTruthy();
  expect(screen.getByText("worker-good")).toBeTruthy();
  expect(screen.getByText("providerControl.vibe.states.completed")).toBeTruthy();
  view.unmount();
});

it("renders nothing when vibe is disabled or has no workers", () => {
  const disabled = render(<ProviderVibe state={{ enabled: false, workers: running.workers }} />);
  expect(disabled.container.innerHTML).toBe("");
  disabled.unmount();
  const empty = render(<ProviderVibe state={{ enabled: true, workers: [] }} />);
  expect(empty.container.innerHTML).toBe("");
});

it("opens a worker only when a handler is supplied", () => {
  const onOpenWorker = vi.fn();
  const view = render(<ProviderVibe state={running} onOpenWorker={onOpenWorker} />);
  fireEvent.click(screen.getByTestId("provider-vibe-worker-worker-fast"));
  expect(onOpenWorker).toHaveBeenCalledExactlyOnceWith(running.workers[0]);
  view.unmount();

  const readonly = render(<ProviderVibe state={running} />);
  fireEvent.click(screen.getByTestId("provider-vibe-worker-worker-good"));
  expect(onOpenWorker).toHaveBeenCalledTimes(1);
  expect(readonly.getByTestId("provider-vibe-worker-worker-good").getAttribute("role")).not.toBe(
    "button",
  );
});
