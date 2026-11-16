import { describe, expect, it } from "vitest";
import type { TodoEntry } from "@/types/stream";
import { presentTaskList } from "./presentation";

const pending = (text: string): TodoEntry => ({ text, completed: false, status: "pending" });

describe("native TODO presentation", () => {
  it("numbers flat native lists in source order without sorting task IDs", () => {
    const tasks = ["2", "10", "1"].map((id) => ({ ...pending(`Task ${id}`), id }));
    const [section] = presentTaskList(tasks);
    expect(section.rows.map((row) => [row.number, row.task.id, row.depth])).toEqual([
      ["1", "2", 0],
      ["2", "10", 0],
      ["3", "1", 0],
    ]);
    expect(section.title).toBeUndefined();
    expect(presentTaskList([])).toEqual([]);
  });

  it("keeps native phases, same-name groups and phase completion separate", () => {
    const tasks: TodoEntry[] = [
      { ...pending("Read"), completed: true, group: { id: "phase:0", title: "Implement" } },
      { ...pending("Build"), group: { id: "phase:0", title: "Implement" } },
      { ...pending("Verify"), group: { id: "phase:1", title: "Implement" } },
    ];
    const sections = presentTaskList(tasks);
    expect(
      sections.map(({ title, number, completed, total }) => ({ title, number, completed, total })),
    ).toEqual([
      { title: "Implement", number: "1", completed: 1, total: 2 },
      { title: "Implement", number: "2", completed: 0, total: 1 },
    ]);
    expect(
      sections.flatMap((section) => section.rows.map((row) => [row.number, row.depth])),
    ).toEqual([
      ["1.1", 1],
      ["1.2", 1],
      ["2.1", 1],
    ]);
  });

  it("does not expose zero-based adapter IDs as human task numbers", () => {
    const [section] = presentTaskList([
      { ...pending("Build"), id: "0" },
      { ...pending("Verify"), id: "1" },
    ]);
    expect(section.rows.map((row) => row.number)).toEqual(["1", "2"]);
  });

  it("aligns explicit outlines without duplicating labels or mutating source text", () => {
    const tasks = [
      pending("1. Build"),
      pending("1.1 Implement"),
      { ...pending("1.1.1) Verify\n  - preserve these details"), activeForm: "1.1.1) Verifying" },
      pending("2) Ship"),
      pending("9.2 Orphan"),
      pending("2026 release support"),
    ];
    const [section] = presentTaskList(tasks);
    expect(section.rows.map((row) => [row.number, row.depth, row.task.text])).toEqual([
      ["1", 0, "Build"],
      ["1.1", 1, "Implement"],
      ["1.1.1", 2, "Verify\n  - preserve these details"],
      ["2", 0, "Ship"],
      ["9.2", 0, "Orphan"],
      ["6", 0, "2026 release support"],
    ]);
    expect(section.rows[2].task.activeForm).toBe("Verifying");
    expect(tasks[2].text).toBe("1.1.1) Verify\n  - preserve these details");
  });

  it("does not merge noncontiguous native groups or split a native task into invented subtasks", () => {
    const group = { id: "a", title: "Build" };
    const sections = presentTaskList([
      { ...pending("Read\n  1. a detail\n  2. another detail"), group },
      pending("Unphased"),
      { ...pending("Verify"), group },
    ]);
    expect(sections.map((section) => section.total)).toEqual([1, 1, 1]);
    expect(new Set(sections.map((section) => section.id)).size).toBe(3);
    expect(sections[0].rows[0].task.text).toContain("1. a detail");
  });
});
