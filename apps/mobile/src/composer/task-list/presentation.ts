import type { TodoEntry } from "@/types/stream";

export interface TaskListPresentationRow {
  task: TodoEntry;
  number: string;
  depth: number;
}

export interface TaskListSection {
  id: string;
  title?: string;
  number?: string;
  completed: number;
  total: number;
  rows: TaskListPresentationRow[];
}

interface NumberedText {
  number: string;
  body: string;
}

function numberedText(text: string): NumberedText | undefined {
  const match = /^[ \t]*(\d+(?:\.\d+)*)([.)]?)[ \t]+([\s\S]+)$/.exec(text);
  if (!match || (!match[1].includes(".") && !match[2])) return undefined;
  return { number: match[1], body: match[3] };
}

function outlineDepth(number: string, numbers: ReadonlySet<string>): number {
  const parts = number.split(".");
  let depth = 0;
  while (parts.length > 1) {
    parts.pop();
    if (!numbers.has(parts.join("."))) break;
    depth += 1;
  }
  return depth;
}

function presentRows(tasks: TodoEntry[], sectionNumber?: string): TaskListPresentationRow[] {
  const prefixes = tasks.map((task) => numberedText(task.text));
  const numbers = new Set(prefixes.flatMap((prefix) => (prefix ? [prefix.number] : [])));
  return tasks.map((task, index) => {
    const prefix = prefixes[index];
    const activePrefix = task.activeForm ? numberedText(task.activeForm) : undefined;
    const presentedTask = prefix
      ? {
          ...task,
          text: prefix.body,
          ...(activePrefix?.number === prefix.number ? { activeForm: activePrefix.body } : {}),
        }
      : task;
    return {
      task: presentedTask,
      number: prefix?.number ?? (sectionNumber ? `${sectionNumber}.${index + 1}` : `${index + 1}`),
      depth: (sectionNumber ? 1 : 0) + (prefix ? outlineDepth(prefix.number, numbers) : 0),
    };
  });
}

/** Preserve native ordering and phase boundaries; only explicit outline parents add indentation. */
export function presentTaskList(tasks: readonly TodoEntry[]): TaskListSection[] {
  const sections: TaskListSection[] = [];
  let sectionTasks: TodoEntry[] = [];
  let currentGroup: TodoEntry["group"];
  let phaseNumber = 0;
  const flush = () => {
    if (sectionTasks.length === 0) return;
    const number = currentGroup ? `${++phaseNumber}` : undefined;
    sections.push({
      id: `${currentGroup?.id ?? "tasks"}:${sections.length}`,
      ...(currentGroup ? { title: currentGroup.title, number } : {}),
      completed: sectionTasks.filter((task) => task.completed || task.status === "completed")
        .length,
      total: sectionTasks.length,
      rows: presentRows(sectionTasks, number),
    });
    sectionTasks = [];
  };

  for (const task of tasks) {
    if (currentGroup?.id !== task.group?.id || currentGroup?.title !== task.group?.title) flush();
    currentGroup = task.group;
    sectionTasks.push(task);
  }
  flush();
  return sections;
}
