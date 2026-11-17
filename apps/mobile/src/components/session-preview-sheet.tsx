import React, { useCallback, useId, useMemo } from "react";
import { Pressable, ScrollView, Text, View } from "react-native";
import { useInfiniteQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import type {
  DaemonClient,
  FetchProviderSessionPreviewEntry,
  FetchRecentProviderSessionEntry,
} from "@ait/client/internal/daemon-client";
import type { AgentTimelineItem } from "@ait/protocol/agent-types";
import { ChevronUp, Eye } from "lucide-react-native";
import { StyleSheet, useUnistyles } from "react-native-unistyles";
import { AdaptiveModalSheet, type SheetHeader } from "@/components/adaptive-modal-sheet";
import { Button } from "@/components/ui/button";
import { LoadingSpinner } from "@/components/ui/loading-spinner";
import { MarkdownRenderer } from "@/components/markdown/renderer";
import { formatTimeAgo } from "@/utils/time";
import { useHostRuntimeSnapshot } from "@/runtime/host-runtime";

const PREVIEW_SNAP_POINTS = ["70%", "92%"];
const PREVIEW_PAGE_SIZE = 50;

type PreviewClient = Pick<DaemonClient, "fetchProviderSessionPreview">;

interface SessionPreviewSheetProps {
  visible: boolean;
  client: PreviewClient | null;
  serverId: string | null;
  /** The listed native session; identity fields are echoed verbatim, never parsed. */
  entry: FetchRecentProviderSessionEntry | null;
  onClose: () => void;
}

/** Read-only observation badge; previewing never claims or restores the native writer. */
function ActiveNotice({ active }: { active: boolean }) {
  const { t } = useTranslation();
  const { theme } = useUnistyles();
  if (!active) return null;
  return (
    <View style={styles.activeNotice} testID="session-preview-active-notice">
      <Eye size={theme.iconSize.sm} color={theme.colors.foregroundMuted} />
      <Text style={styles.activeNoticeText}>{t("importSession.sessionPreview.activeNotice")}</Text>
    </View>
  );
}

function ToolCallSummary({ item }: { item: Extract<AgentTimelineItem, { type: "tool_call" }> }) {
  const { t } = useTranslation();
  const statusLabel =
    item.status === "failed"
      ? t("importSession.sessionPreview.toolFailed")
      : item.status === "canceled"
        ? t("importSession.sessionPreview.toolCanceled")
        : item.status === "running"
          ? t("importSession.sessionPreview.toolRunning")
          : t("importSession.sessionPreview.toolCompleted");
  return (
    <View style={styles.toolRow}>
      <Text style={styles.toolName} numberOfLines={1}>
        {item.name}
      </Text>
      <Text style={styles.toolStatus}>{statusLabel}</Text>
    </View>
  );
}

function PreviewItem({ entry }: { entry: FetchProviderSessionPreviewEntry }) {
  const { t } = useTranslation();
  const item = entry.item;
  switch (item.type) {
    case "user_message":
      return (
        <View style={styles.userBlock} testID={`session-preview-user-${entry.key}`}>
          <Text style={styles.roleLabel}>{t("importSession.sessionPreview.you")}</Text>
          <Text style={styles.userText}>{item.text}</Text>
        </View>
      );
    case "assistant_message":
      return (
        <View style={styles.assistantBlock} testID={`session-preview-assistant-${entry.key}`}>
          <Text style={styles.roleLabel}>{t("importSession.sessionPreview.assistant")}</Text>
          <MarkdownRenderer text={item.text} />
        </View>
      );
    case "reasoning":
      return (
        <View style={styles.reasoningBlock}>
          <Text style={styles.reasoningText}>{item.text}</Text>
        </View>
      );
    case "tool_call":
      return <ToolCallSummary item={item} />;
    case "todo":
      return (
        <View style={styles.todoBlock}>
          {item.items.map((todo, index) => (
            <Text key={todo.id ?? index} style={styles.todoText}>
              {todo.completed ? "• " : "◦ "}
              {todo.text}
            </Text>
          ))}
        </View>
      );
    case "error":
      return <Text style={styles.errorText}>{item.message}</Text>;
    case "notification":
      return <Text style={styles.notificationText}>{item.message}</Text>;
    case "compaction":
      return (
        <Text style={styles.compactionText}>{t("importSession.sessionPreview.compacted")}</Text>
      );
    default:
      return null;
  }
}

/**
 * Read-only native history preview. Every request is a typed preview read against the same
 * fixed native cut; no import, resume, prompt or writer activation is reachable from here.
 */
export function SessionPreviewSheet(props: SessionPreviewSheetProps) {
  return props.visible && props.entry ? <OpenSessionPreview {...props} /> : null;
}

function OpenSessionPreview({
  visible,
  client,
  serverId,
  entry,
  onClose,
}: SessionPreviewSheetProps) {
  const { t } = useTranslation();
  const { theme } = useUnistyles();
  // An explicit opening owns one cut. Reopening must not reuse Infinity-stale cached pages.
  const openingId = useId();
  const entryKey = entry ? `${entry.providerId}:${entry.providerHandleId}:${entry.cwd}` : null;
  // A different row starts a fresh preview; cursors never cross sessions.
  // A same-host reconnect advances the runtime epoch; cached pages and cursors from the
  // previous connection must not mix with fresh reads (workspace-screen query convention).
  const hostSnapshot = useHostRuntimeSnapshot(serverId ?? "");

  const query = useInfiniteQuery({
    queryKey: [
      "native-session-preview",
      serverId,
      entryKey,
      hostSnapshot?.clientGeneration ?? 0,
      hostSnapshot?.connectionEpoch ?? 0,
      openingId,
    ],
    enabled: Boolean(visible && client && entry),
    initialPageParam: null as string | null,
    retry: false,
    gcTime: 0,
    queryFn: async ({ pageParam, signal }) => {
      if (!client || !entry) {
        throw new Error(t("workspace.terminal.hostDisconnected"));
      }
      const page = await client.fetchProviderSessionPreview({
        providerId: entry.providerId,
        providerHandleId: entry.providerHandleId,
        cwd: entry.cwd,
        limit: PREVIEW_PAGE_SIZE,
        ...(pageParam ? { cursor: pageParam } : {}),
      });
      // A cancelled query (sheet closed, row switched, host reconnected) must not settle pages.
      if (signal.aborted) throw new Error("Preview query cancelled");
      return page;
    },
    getNextPageParam: (lastPage) => (lastPage.hasOlder ? lastPage.olderCursor : undefined),
  });

  const pages = query.data?.pages ?? [];
  const newest = pages[0];
  // Pages arrive newest-first and each page's entries are newest-first; flatten the whole
  // accumulated window once, then reverse, so older pages render before newer ones.
  const entries = useMemo(() => pages.flatMap((page) => page.entries).reverse(), [pages]);

  const handleLoadOlder = useCallback(() => {
    if (query.hasNextPage && !query.isFetchingNextPage) {
      void query.fetchNextPage();
    }
  }, [query]);

  const header = useMemo<SheetHeader>(
    () => ({
      title: newest?.title ?? entry?.title ?? t("importSession.sessionPreview.title"),
      subtitle: (
        <Text style={styles.subtitle} numberOfLines={1}>
          {entry ? formatTimeAgo(new Date(newest?.lastActivityAt ?? entry.lastActivityAt)) : ""}
        </Text>
      ),
    }),
    [newest, entry, t],
  );

  const isLoading = query.isLoading || query.isPending;
  const errorMessage = query.isError ? t("importSession.sessionPreview.failed") : null;

  return (
    <AdaptiveModalSheet
      visible={visible}
      onClose={onClose}
      header={header}
      testID="session-preview-sheet"
      desktopMaxWidth={640}
      snapPoints={PREVIEW_SNAP_POINTS}
      footer={
        query.hasNextPage ? (
          <Button
            variant="ghost"
            onPress={handleLoadOlder}
            disabled={query.isFetchingNextPage}
            testID="session-preview-load-older"
          >
            {query.isFetchingNextPage
              ? t("importSession.sessionPreview.loadingOlder")
              : t("importSession.sessionPreview.loadOlder")}
          </Button>
        ) : undefined
      }
    >
      <ActiveNotice active={newest?.active ?? false} />
      {isLoading ? (
        <View style={styles.center} testID="session-preview-loading">
          <LoadingSpinner color={theme.colors.foregroundMuted} />
        </View>
      ) : null}
      {errorMessage ? (
        <View style={styles.center} testID="session-preview-error">
          <Text style={styles.errorText}>{errorMessage}</Text>
          <Button
            variant="ghost"
            onPress={() => void query.refetch()}
            testID="session-preview-retry"
          >
            {t("importSession.sessionPreview.retry")}
          </Button>
        </View>
      ) : null}
      {!isLoading && !errorMessage && entries.length === 0 ? (
        <View style={styles.center} testID="session-preview-empty">
          <Text style={styles.emptyText}>{t("importSession.sessionPreview.empty")}</Text>
        </View>
      ) : null}
      {entries.length > 0 ? (
        <ScrollView style={styles.list} contentContainerStyle={styles.listContent}>
          {query.hasNextPage || query.isFetchingNextPage ? (
            <Pressable
              onPress={handleLoadOlder}
              disabled={query.isFetchingNextPage}
              style={styles.olderHint}
              accessibilityRole="button"
              testID="session-preview-older-hint"
            >
              <ChevronUp size={theme.iconSize.sm} color={theme.colors.foregroundMuted} />
              <Text style={styles.olderHintText}>
                {t("importSession.sessionPreview.loadOlder")}
              </Text>
            </Pressable>
          ) : null}
          {entries.map((previewEntry) => (
            <PreviewItem key={previewEntry.key} entry={previewEntry} />
          ))}
        </ScrollView>
      ) : null}
    </AdaptiveModalSheet>
  );
}

const styles = StyleSheet.create((theme) => ({
  subtitle: {
    color: theme.colors.foregroundMuted,
    fontSize: theme.fontSize.sm,
  },
  activeNotice: {
    flexDirection: "row",
    alignItems: "center",
    gap: theme.spacing[2],
    paddingVertical: theme.spacing[2],
    paddingHorizontal: theme.spacing[3],
    borderRadius: theme.borderRadius.md,
    backgroundColor: theme.colors.surface1,
    marginBottom: theme.spacing[2],
  },
  activeNoticeText: {
    flexShrink: 1,
    color: theme.colors.foregroundMuted,
    fontSize: theme.fontSize.sm,
  },
  center: {
    alignItems: "center",
    justifyContent: "center",
    gap: theme.spacing[3],
    paddingVertical: theme.spacing[6],
  },
  list: {
    flex: 1,
  },
  listContent: {
    gap: theme.spacing[3],
    paddingBottom: theme.spacing[4],
  },
  olderHint: {
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "center",
    gap: theme.spacing[1],
    paddingVertical: theme.spacing[2],
  },
  olderHintText: {
    color: theme.colors.foregroundMuted,
    fontSize: theme.fontSize.sm,
  },
  roleLabel: {
    color: theme.colors.foregroundMuted,
    fontSize: theme.fontSize.sm,
    marginBottom: theme.spacing[1],
  },
  userBlock: {
    alignSelf: "flex-end",
    maxWidth: "85%",
    padding: theme.spacing[3],
    borderRadius: theme.borderRadius.lg,
    backgroundColor: theme.colors.surface1,
  },
  userText: {
    color: theme.colors.foreground,
    fontSize: theme.fontSize.base,
  },
  assistantBlock: {
    alignSelf: "stretch",
  },
  reasoningBlock: {
    borderLeftWidth: 2,
    borderLeftColor: theme.colors.border,
    paddingLeft: theme.spacing[3],
  },
  reasoningText: {
    color: theme.colors.foregroundMuted,
    fontSize: theme.fontSize.sm,
    fontStyle: "italic",
  },
  toolRow: {
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "space-between",
    gap: theme.spacing[2],
    paddingVertical: theme.spacing[2],
    paddingHorizontal: theme.spacing[3],
    borderRadius: theme.borderRadius.md,
    borderWidth: 1,
    borderColor: theme.colors.border,
  },
  toolName: {
    flexShrink: 1,
    color: theme.colors.foreground,
    fontSize: theme.fontSize.sm,
    fontFamily: theme.fontFamily.mono,
  },
  toolStatus: {
    color: theme.colors.foregroundMuted,
    fontSize: theme.fontSize.sm,
  },
  todoBlock: {
    gap: theme.spacing[1],
  },
  todoText: {
    color: theme.colors.foreground,
    fontSize: theme.fontSize.sm,
  },
  errorText: {
    color: theme.colors.statusDanger,
    fontSize: theme.fontSize.sm,
  },
  notificationText: {
    color: theme.colors.foregroundMuted,
    fontSize: theme.fontSize.sm,
  },
  compactionText: {
    color: theme.colors.foregroundMuted,
    fontSize: theme.fontSize.sm,
    textAlign: "center",
  },
  emptyText: {
    color: theme.colors.foregroundMuted,
    fontSize: theme.fontSize.sm,
  },
}));
