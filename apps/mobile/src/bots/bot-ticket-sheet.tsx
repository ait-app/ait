import React, { useState } from "react";
import { Pressable, Text, View } from "react-native";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type {
  BotTicket,
  BotTicketAssignment,
  BotTicketPull,
  BotTicketReport,
  BotTicketUserStatus,
} from "@ait/protocol/bot/types";
import { AdaptiveModalSheet } from "@/components/adaptive-modal-sheet";
import { Button } from "@/components/ui/button";
import { FormTextInput } from "@/components/ui/form-field";
import { useHostRuntimeClient } from "@/runtime/host-runtime";
import { useSessionStore } from "@/stores/session-store";
import { confirmDialog } from "@/utils/confirm-dialog";
import { openExternalUrl } from "@/utils/open-external-url";
import { formatMessageTimestamp } from "@/utils/time";
import { TicketStatusIcon } from "./bot-tickets";
import { BotLoadingSpinner } from "./loading";
import { botKeys, hasBotErrorCode, isBotConflict } from "./queries";
import { botStyles as styles, botTicketStyles as ticketStyles } from "./styles";
import {
  isTicketClosed,
  pullStateKey,
  reportKindKey,
  reportSourceKey,
  ticketStatusKey,
  ticketWaitingKey,
} from "./tickets";

function formatTime(value: string): string {
  const at = new Date(value);
  return Number.isNaN(at.getTime()) ? value : formatMessageTimestamp(at);
}

/** The Agent's title from the Agent directory this client already holds, else a short id. */
function useAgentName(serverId: string, agentId: string): string {
  const { t } = useTranslation();
  const title = useSessionStore((state) => state.sessions[serverId]?.agents.get(agentId)?.title);
  return title?.trim() || t("bots.tickets.agentFallback", { id: agentId.slice(0, 8) });
}

/** An assigned Agent; opens beside the chat when it runs in this Workspace, as wake cards do. */
function AssignedAgent({
  serverId,
  workspaceId,
  assignment,
  onOpenAgent,
}: {
  serverId: string;
  workspaceId: string;
  assignment: BotTicketAssignment;
  onOpenAgent: (agentId: string) => void;
}) {
  const name = useAgentName(serverId, assignment.agentId);
  if (assignment.workspaceId !== workspaceId)
    return (
      <Text style={styles.text} numberOfLines={1}>
        {name}
      </Text>
    );
  return (
    <Pressable
      onPress={() => onOpenAgent(assignment.agentId)}
      accessibilityRole="link"
      testID={`bot-ticket-agent-${assignment.agentId}`}
    >
      <Text style={[styles.text, styles.link]} numberOfLines={1}>
        {name}
      </Text>
    </Pressable>
  );
}

function ReportAuthor({ serverId, report }: { serverId: string; report: BotTicketReport }) {
  const { t } = useTranslation();
  const agent = useAgentName(serverId, report.agentId ?? "");
  const who = t(reportSourceKey(report.source));
  const kind = t(reportKindKey(report.kind));
  const moved = report.status
    ? ` · ${t("bots.tickets.movedTo", { status: t(ticketStatusKey(report.status)) })}`
    : "";
  return (
    <Text style={styles.hint}>
      {`${report.agentId ? `${who} · ${agent}` : who} · ${formatTime(report.at)} · ${kind}${moved}`}
    </Text>
  );
}

/** The kept history, oldest first, each entry saying who wrote it. */
function TicketHistory({ serverId, reports }: { serverId: string; reports: BotTicketReport[] }) {
  const { t } = useTranslation();
  if (reports.length === 0) return <Text style={styles.hint}>{t("bots.tickets.noReports")}</Text>;
  return (
    <>
      {reports.map((report) => (
        <View
          key={report.sequence}
          style={ticketStyles.report}
          testID={`bot-ticket-report-${report.sequence}`}
        >
          <ReportAuthor serverId={serverId} report={report} />
          {report.text.trim() ? (
            <Text style={styles.text} selectable>
              {report.text}
            </Text>
          ) : null}
        </View>
      ))}
    </>
  );
}

