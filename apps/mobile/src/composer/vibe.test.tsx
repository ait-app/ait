/** @vitest-environment jsdom */
import React from "react";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { i18n } from "@/i18n/i18next";
import { VibeTrack } from "./vibe";

beforeEach(async () => {
  vi.stubGlobal("React", React);
  await i18n.changeLanguage("zh-CN");
});
afterEach(cleanup);

it("keeps the confirmed mode visible after exit fails, then allows retry", async () => {
  const onExit = vi
    .fn()
    .mockRejectedValueOnce(new Error("Native refusal"))
    .mockResolvedValue(undefined);
  render(<VibeTrack onExit={onExit} />);
  fireEvent.click(screen.getByTestId("agent-vibe-pill"));
  fireEvent.click(await screen.findByText("退出 Vibe"));
  expect(await screen.findByRole("alert")).toHaveProperty("textContent", "Native refusal");
  expect(screen.getByText("Vibe ON")).toBeTruthy();
  fireEvent.click(screen.getByTestId("agent-vibe-pill"));
  fireEvent.click(screen.getByTestId("agent-vibe-pill"));
  await screen.findByText("退出 Vibe");
  await waitFor(() => expect(screen.queryByRole("alert")).toBeNull());
  fireEvent.click(screen.getByText("退出 Vibe"));
  await waitFor(() => expect(onExit).toHaveBeenCalledTimes(2));
  expect(screen.queryByRole("alert")).toBeNull();
  // Only a new authoritative runtime snapshot may unmount the pill.
  expect(screen.getByText("Vibe ON")).toBeTruthy();
});

it("shows the consequence and preserves read access while disconnected", async () => {
  const onExit = vi.fn();
  render(<VibeTrack disabled onExit={onExit} />);
  fireEvent.click(screen.getByTestId("agent-vibe-pill"));
  expect(await screen.findByText("退出将取消全部 Vibe 子代理。")).toBeTruthy();
  fireEvent.click(screen.getByText("退出 Vibe"));
  expect(onExit).not.toHaveBeenCalled();
});
