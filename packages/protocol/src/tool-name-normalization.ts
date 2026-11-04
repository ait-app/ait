const TOOL_TOKEN_REGEX = /[a-z0-9]+/g;
const STANDARD_NAMESPACE_SEPARATOR_REGEX = /[.:/]/;

export function normalizeToolName(name: string): string {
  return name.trim().toLowerCase();
}

export function tokenizeToolName(name: string): string[] {
  const normalized = normalizeToolName(name);
  return normalized.match(TOOL_TOKEN_REGEX) ?? [];
}

export function getToolLeafName(name: string): string | null {
  const tokens = tokenizeToolName(name);
  return tokens.length > 0 ? tokens[tokens.length - 1] : null;
}

export function isSpeakToolName(name: string): boolean {
  return getToolLeafName(name) === "speak";
}

export function isLikelyNamespacedToolName(name: string): boolean {
  const normalized = normalizeToolName(name);
  if (STANDARD_NAMESPACE_SEPARATOR_REGEX.test(normalized)) {
    return true;
  }
  if (!normalized.includes("__")) {
    return false;
  }

  // Keep `__` handling strict to avoid false positives on arbitrary custom names.
  const segments = normalized.split("__").filter((segment) => segment.length > 0);
  if (segments.length >= 3) {
    return true;
  }
  if (segments.length === 2 && segments[1].includes("_")) {
    return true;
  }
  return false;
}

/**
 * MCP server names the daemon's own agent tools are exposed under. The daemon
 * registers itself as `ait`; `paseo` remains for transcripts recorded before
 * the rename and sessions imported from Paseo.
 *
 * TODO(0.2.0): drop the legacy `paseo` alias here, in `DIRECT_AIT_TOOL_PREFIXES`
 * (apps/mobile tool-call overview) and in the Rust `AIT_TOOL_NAMESPACES`
 * (crates/provider fork context); see docs/plans/README.md.
 */
const AIT_TOOL_NAMESPACES = ["ait", "paseo"] as const;

function isAitToolNamespace(segment: string): boolean {
  return AIT_TOOL_NAMESPACES.some(
    (namespace) => segment === namespace || segment.startsWith(`${namespace}_`),
  );
}

export function isAitToolName(name: string): boolean {
  const normalized = normalizeToolName(name);
  if (isSpeakToolName(normalized)) {
    return false;
  }
  return getAitToolLeafName(normalized) !== null;
}

export function getAitToolLeafName(name: string): string | null {
  const normalized = normalizeToolName(name);
  if (normalized.includes("__")) {
    const segments = normalized.split("__").filter((s) => s.length > 0);
    if (segments.length >= 3 && segments[0] === "mcp" && isAitToolNamespace(segments[1])) {
      return segments.slice(2).join("__");
    }
    return null;
  }
  if (normalized.includes(".")) {
    const [namespace, ...rest] = normalized.split(".");
    return isAitToolNamespace(namespace) ? rest.join(".") : null;
  }
  return null;
}

export function isLikelyExternalToolName(name: string): boolean {
  const normalized = normalizeToolName(name);
  if (!normalized) {
    return false;
  }
  if (isSpeakToolName(normalized)) {
    return true;
  }
  return isLikelyNamespacedToolName(normalized);
}
