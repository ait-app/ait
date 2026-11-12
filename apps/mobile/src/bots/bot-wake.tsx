import React from "react";
import { Pressable, Text, View } from "react-native";
import { useTranslation } from "react-i18next";
import {
  Bell,
  CircleAlert,
  CircleCheck,
  CircleX,
  Hand,
  ListTodo,
  SquareTerminal,
} from "lucide-react-native";
import { withUnistyles } from "react-native-unistyles";
import type { BotNotification, BotTurn, BotTurnMessage } from "@ait/protocol/bot/types";
import type { Theme } from "@/styles/theme";
import type { WorkspaceTabTarget } from "@/workspace-tabs/model";
import {
  botChatStyles as chatStyles,
  botStyles as styles,
  botTicketStyles as ticketStyles,
} from "./styles";

/** The exact reply a Bot gives when a wake-up needs no message (ADR-271 §8). */
export const SILENT_REPLY = "NO_REPLY";

const ThemedBell = withUnistyles(Bell);
const ThemedCircleCheck = withUnistyles(CircleCheck);
const ThemedCircleX = withUnistyles(CircleX);
const ThemedCircleAlert = withUnistyles(CircleAlert);
const ThemedSquareTerminal = withUnistyles(SquareTerminal);
const ThemedHand = withUnistyles(Hand);
const ThemedListTodo = withUnistyles(ListTodo);
const mutedColor = (theme: Theme) => ({ color: theme.colors.foregroundMuted });
const successColor = (theme: Theme) => ({ color: theme.colors.statusSuccess });
const dangerColor = (theme: Theme) => ({ color: theme.colors.statusDanger });
const warningColor = (theme: Theme) => ({ color: theme.colors.statusWarning });

const KIND_LABEL_KEYS: Record<string, string> = {
  agent_finished: "bots.notifications.kind.agent_finished",
  agent_failed: "bots.notifications.kind.agent_failed",
  agent_permission: "bots.notifications.kind.agent_permission",
  schedule_succeeded: "bots.notifications.kind.schedule_succeeded",
  schedule_failed: "bots.notifications.kind.schedule_failed",
  terminal_attention: "bots.notifications.kind.terminal_attention",
  ticket_blocked: "bots.notifications.kind.ticket_blocked",
  ticket_done: "bots.notifications.kind.ticket_done",
  ticket_failed: "bots.notifications.kind.ticket_failed",
};

/** Localized name of a notification kind; kinds from later hosts read as a generic update. */
export function notificationKindLabel(kind: string, t: (key: string) => string): string {
  return t(KIND_LABEL_KEYS[kind] ?? "bots.notifications.kind.other");
}

export function NotificationKindIcon({ kind }: { kind: string }) {
  switch (kind) {
    case "agent_finished":
    case "schedule_succeeded":
    case "ticket_done":
      return <ThemedCircleCheck size={14} uniProps={successColor} />;
    case "agent_failed":
    case "schedule_failed":
    case "ticket_failed":
      return <ThemedCircleX size={14} uniProps={dangerColor} />;
    case "agent_permission":
      return <ThemedCircleAlert size={14} uniProps={warningColor} />;
    case "ticket_blocked":
      return <ThemedHand size={14} uniProps={warningColor} />;
    case "terminal_attention":
      return <ThemedSquareTerminal size={14} uniProps={mutedColor} />;
    default:
      return <ThemedBell size={14} uniProps={mutedColor} />;
  }
}

/**
 * Where a notification's subject opens in this Workspace, if the app can show it. Subjects in
 * another Workspace of the Bot's scope stay plain text, because this tab's layout cannot hold them.
 */
export function notificationTarget(
  notification: BotNotification,
  workspaceId: string,
): WorkspaceTabTarget | null {
  if (notification.workspaceId !== workspaceId) return null;
  if (notification.kind.startsWith("schedule_") && notification.scheduleId)
    return { kind: "schedule", scheduleId: notification.scheduleId };
  if (notification.agentId) return { kind: "agent", agentId: notification.agentId };
  return null;
}

/**
 * Assistant messages of a turn as the chat shows them. A wake turn never shows the silent
 * sentinel; while it streams, text that may still become the sentinel stays hidden so it never
 * flashes before the host marks the turn silent.
 */