/** A pull request the ticket follows: number and title open it, then its state and checks. */
function FollowedPull({ pull }: { pull: BotTicketPull }) {
  const { t } = useTranslation();
  const facts = [t(pullStateKey(pull.state))];
  if (pull.checks && pull.checks.failing > 0)
    facts.push(t("bots.tickets.checksFailing", { count: pull.checks.failing }));
  return (
    <View testID={`bot-ticket-pull-${pull.number}`}>
      <Pressable onPress={() => void openExternalUrl(pull.url)} accessibilityRole="link">
        <Text style={[styles.text, styles.link]} numberOfLines={1}>
          {`#${pull.number} ${pull.title}`}
        </Text>
      </Pressable>
      <Text style={styles.hint}>{facts.join(" · ")}</Text>
      {pull.error ? (
        <Text style={styles.warning}>{t("bots.tickets.pullError", { error: pull.error })}</Text>
      ) : null}
    </View>
  );
}

function TicketBody({
  serverId,
  workspaceId,
  ticket,
  onOpenAgent,
}: {
  serverId: string;
  workspaceId: string;
  ticket: BotTicket;
  onOpenAgent: (agentId: string) => void;
}) {
  const { t } = useTranslation();
  return (
    <View style={ticketStyles.body} testID={`bot-ticket-${ticket.id}`}>
      <View style={ticketStyles.statusLine}>
        <TicketStatusIcon status={ticket.status} />
        <Text
          style={ticket.status === "waiting" ? styles.warning : styles.hint}
          testID="bot-ticket-status"
        >
          {ticket.status === "waiting"
            ? `${t(ticketStatusKey(ticket.status))} · ${t(ticketWaitingKey(ticket.waiting))}`
            : t(ticketStatusKey(ticket.status))}
        </Text>
      </View>
      {ticket.status === "waiting" && ticket.waiting?.question ? (
        <View style={ticketStyles.section}>
          <Text style={ticketStyles.heading}>{t("bots.tickets.question")}</Text>
          <Text style={styles.text} selectable testID="bot-ticket-question">
            {ticket.waiting.question}
          </Text>
        </View>
      ) : null}
      {isTicketClosed(ticket.status) ? (
        <View style={ticketStyles.section}>
          <Text style={ticketStyles.heading}>{t("bots.tickets.outcome")}</Text>
          <Text style={styles.text} selectable testID="bot-ticket-outcome">
            {ticket.outcome?.trim() || t("bots.tickets.noOutcome")}
          </Text>
        </View>
      ) : null}
      <View style={ticketStyles.section}>
        <Text style={ticketStyles.heading}>{t("bots.tickets.request")}</Text>
        <Text style={styles.text} selectable>
          {ticket.request.trim() || ticket.title}
        </Text>
      </View>
      <View style={ticketStyles.section}>
        <Text style={ticketStyles.heading}>{t("bots.tickets.agents")}</Text>
        {ticket.assignments.length === 0 ? (
          <Text style={styles.hint}>{t("bots.tickets.noAgents")}</Text>
        ) : (
          ticket.assignments.map((assignment) => (
            <AssignedAgent
              key={assignment.agentId}
              serverId={serverId}
              workspaceId={workspaceId}
              assignment={assignment}
              onOpenAgent={onOpenAgent}
            />
          ))
        )}
      </View>
      {ticket.pulls.length > 0 ? (
        <View style={ticketStyles.section}>
          <Text style={ticketStyles.heading}>{t("bots.tickets.pulls")}</Text>
          {ticket.pulls.map((pull) => (
            <FollowedPull key={pull.number} pull={pull} />
          ))}
        </View>
      ) : null}
      <View style={ticketStyles.section}>
        <Text style={ticketStyles.heading}>{t("bots.tickets.history")}</Text>
        <TicketHistory serverId={serverId} reports={ticket.reports} />
      </View>
    </View>
  );
}

/**
 * The owner's actions: close an open ticket as done or cancelled with an optional outcome, or
 * reopen a closed one. Cancelling asks first, as resetting a conversation does.
 */
