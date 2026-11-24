import { expect, it } from "vitest";
import { goalCommand, readGoal } from "./goal-model";

it("requires authoritative structured facts and never invents actions or usage", () => {
  expect(readGoal(null)).toBeNull();
  expect(readGoal("Goal active: ship")).toBeNull();
  expect(readGoal({ objective: "", status: "active" })).toBeNull();
  const goal = readGoal({
    objective: "Ship",
    status: "paused",
    actions: { resume: true, edit: "yes" },
    tokensUsed: -1,
  });
  expect(goal?.actions).toEqual({ edit: false, pause: false, resume: true, clear: false });
  expect(goal?.tokensUsed).toBeUndefined();
  expect(goalCommand("edit", " pause ")).toBe("/goal set pause");
  expect(goalCommand("clear")).toBe("/goal clear");
});
