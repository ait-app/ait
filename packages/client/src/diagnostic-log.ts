import type { ZodError } from "zod";

const SECRET_FIELD = /password|token|secret|authorization|api[_-]?key/i;
const MAX_LOG_LENGTH = 8192;

function redact(value: string): string {
  return value
    .replace(/\b(Bearer|Basic)\s+[A-Za-z0-9._~+/-]+=*/gi, "$1 [redacted]")
    .replace(
      /((?:access|refresh)[_-]?token|api[_-]?key|password|secret|authorization)["']?\s*[:=]\s*("[^"]*"|'[^']*'|[^\s,}]+)/gi,
      "$1=[redacted]",
    )
    .replace(/([?&](?:password|token|secret|key)=)[^&\s"']+/gi, "$1[redacted]")
    .replace(/(\w+:\/\/)[^\s/@]+@/g, "$1[redacted]@");
}

/** Describe schema failures without recording rejected payloads or user content. */
export function describeValidationIssues(error: ZodError) {
  const result: { code: string; path: PropertyKey[]; expected?: unknown; values?: unknown[] }[] =
    [];
  const collect = (issues: ZodError["issues"], prefix: PropertyKey[] = []) => {
    for (const issue of issues) {
      if (result.length >= 16) return;
      const path = [...prefix, ...issue.path];
      if (issue.code === "invalid_union" && issue.errors.length) {
        for (const branch of issue.errors) collect(branch, path);
      } else {
        result.push({
          code: issue.code,
          path,
          ...("expected" in issue ? { expected: issue.expected } : {}),
          ...("values" in issue ? { values: issue.values.slice(0, 8) } : {}),
        });
      }
    }
  };
  collect(error.issues);
  return result;
}

/** Preserve structured warning context through Electron's string-only console forwarding. */
export function formatDiagnosticFields(fields: object): string {
  const seen = new WeakSet<object>();
  try {
    const serialized = JSON.stringify(fields, (key, value: unknown) => {
      if (SECRET_FIELD.test(key)) return "[redacted]";
      if (typeof value === "string") return redact(value);
      if (typeof value === "bigint") return value.toString();
      if (value && typeof value === "object") {
        if (seen.has(value)) return "[Circular]";
        seen.add(value);
        if (value instanceof Error) {
          return { ...value, name: value.name, message: value.message, stack: value.stack };
        }
      }
      return value;
    });
    return serialized.length > MAX_LOG_LENGTH
      ? `${serialized.slice(0, MAX_LOG_LENGTH)} [truncated]`
      : serialized;
  } catch {
    return '{"diagnostic":"unserializable fields"}';
  }
}
