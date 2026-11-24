/** @vitest-environment jsdom */
import React from "react";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { i18n } from "@/i18n/i18next";
import { GoalTrack } from "./goal";
import { readGoal } from "./goal-model";

const goal = readGoal({
  objective: "Ship the feature",
  status: "active",
  tokensUsed: 12,
  tokenBudget: 100,
  actions: { edit: true, pause: true, clear: true },
})!;

beforeEach(async () => {
  vi.stubGlobal("React", React);
  await i18n.changeLanguage("zh-CN");
});
afterEach(cleanup);

it.each([undefined, null])(
  "hides a missing native token budget (%s) until one is supplied",
  async (tokenBudget) => {
    const snapshot = readGoal({ ...goal, tokenBudget })!;
    const onAction = vi.fn();
    const { rerender } = render(<GoalTrack goal={snapshot} onAction={onAction} />);
    fireEvent.click(screen.getByTestId("agent-goal-pill"));
    expect(await screen.findByText("Ship the feature")).toBeTruthy();
    expect(screen.queryByText(/Token 预算/u)).toBeNull();
    rerender(<GoalTrack goal={goal} onAction={onAction} />);
    expect(await screen.findByText("Token 预算：100")).toBeTruthy();
    rerender(<GoalTrack goal={snapshot} onAction={onAction} />);
    expect(screen.queryByText(/Token 预算/u)).toBeNull();
  },
);

it("opens the goal card and edits its objective without optimistic state changes", async () => {
  const onAction = vi.fn().mockResolvedValue(undefined);
  render(<GoalTrack goal={goal} onAction={onAction} />);
  fireEvent.click(screen.getByTestId("agent-goal-pill"));
  expect(await screen.findByText("Ship the feature")).toBeTruthy();
  fireEvent.click(screen.getByText("编辑目标"));
  fireEvent.change(screen.getByTestId("goal-objective-input"), { target: { value: "pause" } });
  fireEvent.click(screen.getByText("保存"));
  await waitFor(() => expect(onAction).toHaveBeenCalledWith("edit", "pause"));
  expect(goal.objective).toBe("Ship the feature");
});

it("shows only native-supported actions and retains the goal after an operation fails", async () => {
  const onAction = vi.fn().mockRejectedValue(new Error("Native refusal"));
  render(
    <GoalTrack goal={{ ...goal, actions: { ...goal.actions, edit: false } }} onAction={onAction} />,
  );
  fireEvent.click(screen.getByTestId("agent-goal-pill"));
  expect(screen.queryByText("编辑目标")).toBeNull();
  fireEvent.click(await screen.findByText("暂停目标"));
  expect(await screen.findByRole("alert")).toHaveProperty("textContent", "Native refusal");
  expect(screen.getByText("Ship the feature")).toBeTruthy();
});
