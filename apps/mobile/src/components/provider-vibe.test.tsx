// @vitest-environment jsdom
import React from "react";
import { afterEach, expect, it, vi } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import type { ProviderUiState } from "@ait/protocol/messages";
import { ProviderVibe } from "./provider-vibe";

type Vibe = NonNullable<ProviderUiState["vibe"]>;

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => (key === "providerControl.vibe.title" ? "Vibe" : key),
  }),
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

it("shows only the native mode status without buttons or duplicated worker details", () => {
  const view = render(<ProviderVibe state={running} />);
  expect(screen.getByTestId("provider-vibe")).toBeTruthy();
  expect(screen.getByTestId("provider-vibe").textContent).toBe("Vibe ON");
  expect(screen.getByText("ON").style.fontWeight).toBe("700");
  expect(screen.queryByRole("button")).toBeNull();
  expect(view.container.querySelector("[aria-expanded], [tabindex]")).toBeNull();
  expect(screen.queryByText("providerControl.vibe.title")).toBeNull();
  expect(screen.queryByText("worker-fast")).toBeNull();
  expect(screen.queryByText("Fast worker: draft the change list")).toBeNull();
});

it.each([false, true])(
  "shows the acknowledged mode independently of worker count, enabled = %s",
  (enabled) => {
    const view = render(<ProviderVibe state={{ enabled, workers: [] }} />);
    expect(Boolean(view.queryByTestId("provider-vibe"))).toBe(enabled);
  },
);

it.each(["completed", "failed", "canceled"] as const)(
  "keeps the director indicator when all workers are %s",
  (status) => {
    const view = render(
      <ProviderVibe
        state={{ enabled: true, workers: running.workers.map((worker) => ({ ...worker, status })) }}
      />,
    );
    expect(view.queryByTestId("provider-vibe")).toBeTruthy();
  },
);

it("hides disabled mode even when its worker history remains", () => {
  const view = render(<ProviderVibe state={{ ...running, enabled: false }} />);
  expect(view.container.innerHTML).toBe("");
});
