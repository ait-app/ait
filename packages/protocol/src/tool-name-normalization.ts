const TOOL_TOKEN_REGEX = /[a-z0-9]+/g;

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

/** MCP server name the daemon registers its own agent tools under. */
const AIT_TOOL_NAMESPACE = "ait";

function isAitToolNamespace(segment: string): boolean {
  return segment === AIT_TOOL_NAMESPACE || segment.startsWith(`${AIT_TOOL_NAMESPACE}_`);
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