function TicketActions({
  ticket,
  pending,
  onUpdate,
}: {
  ticket: BotTicket;
  pending: boolean;
  onUpdate: (status: BotTicketUserStatus, outcome: string) => void;
}) {
  const { t } = useTranslation();
  const [outcome, setOutcome] = useState("");
  if (isTicketClosed(ticket.status))
    return (
      <View style={ticketStyles.actions}>
        <Button
          variant="outline"
          size="sm"
          disabled={pending}
          onPress={() => onUpdate("open", "")}
          testID="bot-ticket-reopen"
        >
          {t("bots.tickets.reopen")}
        </Button>
      </View>
    );
  // A status a later host adds offers nothing this client can safely change.
  if (ticket.status !== "open" && ticket.status !== "waiting") return null;
  return (
    <View style={ticketStyles.footer}>
      <FormTextInput
        initialValue=""
        onChangeText={setOutcome}
        placeholder={t("bots.tickets.outcomePlaceholder")}
        accessibilityLabel={t("bots.tickets.outcome")}
        editable={!pending}
        maxLength={2048}
        testID="bot-ticket-outcome-input"
      />
      <View style={ticketStyles.actions}>
        <Button
          variant="outline"
          size="sm"
          disabled={pending}
          onPress={() => onUpdate("cancelled", outcome)}
          testID="bot-ticket-cancel"
        >
          {t("bots.tickets.cancel")}
        </Button>
        <Button
          size="sm"
          disabled={pending}
          onPress={() => onUpdate("done", outcome)}
          testID="bot-ticket-done"
        >
          {t("bots.tickets.markDone")}
        </Button>
      </View>
    </View>
  );
}

/** The owner's answer to the question of the blocked worker the ticket waits on. */
function TicketAnswer({
  pending,
  onAnswer,
}: {
  pending: boolean;
  onAnswer: (answer: string) => void;
}) {
  const { t } = useTranslation();
  const [answer, setAnswer] = useState("");
  return (
    <View style={ticketStyles.footer}>
      <FormTextInput
        initialValue=""
        onChangeText={setAnswer}
        placeholder={t("bots.tickets.answerPlaceholder")}
        accessibilityLabel={t("bots.tickets.answerPlaceholder")}
        editable={!pending}
        maxLength={2048}
        testID="bot-ticket-answer-input"
      />
      <View style={ticketStyles.actions}>
        <Button
          size="sm"
          disabled={pending || !answer.trim()}
          onPress={() => onAnswer(answer.trim())}
          testID="bot-ticket-answer"
        >
          {t("bots.tickets.sendAnswer")}
        </Button>
      </View>
    </View>
  );
}

