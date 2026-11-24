/**
 * Readable layout for tools without a typed detail (`unknown`): MCP tools, skills, REPLs.
 *
 * Values are laid out by shape rather than by tool, so new tools need no entry here.
 * Anything the layout cannot express falls back to JSON, so nothing is hidden.
 */
import { formatValue, humanizeKey } from "./paseo-tool-presentation/detail-sections.js";
import { toSummaryText } from "./paseo-tool-presentation/summary-text.js";

export type ToolDetailBlock =
  | { kind: "fields"; fields: Array<{ label: string; value: string }> }
  | { kind: "code"; label?: string; text: string; language?: string }
  | { kind: "text"; label?: string; text: string }
  | { kind: "images"; count: number };

export interface ToolDetailGroup {
  role: "input" | "output";
  blocks: ToolDetailBlock[];
}

// Strings longer than this, or with a line break, get their own block instead of a field row.
const FIELD_VALUE_CHARS = 120;
// Nested values up to this size read better as indented bullets than as JSON.
const NESTED_FIELD_CHARS = 400;

const CODE_KEYS: Readonly<Record<string, string | undefined>> = {
  code: undefined,
  script: undefined,
  source: undefined,
  command: "sh",
  cmd: "sh",
  sql: "sql",
  query: undefined,
  patch: "diff",
  diff: "diff",
  expression: undefined,
  python: "py",
  javascript: "js",
};

// Placeholders the daemon leaves where it moved an image into its own timeline message.
const IMAGE_PLACEHOLDERS = new Set(["[Image]", "[Image attachment]"]);

// Keys that name what a call acts on, preferred for the one-line summary.
const SUMMARY_KEYS = [
  "skill",
  "name",
  "command",
  "cmd",
  "query",
  "url",
  "path",
  "file_path",
  "filePath",
  "pattern",
  "description",
  "prompt",
  "code",
];

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function toolLanguage(toolName: string | undefined): string | undefined {
  const leaf = toolName
    ?.split(/[.:/]|__/)
    .pop()
    ?.toLowerCase();
  if (leaf === "js" || leaf === "javascript" || leaf === "node") return "js";
  if (leaf === "py" || leaf === "python") return "py";
  return undefined;
}

class BlockList {
  readonly blocks: ToolDetailBlock[] = [];

  field(label: string, value: string): void {
    const last = this.blocks.at(-1);
    if (last?.kind === "fields") {
      last.fields.push({ label, value });
    } else {
      this.blocks.push({ kind: "fields", fields: [{ label, value }] });
    }
  }

  image(): void {
    const last = this.blocks.at(-1);
    if (last?.kind === "images") {
      last.count += 1;
    } else {
      this.blocks.push({ kind: "images", count: 1 });
    }
  }

  push(block: ToolDetailBlock): void {
    this.blocks.push(block);
  }
}

function isLongText(text: string): boolean {
  return text.length > FIELD_VALUE_CHARS || text.includes("\n");
}

function isSimpleNested(value: unknown): boolean {
  let size = 0;
  const visit = (node: unknown, depth: number): boolean => {
    if (depth > 2) return false;
    if (typeof node === "string") {
      size += node.length;
      return !node.includes("\n") && size <= NESTED_FIELD_CHARS;
    }
    if (Array.isArray(node)) return node.every((child) => visit(child, depth + 1));
    if (isRecord(node)) return Object.values(node).every((child) => visit(child, depth + 1));
    size += 8;
    return size <= NESTED_FIELD_CHARS;
  };
  return visit(value, 0);
}

function json(value: unknown): string {
  try {
    return JSON.stringify(value, null, 2) ?? String(value);
  } catch {
    return String(value);
  }
}

function addKeyedValue(list: BlockList, key: string, value: unknown, toolName?: string): void {
  if (value === undefined) return;
  const label = humanizeKey(key);
  if (typeof value === "string") {
    const lowerKey = key.toLowerCase();
    const isCode = Object.hasOwn(CODE_KEYS, lowerKey);
    if (!isLongText(value) && !(isCode && lowerKey === "code")) {
      list.field(label, value);
      return;
    }
    if (isCode) {
      const language = CODE_KEYS[lowerKey] ?? toolLanguage(toolName);
      list.push({ kind: "code", label, text: value, ...(language ? { language } : {}) });
    } else {
      list.push({ kind: "text", label, text: value });
    }
    return;
  }
  if (typeof value !== "object" || value === null || isSimpleNested(value)) {
    list.field(label, formatValue(value));
    return;
  }
  list.push({ kind: "code", label, text: json(value), language: "json" });
}

