import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { i18n } from "@/i18n/i18next";
import type { TodoEntry } from "@/types/stream";
import { AgentTaskList } from "./index";

let root: Root | undefined;
let container: HTMLDivElement;

const tasks: TodoEntry[] = [
  { id: "inspect", text: "Inspect the provider", completed: true, status: "completed" },
  {
    id: "implement",
    text: "Implement shared TODO support",
    activeForm: "Implementing shared TODO support",
    completed: false,
    status: "in_progress",
  },
  { id: "verify", text: "Verify the TODO card", completed: false, status: "pending" },
];

beforeEach(async () => {
  vi.stubGlobal("React", React);
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  await i18n.changeLanguage("en");
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root?.unmount());
  root = undefined;
  container.remove();
  vi.unstubAllGlobals();
});

function render(snapshot: TodoEntry[] | undefined): void {
  act(() => root?.render(<AgentTaskList tasks={snapshot} />));
}

function pill(): HTMLElement {
  const element = container.querySelector<HTMLElement>('[data-testid="agent-task-list-header"]');
  if (!element) throw new Error("TODO capsule did not render");
  return element;
}

function panel(): HTMLElement | null {
  return document.querySelector('[data-testid="agent-task-list-header-panel"]');
}

describe("provider TODO capsule", () => {
  it("appears only after a nonempty TODO snapshot and opens the current card on demand", () => {
    render(undefined);
    expect(container.textContent).toBe("");

    render(tasks);
    expect(pill().textContent).toBe("TODO 1/3");
    expect(pill().getAttribute("aria-label")).toBe("TODO, 1/3 tasks");
    expect(pill().getAttribute("aria-expanded")).toBe("false");
    expect(panel()).toBeNull();

    act(() => pill().click());
    expect(pill().getAttribute("aria-expanded")).toBe("true");
    expect(panel()?.textContent).toContain("Inspect the provider");
    expect(panel()?.textContent).toContain("Implementing shared TODO support");
    expect(panel()?.textContent).toContain("Verify the TODO card");

    act(() => pill().click());
    expect(pill().getAttribute("aria-expanded")).toBe("false");
    expect(panel()).toBeNull();
  });

  it("updates an open card without closing it and removes cleared TODOs", () => {
    render(tasks);
    act(() => pill().click());

    render(tasks.map((task) => ({ ...task, completed: true, status: "completed" })));
    expect(pill().textContent).toBe("TODO 3/3");
    expect(pill().getAttribute("aria-expanded")).toBe("true");
    expect(panel()?.textContent).toContain("Implement shared TODO support");
    expect(panel()?.textContent).not.toContain("Implementing shared TODO support");

    render([]);
    expect(container.textContent).toBe("");
    expect(panel()).toBeNull();
  });

  it("preserves legacy completed flags and spells out progress in the selected language", async () => {
    await i18n.changeLanguage("zh-CN");
    render([{ text: "验证任务", completed: true }]);

    expect(pill().textContent).toBe("TODO 1/1");
    expect(pill().getAttribute("aria-label")).toBe("TODO, 1/1 项任务");
  });

  it("wraps long task text in the expanded card", () => {
    const text =
      "Review the provider TODO event and keep every detail readable in the card. ".repeat(8);
    render([{ text, completed: false }]);
    act(() => pill().click());

    const row = panel()?.querySelector<HTMLElement>(`[aria-label="1 ${text}"]`);
    const label = row?.lastElementChild;
    expect(label).toBeInstanceOf(HTMLElement);
    expect(getComputedStyle(label as HTMLElement).whiteSpace).not.toBe("nowrap");
    expect(label?.textContent).toBe(text);
  });

  it("shows native phase headings, phase progress, and task ordinals inside the card", () => {
    render([
      { text: "Read native events", completed: true, group: { id: "phase:0", title: "Inspect" } },
      {
        text: "Check canonical state",
        completed: false,
        group: { id: "phase:0", title: "Inspect" },
      },
      { text: "Build the card", completed: false, group: { id: "phase:1", title: "Implement" } },
    ]);
    act(() => pill().click());

    const headings = [...(panel()?.querySelectorAll<HTMLElement>('[role="heading"]') ?? [])];
    expect(headings.map((heading) => heading.textContent)).toEqual(["Inspect", "Implement"]);
    expect(headings[0]?.parentElement?.textContent).toBe("1Inspect1/2");
    expect(headings[1]?.parentElement?.textContent).toBe("2Implement0/1");
    expect(panel()?.querySelector('[aria-label="1.1 Read native events"]')?.textContent).toBe(
      "1.1Read native events",
    );
    expect(panel()?.querySelector('[aria-label="2.1 Build the card"]')?.textContent).toBe(
      "2.1Build the card",
    );
  });
});
