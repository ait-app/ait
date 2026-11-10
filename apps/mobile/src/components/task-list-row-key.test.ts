import { describe, expect, it } from "vitest";
import { taskListRowKey } from "./task-list-row-key";

describe("taskListRowKey", () => {
  it("distinguishes id-less todos that share the same text", () => {
    const item = { text: "Same", completed: false };

    expect(taskListRowKey(item, 0)).not.toBe(taskListRowKey(item, 1));
  });

  it("uses the id alone when there is one", () => {
    const item = { id: "todo-1", text: "Same", completed: false };

    expect(taskListRowKey(item, 0)).toBe("todo-1");
    expect(taskListRowKey(item, 3)).toBe("todo-1");
  });
});