function addValue(list: BlockList, value: unknown, toolName?: string): void {
  if (value === null || value === undefined) return;
  if (typeof value === "string") {
    addText(list, value);
    return;
  }
  if (isRecord(value)) {
    for (const [key, child] of Object.entries(value)) {
      addKeyedValue(list, key, child, toolName);
    }
    return;
  }
  if (typeof value !== "object" || isSimpleNested(value)) {
    list.field("Value", formatValue(value));
    return;
  }
  list.push({ kind: "code", text: json(value), language: "json" });
}

function parseJsonObject(text: string): unknown {
  const trimmed = text.trim();
  if (!trimmed.startsWith("{") && !trimmed.startsWith("[")) return undefined;
  try {
    const parsed = JSON.parse(trimmed) as unknown;
    return typeof parsed === "object" && parsed !== null ? parsed : undefined;
  } catch {
    return undefined;
  }
}

function addText(list: BlockList, text: string): void {
  const trimmed = text.replace(/^\n+|\n+$/g, "");
  if (!trimmed) return;
  if (IMAGE_PLACEHOLDERS.has(trimmed.trim())) {
    list.image();
    return;
  }
  const parsed = parseJsonObject(trimmed);
  if (parsed !== undefined) {
    addValue(list, parsed);
    return;
  }
  list.push(
    isLongText(trimmed) ? { kind: "code", text: trimmed } : { kind: "text", text: trimmed },
  );
}

function isContentBlockList(value: unknown): value is Array<Record<string, unknown>> {
  return (
    Array.isArray(value) &&
    value.length > 0 &&
    value.every((item) => isRecord(item) && typeof item.type === "string")
  );
}

function addContentBlocks(list: BlockList, blocks: Array<Record<string, unknown>>): void {
  for (const block of blocks) {
    if (block.type === "text" && typeof block.text === "string") {
      addText(list, block.text);
    } else if (block.type === "image") {
      list.image();
    } else {
      list.push({ kind: "code", text: json(block), language: "json" });
    }
  }
}

function addOutput(list: BlockList, output: unknown): void {
  if (isContentBlockList(output)) {
    addContentBlocks(list, output);
    return;
  }
  // An MCP result envelope; `structuredContent` repeats the text content as data.
  if (isRecord(output) && isContentBlockList(output.content)) {
    if (output.structuredContent !== undefined && output.structuredContent !== null) {
      addValue(list, output.structuredContent);
    } else {
      addContentBlocks(list, output.content);
    }
    return;
  }
  addValue(list, output);
}

/** Lay out an unknown tool call's input and output; empty groups are omitted. */
export function buildToolDetailGroups(
  toolName: string | undefined,
  input: unknown,
  output: unknown,
): ToolDetailGroup[] {
  const inputList = new BlockList();
  addValue(inputList, input, toolName);
  const outputList = new BlockList();
  addOutput(outputList, output);
  const groups: ToolDetailGroup[] = [];
  if (inputList.blocks.length > 0) groups.push({ role: "input", blocks: inputList.blocks });
  if (outputList.blocks.length > 0) groups.push({ role: "output", blocks: outputList.blocks });
  return groups;
}

function firstLine(text: string): string {
  return text.split("\n").find((line) => line.trim().length > 0) ?? "";
}

/** The argument that best names what an unknown tool call acts on, as one short line. */
export function summarizeUnknownToolInput(input: unknown): string | undefined {
  if (typeof input === "string") return toSummaryText(firstLine(input));
  if (!isRecord(input)) return undefined;
  const preferred = SUMMARY_KEYS.map((key) => input[key]).find(
    (value): value is string => typeof value === "string" && value.trim().length > 0,
  );
  const fallback = Object.values(input).find(
    (value): value is string => typeof value === "string" && value.trim().length > 0,
  );
  const value = preferred ?? fallback;
  return value === undefined ? undefined : toSummaryText(firstLine(value));
}
