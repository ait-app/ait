/** @vitest-environment jsdom */
import React from "react";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { i18n } from "@/i18n/i18next";
import { VibeTrack } from "./vibe";
import { readVibe } from "./vibe-model";

const vibe = readVibe({
  enabled: true,
  workers: [
    {
      id: "layout",
      description: "修复窄屏布局",
      model: '["google", "gemini-4-carbon"]',
      status: "running",
    },
    { id: "tests", description: "验证退出行为", model: '["openai", "gpt-5.4"]', status: "failed" },
  ],
})!;

beforeEach(async () => {
  vi.stubGlobal("React", React);
  await i18n.changeLanguage("zh-CN");
});
afterEach(cleanup);

it("shows actual tasks, model routes and changing states, with direct access to a worker", async () => {
  const openWorker = vi.fn();
  const props = {
    onExit: vi.fn(),
    workerLink: (id: string) => ({ onPress: () => openWorker(id), closeOnSelect: false }),
  };
  const { rerender } = render(<VibeTrack vibe={vibe} {...props} />);
  fireEvent.click(screen.getByTestId("agent-vibe-pill"));
  expect(await screen.findByText("修复窄屏布局")).toBeTruthy();
  expect(screen.getByText("google / gemini-4-carbon")).toBeTruthy();
  expect(screen.getByText("运行中")).toBeTruthy();
  expect(screen.getByText("失败")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "修复窄屏布局" }));
  expect(openWorker).toHaveBeenCalledWith("layout");
  rerender(
    <VibeTrack vibe={{ workers: [{ ...vibe.workers![0]!, status: "completed" }] }} {...props} />,
  );
  expect(await screen.findByText("已完成")).toBeTruthy();
  expect(screen.queryByText("运行中")).toBeNull();
  expect(screen.queryByText("验证退出行为")).toBeNull();
  expect(screen.getByText("退出 Vibe")).toBeTruthy();
});

it("keeps the confirmed mode visible after exit fails, then allows retry", async () => {
  const onExit = vi
    .fn()
    .mockRejectedValueOnce(new Error("Native refusal"))
    .mockResolvedValue(undefined);
  render(<VibeTrack vibe={vibe} onExit={onExit} />);
  fireEvent.click(screen.getByTestId("agent-vibe-pill"));
  fireEvent.click(await screen.findByText("停止任务并退出 Vibe"));
  expect(await screen.findByRole("alert")).toHaveProperty("textContent", "Native refusal");
  expect(screen.getByTestId("agent-vibe-pill")).toBeTruthy();
  fireEvent.click(screen.getByTestId("agent-vibe-pill"));
  fireEvent.click(screen.getByTestId("agent-vibe-pill"));
  await screen.findByText("停止任务并退出 Vibe");
  await waitFor(() => expect(screen.queryByRole("alert")).toBeNull());
  fireEvent.click(screen.getByText("停止任务并退出 Vibe"));
  await waitFor(() => expect(onExit).toHaveBeenCalledTimes(2));
  expect(screen.queryByRole("alert")).toBeNull();
  expect(screen.getByTestId("agent-vibe-pill")).toBeTruthy();
});

it("marks disconnected task states unconfirmed and disables exit while retaining read access", async () => {
  const onExit = vi.fn();
  const onOpen = vi.fn();
  render(
    <VibeTrack
      vibe={vibe}
      disabled
      onExit={onExit}
      workerLink={() => ({ onPress: onOpen, closeOnSelect: false })}
    />,
  );
  fireEvent.click(screen.getByTestId("agent-vibe-pill"));
  expect(await screen.findByText("已断开 · 显示上次任务记录")).toBeTruthy();
  expect(screen.queryByText("运行中")).toBeNull();
  expect(screen.queryByText("1 个运行中")).toBeNull();
  expect(screen.getByText("待确认")).toBeTruthy();
  expect(screen.getByText("失败")).toBeTruthy();
  fireEvent.click(screen.getByText("停止任务并退出 Vibe"));
  expect(onExit).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "修复窄屏布局" }));
  expect(onOpen).toHaveBeenCalledOnce();
});

it("distinguishes no assigned tasks from unavailable worker data", async () => {
  const { rerender } = render(<VibeTrack vibe={{ workers: [] }} onExit={vi.fn()} />);
  fireEvent.click(screen.getByTestId("agent-vibe-pill"));
  expect(await screen.findByText("尚未分配任务")).toBeTruthy();
  expect(screen.getByText("退出 Vibe")).toBeTruthy();
  rerender(<VibeTrack vibe={{ workers: null }} onExit={vi.fn()} />);
  expect(await screen.findByText("任务数据不可用")).toBeTruthy();
  expect(screen.queryByText("尚未分配任务")).toBeNull();
});

it("does not submit duplicate exits while awaiting the command response", async () => {
  const response = Promise.withResolvers<void>();
  const onExit = vi.fn(() => response.promise);
  render(<VibeTrack vibe={vibe} onExit={onExit} />);
  fireEvent.click(screen.getByTestId("agent-vibe-pill"));
  const exit = await screen.findByRole("button", { name: "停止任务并退出 Vibe" });
  fireEvent.click(exit);
  fireEvent.click(exit);
  expect(onExit).toHaveBeenCalledOnce();
  await act(async () => response.resolve());
  // Admission does not authorize optimistic mode or worker-state changes.
  expect(screen.getByTestId("agent-vibe-pill")).toBeTruthy();
  expect(screen.getByText("运行中")).toBeTruthy();
});