/** One ticket's detail over the Bot chat, current while it is open. */
export function BotTicketSheet({
  serverId,
  workspaceId,
  ticketId,
  visible,
  readOnly,
  onClose,
  onDismiss,
  onOpenAgent,
}: {
  serverId: string;
  workspaceId: string;
  ticketId: string;
  visible: boolean;
  /** Guests read tickets; only the Host owner closes, cancels or reopens them. */
  readOnly: boolean;
  onClose: () => void;
  /** The sheet finished closing and may unmount. */
  onDismiss: () => void;
  onOpenAgent: (agentId: string) => void;
}) {
  const { t } = useTranslation();
  const client = useHostRuntimeClient(serverId);
  const queryClient = useQueryClient();
  const queryKey = botKeys.ticket(serverId, workspaceId, ticketId);
  const detail = useQuery({
    queryKey,
    enabled: visible && Boolean(client),
    staleTime: 0,
    refetchInterval: 5_000,
    queryFn: async () => {
      if (!client) throw new Error(t("bots.disconnected"));
      return (await client.botRequest("bot.ticket.get.request", { workspaceId, ticketId })).ticket;
    },
  });
  const update = useMutation({
    mutationFn: async ({ status, outcome }: { status: BotTicketUserStatus; outcome: string }) => {
      if (readOnly) throw new Error(t("bots.requestFailed"));
      if (!client) throw new Error(t("bots.disconnected"));
      if (status === "cancelled") {
        const confirmed = await confirmDialog({
          title: t("bots.tickets.cancel"),
          message: t("bots.tickets.cancelConfirm"),
          confirmLabel: t("bots.tickets.cancel"),
          cancelLabel: t("bots.tickets.keep"),
        });
        if (!confirmed) return null;
      }
      const text = outcome.trim();
      try {
        return await client.botRequest("bot.ticket.update.request", {
          workspaceId,
          ticketId,
          status,
          ...(status !== "open" && text ? { outcome: text } : {}),
        });
      } catch (cause) {
        if (hasBotErrorCode(cause, "resource_exhausted"))
          throw new Error(t("bots.tickets.openLimit"));
        if (isBotConflict(cause)) {
          // The Bot closed or reopened it meanwhile; show where it stands now.
          void detail.refetch();
          throw new Error(t("bots.tickets.conflict"));
        }
        throw cause;
      }
    },
    onSuccess: (result) => {
      if (!result) return;
      queryClient.setQueryData(queryKey, result.ticket);
      void queryClient.invalidateQueries({ queryKey: ["bot-tickets", serverId, workspaceId] });
    },
  });
  const answerable = useSessionStore(
    (state) => state.sessions[serverId]?.serverInfo?.features?.workspaceBotTicketAnswers === true,
  );
  const answer = useMutation({
    // The answer names the worker whose question the sheet showed, so the host refuses it if the
    // wait passed to another worker meanwhile.
    mutationFn: async ({ agentId, text }: { agentId: string; text: string }) => {
      if (readOnly) throw new Error(t("bots.requestFailed"));
      if (!client) throw new Error(t("bots.disconnected"));
      try {
        return await client.answerWorkspaceBotTicket(workspaceId, ticketId, agentId, text);
      } catch (cause) {
        if (isBotConflict(cause)) {
          void detail.refetch();
          throw new Error(t("bots.tickets.answerConflict"));
        }
        throw cause;
      }
    },
    onSuccess: (result) => {
      queryClient.setQueryData(queryKey, result.ticket);
      void queryClient.invalidateQueries({ queryKey: ["bot-tickets", serverId, workspaceId] });
    },
  });
  const ticket = detail.data;
  const asked = ticket?.status === "waiting" && ticket.waiting?.reason === "blocked";
  const footer =
    ticket && !readOnly ? (
      <View style={ticketStyles.footer}>
        {update.error ? (
          <Text style={styles.error} accessibilityRole="alert">
            {update.error.message}
          </Text>
        ) : null}
        {answer.error ? (
          <Text style={styles.error} accessibilityRole="alert">
            {answer.error.message}
          </Text>
        ) : null}
        {answer.data && !answer.data.delivered ? (
          <Text style={styles.hint} testID="bot-ticket-answer-queued">
            {t("bots.tickets.answerQueued")}
          </Text>
        ) : null}
        {answerable && asked ? (
          <TicketAnswer
            // A new wait starts a fresh answer draft.
            key={ticket.waiting?.since}
            pending={answer.isPending}
            onAnswer={(text) => {
              const agentId = ticket.waiting?.agentId;
              if (agentId) answer.mutate({ agentId, text });
            }}
          />
        ) : null}
        <TicketActions
          // A new status starts a fresh outcome draft.
          key={ticket.status}
          ticket={ticket}
          pending={update.isPending}
          onUpdate={(status, outcome) => update.mutate({ status, outcome })}
        />
      </View>
    ) : null;
  return (
    <AdaptiveModalSheet
      header={{ title: ticket?.title ?? t("bots.tickets.title") }}
      visible={visible}
      onClose={onClose}
      onDismiss={onDismiss}
      footer={footer}
      desktopMaxWidth={640}
      testID="bot-ticket-sheet"
    >
      {detail.isPending ? <BotLoadingSpinner /> : null}
      {detail.error ? (
        <Text style={styles.error} accessibilityRole="alert">
          {detail.error.message}
        </Text>
      ) : null}
      {ticket ? (
        <TicketBody
          serverId={serverId}
          workspaceId={workspaceId}
          ticket={ticket}
          onOpenAgent={onOpenAgent}
        />
      ) : null}
    </AdaptiveModalSheet>
  );
}