export function visibleBotMessages(turn: BotTurn, pending: boolean): BotTurnMessage[] {
  const messages: BotTurnMessage[] =
    turn.messages.length > 0
      ? turn.messages
      : turn.reply
        ? [{ id: "legacy", text: turn.reply, status: "completed" }]
        : [];
  if (!turn.wake) return messages;
  if (turn.silent) return [];
  return messages.filter((message) => {
    const text = message.text.trim();
    if (text === SILENT_REPLY) return false;
    return !(pending && text.length > 0 && SILENT_REPLY.startsWith(text));
  });
}

/**
 * The task a wake notification's subject works on. It opens the task when the host serves tickets;
 * otherwise the title is plain text.
 */
function WakeTicket({
  notification,
  onOpenTicket,
}: {
  notification: BotNotification;
  onOpenTicket?: (ticketId: string) => void;
}) {
  const { t } = useTranslation();
  const ticketId = notification.ticketId;
  if (!ticketId) return null;
  const label = t("bots.tickets.linked", {
    title: notification.ticketTitle?.trim() || t("bots.tickets.untitled"),
  });
  return (
    <View style={[chatStyles.wakeEntryLine, ticketStyles.wakeTicket]}>
      <ThemedListTodo size={14} uniProps={mutedColor} />
      {onOpenTicket ? (
        <Pressable
          style={styles.grow}
          onPress={() => onOpenTicket(ticketId)}
          accessibilityRole="link"
          testID={`bot-wake-ticket-${notification.id}`}
        >
          <Text style={[styles.hint, styles.link]} numberOfLines={1}>
            {label}
          </Text>
        </Pressable>
      ) : (
        <Text
          style={[styles.hint, styles.grow]}
          numberOfLines={1}
          testID={`bot-wake-ticket-${notification.id}`}
        >
          {label}
        </Text>
      )}
    </View>
  );
}

/** Header of a wake turn: what happened, in place of a user message. */
export function BotWakeHeader({
  turn,
  workspaceId,
  onOpen,
  onOpenTicket,
}: {
  turn: BotTurn;
  workspaceId: string;
  onOpen: (target: WorkspaceTabTarget) => void;
  /** Absent when the host serves no tickets, so a named task stays plain text. */
  onOpenTicket?: (ticketId: string) => void;
}) {
  const { t } = useTranslation();
  const notifications = turn.wake?.notifications ?? [];
  return (
    <View style={chatStyles.wake} testID={`bot-wake-${turn.id}`}>
      <View style={chatStyles.wakeTitle}>
        <ThemedBell size={14} uniProps={mutedColor} />
        <Text style={styles.hint}>{t("bots.notifications.wokeBy")}</Text>
      </View>
      {notifications.map((notification) => {
        const target = notificationTarget(notification, workspaceId);
        const label = notificationKindLabel(notification.kind, t);
        return (
          <View key={notification.id} style={chatStyles.wakeEntry}>
            <View style={chatStyles.wakeEntryLine}>
              <View accessibilityLabel={label} accessible>
                <NotificationKindIcon kind={notification.kind} />
              </View>
              {target ? (
                <Pressable
                  style={styles.grow}
                  onPress={() => onOpen(target)}
                  accessibilityRole="link"
                  accessibilityHint={label}
                  testID={`bot-wake-open-${notification.id}`}
                >
                  <Text style={[styles.text, styles.link]} numberOfLines={2}>
                    {notification.title}
                  </Text>
                </Pressable>
              ) : (
                <Text style={[styles.text, styles.grow]} numberOfLines={2}>
                  {notification.title}
                </Text>
              )}
            </View>
            {notification.summary ? (
              <Text style={[styles.hint, chatStyles.wakeSummary]} numberOfLines={2}>
                {notification.summary}
              </Text>
            ) : null}
            <WakeTicket notification={notification} onOpenTicket={onOpenTicket} />
          </View>
        );
      })}
    </View>
  );
}

/** A wake turn the Bot judged not worth a message: one subdued line instead of a reply. */
export function BotSilentNote({ turnId }: { turnId: string }) {
  const { t } = useTranslation();
  return (
    <Text style={chatStyles.turnStatusText} testID={`bot-silent-${turnId}`}>
      {t("bots.notifications.silent")}
    </Text>
  );
}
