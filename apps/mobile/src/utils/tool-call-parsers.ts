import { z } from "zod";
import type { HighlightToken } from "@ait/highlight";

export interface DiffSegment {
  text: string;
  changed: boolean;
}

export interface DiffLine {
  type: "add" | "remove" | "context" | "header";
  content: string;
  segments?: DiffSegment[];
  // Syntax-highlight tokens for the code on this line (prefix char excluded),
  // attached by highlightDiffLines when the file's language is supported.
  tokens?: HighlightToken[];
}

function splitIntoLines(text: string): string[] {
  if (!text) {
    return [];
  }

  return text.replace(/\r\n/g, "\n").split("\n");
}

function splitIntoWords(text: string): string[] {
  const result: string[] = [];
  let current = "";
  let inWord = false;

  for (const char of text) {
    const isWordChar = /\w/.test(char);
    if (isWordChar) {
      if (!inWord && current) {
        result.push(current);
        current = "";
      }
      inWord = true;
      current += char;
    } else {
      if (inWord && current) {
        result.push(current);
        current = "";
      }
      inWord = false;
      current += char;
    }
  }
  if (current) {
    result.push(current);
  }
  return result;
}

// LCS tables are allocated eagerly, so anything larger is skipped: about 2,000 x 2,000 entries.
const MAX_LCS_TABLE_CELLS = 4_000_000;

function lcsTableCells(leftLength: number, rightLength: number): number {
  return (leftLength + 1) * (rightLength + 1);
}

// dp[i][j] is the length of the longest common subsequence of left[i..] and right[j..].
function buildLcsTable(left: string[], right: string[]): number[][] {
  const m = left.length;
  const n = right.length;
  const dp: number[][] = Array.from({ length: m + 1 }, () => Array(n + 1).fill(0));

  for (let i = m - 1; i >= 0; i -= 1) {
    for (let j = n - 1; j >= 0; j -= 1) {
      if (left[i] === right[j]) {
        dp[i][j] = dp[i + 1][j + 1] + 1;
      } else {
        dp[i][j] = Math.max(dp[i + 1][j], dp[i][j + 1]);
      }
    }
  }
  return dp;
}

function computeWordLevelDiff(
  oldWords: string[],
  newWords: string[],
): { oldSegments: DiffSegment[]; newSegments: DiffSegment[] } {
  const m = oldWords.length;
  const n = newWords.length;

  // LCS to find common words
  const dp = buildLcsTable(oldWords, newWords);

  // Mark which words are in LCS (unchanged)
  const oldInLCS = new Set<number>();
  const newInLCS = new Set<number>();

  let i = 0;
  let j = 0;
  while (i < m && j < n) {
    if (oldWords[i] === newWords[j]) {
      oldInLCS.add(i);
      newInLCS.add(j);
      i += 1;
      j += 1;
    } else if (dp[i + 1][j] >= dp[i][j + 1]) {
      i += 1;
    } else {
      j += 1;
    }
  }

  // Build segments: consecutive unchanged or changed words merged
  const buildSegments = (words: string[], inLCS: Set<number>): DiffSegment[] => {
    if (words.length === 0) return [];

    const segments: DiffSegment[] = [];
    let currentText = "";
    let currentChanged: boolean | null = null;

    for (let idx = 0; idx < words.length; idx++) {
      const word = words[idx];
      const changed = !inLCS.has(idx);

      if (currentChanged === null) {
        currentText = word;
        currentChanged = changed;
      } else if (changed === currentChanged) {
        currentText += word;
      } else {
        segments.push({ text: currentText, changed: currentChanged });
        currentText = word;
        currentChanged = changed;
      }
    }

    if (currentText) {
      segments.push({ text: currentText, changed: currentChanged ?? false });
    }

    return segments;
  };

  const oldSegments = buildSegments(oldWords, oldInLCS);
  const newSegments = buildSegments(newWords, newInLCS);

  return {
    oldSegments,
    newSegments,
  };
}

export function buildLineDiff(originalText: string, updatedText: string): DiffLine[] {
  const originalLines = splitIntoLines(originalText);
  const updatedLines = splitIntoLines(updatedText);

  const hasAnyContent = originalLines.length > 0 || updatedLines.length > 0;
  if (!hasAnyContent) {
    return [];
  }

  const m = originalLines.length;
  const n = updatedLines.length;
  if (lcsTableCells(m, n) > MAX_LCS_TABLE_CELLS) {
    return [
      ...originalLines.map((line): DiffLine => ({ type: "remove", content: `-${line}` })),
      ...updatedLines.map((line): DiffLine => ({ type: "add", content: `+${line}` })),
    ];
  }

  const dp = buildLcsTable(originalLines, updatedLines);

  const diff: DiffLine[] = [];

  let i = 0;
  let j = 0;
  while (i < m && j < n) {
    if (originalLines[i] === updatedLines[j]) {
      diff.push({ type: "context", content: ` ${originalLines[i]}` });
      i += 1;
      j += 1;
    } else if (dp[i + 1][j] >= dp[i][j + 1]) {
      diff.push({ type: "remove", content: `-${originalLines[i]}` });
      i += 1;
    } else {
      diff.push({ type: "add", content: `+${updatedLines[j]}` });
      j += 1;
    }
  }

  while (i < m) {
    diff.push({ type: "remove", content: `-${originalLines[i]}` });
    i += 1;
  }

  while (j < n) {
    diff.push({ type: "add", content: `+${updatedLines[j]}` });
    j += 1;
  }

  // Post-process to add word-level segments for adjacent remove/add pairs
  let remainingWordDiffCells = MAX_LCS_TABLE_CELLS;
  for (let idx = 0; idx < diff.length - 1; idx++) {
    const curr = diff[idx];
    const next = diff[idx + 1];

    if (curr.type === "remove" && next.type === "add") {
      // Strip the leading -/+ from content for comparison
      const oldWords = splitIntoWords(curr.content.slice(1));
      const newWords = splitIntoWords(next.content.slice(1));
      const wordDiffCells = lcsTableCells(oldWords.length, newWords.length);
      if (wordDiffCells > remainingWordDiffCells) {
        continue;
      }
      remainingWordDiffCells -= wordDiffCells;

      const { oldSegments, newSegments } = computeWordLevelDiff(oldWords, newWords);
      curr.segments = oldSegments;
      next.segments = newSegments;
    }
  }

  return diff;
}

