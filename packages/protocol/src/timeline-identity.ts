import type { AgentTimelineItem } from "./agent-types.js";

/** Decode the additive command metadata carried by the legacy-compatible text envelope. */
export function unwrapTimelineItem(item: AgentTimelineItem): AgentTimelineItem {
  return item.type === "assistant_message" && item.providerCommand
    ? { ...item.providerCommand, type: "provider_command", text: item.text }
    : item;
}

export function timelineItemIdentity(wireItem: AgentTimelineItem): string | null {
  const item = unwrapTimelineItem(wireItem);
  if (item.type === "voice") return `voice:${item.sessionId}/${item.id}`;
  if (item.type === "tool_call") return item.callId;
  if (item.type === "plugin") return `${item.pluginId}/${item.id}`;
  if (item.type === "provider_command") return `provider-command:${item.id}`;
  return null;
}
