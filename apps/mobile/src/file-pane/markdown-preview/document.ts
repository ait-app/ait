import { parseDocument, stringify } from "yaml";
import { isRecord } from "@/utils/is-record";

export interface MarkdownFrontMatterRow {
  key: string;
  value: string;
}

export interface MarkdownPreviewDocument {
  frontMatter: MarkdownFrontMatterRow[];
  body: string;
}

const FRONT_MATTER_PATTERN = /^\uFEFF?---[ \t]*\r?\n([\s\S]*?)\r?\n---[ \t]*(?:\r?\n|$)/;

export function parseMarkdownPreviewDocument(source: string): MarkdownPreviewDocument {
  const match = FRONT_MATTER_PATTERN.exec(source);
  if (!match) {
    return { frontMatter: [], body: source };
  }

  try {
    return parseFrontMatter(match, source);
  } catch {
    return { frontMatter: [], body: source };
  }
}

function parseFrontMatter(match: RegExpExecArray, source: string): MarkdownPreviewDocument {
  const document = parseDocument(match[1], { schema: "core" });
  if (document.errors.length > 0) {
    return { frontMatter: [], body: source };
  }

  const metadata = document.toJS({ maxAliasCount: 100 });
  if (!isRecord(metadata)) {
    return { frontMatter: [], body: source };
  }

  const frontMatter = Object.entries(metadata).map(([key, value]) => ({
    key,
    value: formatFrontMatterValue(value),
  }));

  return {
    frontMatter,
    body: source.slice(match[0].length),
  };
}

function formatFrontMatterValue(value: unknown): string {
  if (Array.isArray(value) && value.every(isScalar)) {
    return value.map(formatScalar).join(", ");
  }
  if (isScalar(value)) {
    return formatScalar(value);
  }
  return stringify(value).trim();
}

function isScalar(value: unknown): value is string | number | boolean | null {
  return value === null || ["string", "number", "boolean"].includes(typeof value);
}

function formatScalar(value: string | number | boolean | null): string {
  return value === null ? "null" : String(value);
}