const HUNK_HEADER_PATTERN = /^@@ -\d+(?:,(\d+))? \+\d+(?:,(\d+))? @@/;

// A header without line counts leaves the hunk open until the next file section.
function hunkLineCounts(header: string): { oldLines: number; newLines: number } {
  const match = HUNK_HEADER_PATTERN.exec(header);
  if (!match) {
    return { oldLines: Infinity, newLines: Infinity };
  }
  return { oldLines: Number(match[1] ?? 1), newLines: Number(match[2] ?? 1) };
}

export function parseUnifiedDiff(diffText?: string): DiffLine[] {
  if (!diffText) {
    return [];
  }

  const lines = splitIntoLines(diffText);
  const diff: DiffLine[] = [];
  let oldLinesLeft = 0;
  let newLinesLeft = 0;

  for (const line of lines) {
    if (!line.length) {
      oldLinesLeft -= 1;
      newLinesLeft -= 1;
      diff.push({ type: "context", content: line });
      continue;
    }

    if (line.startsWith("@@")) {
      const counts = hunkLineCounts(line);
      oldLinesLeft = counts.oldLines;
      newLinesLeft = counts.newLines;
      diff.push({ type: "header", content: line });
      continue;
    }

    if (line.startsWith("diff --git")) {
      oldLinesLeft = 0;
      newLinesLeft = 0;
      continue;
    }

    // Inside a hunk these prefixes are real content; outside, they are file headers.
    const inHunk = oldLinesLeft > 0 || newLinesLeft > 0;

    if (line.startsWith("+")) {
      if (inHunk || !line.startsWith("+++")) {
        newLinesLeft -= 1;
        diff.push({ type: "add", content: line });
      }
      continue;
    }

    if (line.startsWith("-")) {
      if (inHunk || !line.startsWith("---")) {
        oldLinesLeft -= 1;
        diff.push({ type: "remove", content: line });
      }
      continue;
    }

    if (line.startsWith("index ")) {
      continue;
    }

    if (line.startsWith("\\ No newline")) {
      diff.push({ type: "header", content: line });
      continue;
    }

    oldLinesLeft -= 1;
    newLinesLeft -= 1;
    diff.push({ type: "context", content: line });
  }

  return diff;
}

// ---- Task Extraction (cross-provider) ----

export type TaskStatus = "pending" | "in_progress" | "completed";

interface TaskEntry {
  text: string;
  status: TaskStatus;
  completed: boolean;
  activeForm?: string;
}

const TaskStatusSchema = z.enum(["pending", "in_progress", "completed"]);

const ClaudeTodoWriteSchema = z.object({
  todos: z.array(
    z.object({
      content: z.string(),
      status: TaskStatusSchema,
      activeForm: z.string().optional(),
    }),
  ),
});

const UpdatePlanSchema = z.object({
  plan: z.array(
    z.object({
      step: z.string(),
      status: TaskStatusSchema.catch("pending"),
    }),
  ),
});

function normalizeToolName(toolName: string): string {
  return toolName
    .trim()
    .replace(/[.\s-]+/g, "_")
    .toLowerCase();
}

export function extractTaskEntriesFromToolCall(
  toolName: string,
  input: unknown,
): TaskEntry[] | null {
  const normalized = normalizeToolName(toolName);

  // Claude's plan mode uses ExitPlanMode for the approval prompt; it is not a task list.
  if (normalized === "exitplanmode") {
    return null;
  }

  if (normalized === "todowrite" || normalized === "todo_write") {
    const parsed = ClaudeTodoWriteSchema.safeParse(input);
    if (!parsed.success) {
      return null;
    }
    return parsed.data.todos.map((todo) => {
      const status = todo.status;
      const text = todo.content.trim();
      const activeForm = todo.activeForm?.trim();
      return {
        text: text.length ? text : todo.content,
        status,
        completed: status === "completed",
        ...(activeForm ? { activeForm } : {}),
      };
    });
  }

  if (normalized === "update_plan") {
    const parsed = UpdatePlanSchema.safeParse(input);
    if (!parsed.success) {
      return null;
    }
    return parsed.data.plan
      .map((entry) => ({
        text: entry.step.trim(),
        status: entry.status,
        completed: entry.status === "completed",
      }))
      .filter((entry) => entry.text.length > 0);
  }

  return null;
}
