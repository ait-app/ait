import type { ProviderUiState } from "@ait/protocol/messages";

export type NativeAction = NonNullable<ProviderUiState["actions"]>[number];
export type NativeField = NativeAction["fields"][number];
export type NativeArguments = Record<string, string>;
export type NativeGroup = "conversation" | "history" | "tasks" | "accounts" | "runtime";

// Native APIs stay available; GUI preset writes belong exclusively to the settings editor.
export function nativeActionGroup(id: string): NativeGroup | null {
  if (
    id.startsWith("model_preset_") ||
    ["get_subagents", "steer_subagent", "cancel_subagent", "btw_cancel"].includes(id)
  )
    return null;
  if (["get_login_providers", "login", "get_logout_accounts", "logout"].includes(id))
    return "accounts";
  if (id.startsWith("set_") && !["set_label", "set_session_name"].includes(id)) return "runtime";
  if (
    [
      "branch",
      "fork",
      "navigate_tree",
      "set_label",
      "get_entries",
      "get_tree",
      "get_branch_messages",
      "abort_branch_summary",
    ].includes(id)
  )
    return "history";
  if (
    [
      "vibe_control",
      "goal",
      "get_subagents",
      "steer_subagent",
      "cancel_subagent",
      "btw",
      "btw_cancel",
      "get_btw_history",
    ].includes(id)
  )
    return "tasks";
  return "conversation";
}

const vibeFields: Record<string, readonly string[]> = {
  list: [],
  spawn: ["cli", "prompt", "name"],
  send: ["sessionId", "message"],
  wait: ["sessions", "timeout"],
  kill: ["sessionId"],
  toggle: ["enabled"],
};

/** Project only the current operation's parameters, including its conditional requirements. */
export function nativeActionFields(action: NativeAction, values: NativeArguments): NativeField[] {
  if (action.id === "btw") return action.fields.filter((field) => field.id !== "recordId");
  const selector = action.id === "vibe_control" ? "operation" : action.id === "goal" ? "op" : null;
  if (!selector) return action.fields;
  const operation = values[selector];
  const allowed =
    action.id === "vibe_control"
      ? (vibeFields[operation ?? ""] ?? [])
      : operation === "create"
        ? ["objective", "token_budget"]
        : [];
  return action.fields
    .filter((field) => field.id === selector || allowed.includes(field.id))
    .map((field) => ({
      ...field,
      required:
        field.required ||
        (action.id === "vibe_control" &&
          ["cli", "prompt", "sessionId", "message", "enabled"].includes(field.id)) ||
        (action.id === "goal" && field.id === "objective"),
    }));
}

export function nativeActionArguments(
  action: NativeAction,
  values: NativeArguments,
): NativeArguments {
  return Object.fromEntries(
    nativeActionFields(action, values)
      .filter((field) => values[field.id] !== undefined && values[field.id] !== "")
      .map((field) => [field.id, values[field.id]!]),
  );
}

export function nativeActionValid(action: NativeAction, values: NativeArguments): boolean {
  return nativeActionFields(action, values).every((field) => {
    const value = values[field.id] ?? "";
    if (!value.trim()) return !field.required;
    if (value.length > 16384 || value.includes("\0")) return false;
    if (field.options.length && !field.options.includes(value)) return false;
    if (field.id === "token_budget")
      return /^\d+$/.test(value) && Number.isSafeInteger(+value) && +value > 0;
    if (field.id === "timeout") return Number.isFinite(+value) && +value > 0 && +value <= 600;
    return true;
  });
}

export function nativeOperationKey(action: NativeAction, values: NativeArguments): string | null {
  if (action.id === "vibe_control") return values.operation ?? null;
  if (action.id === "goal") return values.op ?? null;
  return null;
}

export function nativeObjectQuery(
  field: string,
  values: NativeArguments,
): { action: string; arguments: NativeArguments; collection: string } | null {
  switch (field) {
    case "entryId":
      return { action: "get_entries", arguments: {}, collection: "entries" };
    case "subagentId":
      return { action: "get_subagents", arguments: {}, collection: "subagents" };
    case "recordId":
      return { action: "get_btw_history", arguments: {}, collection: "records" };
    case "providerId":
      return { action: "get_login_providers", arguments: {}, collection: "providers" };
    case "credentialId":
      return values.providerId
        ? {
            action: "get_logout_accounts",
            arguments: { providerId: values.providerId },
            collection: "accounts",
          }
        : null;
    default:
      return null;
  }
}

export type NativeObjectOption = { id: string; label: string; description?: string };

function record(value: unknown): Record<string, unknown> | null {
  return value !== null && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
}

/** Read native object lists without ever offering an editable internal identifier. */
export function nativeObjectOptions(
  text: string,
  collection: string,
  fallbackLabel = "Item",
  includeUnavailable = true,
): NativeObjectOption[] {
  let data: unknown;
  try {
    data = JSON.parse(text);
  } catch {
    throw new SyntaxError("Incomplete native object list");
  }
  const container = record(data);
  const items = Array.isArray(data) ? data : container?.[collection];
  if (!Array.isArray(items)) throw new SyntaxError("Incomplete native object list");
  return items.flatMap((value, index) => {
    if (typeof value === "string" && collection === "providers")
      return [{ id: value, label: value }];
    const item = record(value);
    if (!item || (collection === "providers" && item.available === false && !includeUnavailable))
      return [];
    const id = collection === "accounts" ? item.credentialId : item.id;
    if (typeof id !== "string" && typeof id !== "number") return [];
    const message = record(item.message);
    const blocks = Array.isArray(message?.content) ? message.content : [];
    const content =
      typeof message?.content === "string"
        ? message.content
        : blocks.map((block) => record(block)?.text ?? "").join(" ");
    const label = [
      item.label,
      item.name,
      item.description,
      item.question,
      content,
      item.type,
      item.providerId,
    ].find((part) => typeof part === "string" && part.trim());
    return [
      {
        id: String(id),
        label:
          typeof label === "string"
            ? label.replace(/\s+/g, " ").slice(0, 160)
            : `${fallbackLabel} ${index + 1}`,
        ...(typeof item.status === "string" ? { description: item.status } : {}),
      },
    ];
  });
}
