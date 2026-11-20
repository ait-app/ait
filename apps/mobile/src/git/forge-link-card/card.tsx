import { Fragment, useCallback, useMemo, useState, type ReactElement, type ReactNode } from "react";
import { Image, Text, View } from "react-native";
import { useTranslation } from "react-i18next";
import { StyleSheet, withUnistyles } from "react-native-unistyles";
import { CircleCheck, CircleDot } from "lucide-react-native";
import { DiffStat } from "@/components/diff-stat";
import { mutedIconColorMapping, statusSuccessIconColorMapping } from "@/components/ui/icon-color";
import { SkeletonPulse, useSkeletonPulse } from "@/components/ui/skeleton-pulse";
import type { ForgeSearchKind } from "@ait/protocol/messages";
import type { ForgeRef } from "@/git/forge-refs";
import { deriveAvatarColor } from "@/git/pull-request-panel/data";
import { PullRequestStateIcon } from "@/git/pull-request-state-icon";
import { useForgeSearchQuery } from "@/git/use-forge-search-query";
import { useHostRuntimeClient, useHostRuntimeIsConnected } from "@/runtime/host-runtime";
import { useSessionStore } from "@/stores/session-store";
import { displayInitial } from "@/utils/display-initial";
import { formatTimeAgo } from "@/utils/time";
import {
  describeForgeLinkItem,
  selectForgeLinkItem,
  type ForgeLinkDetails,
  type ForgeLinkState,
} from "./model";

export const FORGE_LINK_CARD_WIDTH = 340;
const AVATAR_SIZE = 16;
const STATE_ICON_SIZE = 14;

// Stable arrays: the search hook builds its query key from them on every render.
const FORGE_LINK_KINDS: Record<ForgeRef["kind"], ForgeSearchKind[]> = {
  change_request: ["change_request"],
  issue: ["issue"],
};

const ThemedCircleDot = withUnistyles(CircleDot);
const ThemedCircleCheck = withUnistyles(CircleCheck);

interface ForgeLinkCardProps {
  serverId: string;
  /** The workspace checkout the link was shown in; forge search runs against its repository. */
  cwd: string;
  forgeRef: ForgeRef;
}

/**
 * What a link to an issue or change request points at: title, repository, author, size, state.
 *
 * It is the composer's forge search, asked for one number in every state: most links in a
 * conversation point at work that has since merged or closed, which the open-only search the
 * composer's picker uses would not find.
 */
export function ForgeLinkCard({ serverId, cwd, forgeRef }: ForgeLinkCardProps): ReactElement {
  const client = useHostRuntimeClient(serverId);
  const isConnected = useHostRuntimeIsConnected(serverId);
  const supportsForgeSearch = useSessionStore(
    (state) => state.sessions[serverId]?.serverInfo?.features?.forgeSearch === true,
  );
  const query = useForgeSearchQuery({
    client,
    serverId,
    cwd,
    query: String(forgeRef.number),
    kinds: FORGE_LINK_KINDS[forgeRef.kind],
    state: "all",
    enabled: isConnected,
    supportsForgeSearch,
  });
  const item = query.data ? selectForgeLinkItem(query.data.items, forgeRef) : null;
  const details = useMemo(() => (item ? describeForgeLinkItem(item) : null), [item]);

  if (details) return <ForgeLinkDetailsView details={details} />;
  if (query.isFetching) return <ForgeLinkCardSkeleton />;
  return <ForgeLinkCardUnavailable />;
}

function ForgeLinkDetailsView({ details }: { details: ForgeLinkDetails }): ReactElement {
  const { t } = useTranslation();
  const meta: { key: string; node: ReactNode }[] = [];
  if (details.author) {
    meta.push({
      key: "author",
      node: (
        <View style={styles.author}>
          <AuthorAvatar login={details.author} avatarUrl={details.authorAvatarUrl} />
          <Text style={styles.metaText} numberOfLines={1} testID="forge-link-card-author">
            {details.author}
          </Text>
        </View>
      ),
    });
  }
  if (details.updatedAt) {
    meta.push({
      key: "updated",
      node: <Text style={styles.metaText}>{formatTimeAgo(details.updatedAt)}</Text>,
    });
  }
  if (details.diffStat) {
    meta.push({
      key: "diff",
      node: (
        <DiffStat
          additions={details.diffStat.additions}
          deletions={details.diffStat.deletions}
          testID="forge-link-card-diff"
        />
      ),
    });
    if (details.diffStat.changedFiles !== null) {
      meta.push({
        key: "files",
        node: (
          <Text style={styles.metaText}>
            {t("workspace.git.linkCard.files", { count: details.diffStat.changedFiles })}
          </Text>
        ),
      });
    }
  }

  return (
    <View style={styles.card} testID="forge-link-card">
      <View style={styles.heading}>
        <Text style={styles.title} numberOfLines={3} testID="forge-link-card-title">
          {details.title}
        </Text>
        <Text style={styles.reference} numberOfLines={1} testID="forge-link-card-reference">
          {details.reference}
        </Text>
      </View>
      {meta.length > 0 ? (
        <View style={styles.metaRow}>
          {meta.map((entry, index) => (
            <Fragment key={entry.key}>
              {index > 0 ? <Text style={styles.metaSeparator}>·</Text> : null}
              {entry.node}
            </Fragment>
          ))}
        </View>
      ) : null}
      <StatePill kind={details.kind} state={details.state} />
    </View>
  );
}

function AuthorAvatar({
  login,
  avatarUrl,
}: {
  login: string;
  avatarUrl: string | null;
}): ReactElement {
  // An avatar that fails to load (a private host, an app account) falls back to the initial.
  const [failed, setFailed] = useState(false);
  const markFailed = useCallback(() => setFailed(true), []);
  const frameStyle = useMemo(
    () => [styles.avatar, { backgroundColor: deriveAvatarColor(login) }],
    [login],
  );
  const source = useMemo(() => (avatarUrl ? { uri: avatarUrl } : null), [avatarUrl]);
  return (
    <View style={frameStyle}>
      {source && !failed ? (
        <Image source={source} style={styles.avatarImage} onError={markFailed} />
      ) : (
        <Text style={styles.avatarText}>{displayInitial(login)}</Text>
      )}
    </View>
  );
}

const STATE_LABEL_KEYS: Record<ForgeLinkState, string> = {
  open: "workspace.git.pr.states.open",
  merged: "workspace.git.pr.states.merged",
  closed: "workspace.git.pr.states.closed",
};

function StatePill({
  kind,
  state,
}: {
  kind: ForgeLinkDetails["kind"];
  state: ForgeLinkState;
}): ReactElement {
  const { t } = useTranslation();
  // A closed change request was turned down; a closed issue is most often simply done.
  const tone = kind === "issue" && state === "closed" ? "neutral" : state;
  return (
    <View style={styles.pill} testID={`forge-link-card-state-${state}`}>
      {/* A layer of its own: on web a theme color is a CSS variable, so the pill cannot mix a
        translucent fill out of one and fades a solid layer instead. */}
      <View style={[styles.pillTint, PILL_TINT_STYLES[tone]]} pointerEvents="none" />
      {kind === "issue" ? (
        <IssueStateIcon open={state === "open"} />
      ) : (
        <PullRequestStateIcon state={state} size={STATE_ICON_SIZE} />
      )}
      <Text style={[styles.pillText, PILL_TEXT_STYLES[tone]]}>{t(STATE_LABEL_KEYS[state])}</Text>
    </View>
  );
}

function IssueStateIcon({ open }: { open: boolean }): ReactElement {
  return open ? (
    <ThemedCircleDot size={STATE_ICON_SIZE} uniProps={statusSuccessIconColorMapping} />
  ) : (
    <ThemedCircleCheck size={STATE_ICON_SIZE} uniProps={mutedIconColorMapping} />
  );
}

function ForgeLinkCardSkeleton(): ReactElement {
  const pulse = useSkeletonPulse();
  return (
    <View style={styles.card} testID="forge-link-card-loading">
      <SkeletonPulse pulse={pulse} style={styles.skeletonTitle} />
      <SkeletonPulse pulse={pulse} style={styles.skeletonLine} />
      <SkeletonPulse pulse={pulse} style={styles.skeletonPill} />
    </View>
  );
}

function ForgeLinkCardUnavailable(): ReactElement {
  const { t } = useTranslation();
  return (
    <View style={styles.card} testID="forge-link-card-unavailable">
      <Text style={styles.metaText}>{t("workspace.git.linkCard.unavailable")}</Text>
    </View>
  );
}

const styles = StyleSheet.create((theme) => ({
  card: {
    width: FORGE_LINK_CARD_WIDTH,
    padding: theme.spacing[4],
    gap: theme.spacing[3],
    alignItems: "flex-start",
  },
  heading: {
    alignSelf: "stretch",
    gap: theme.spacing[1.5],
  },
  title: {
    color: theme.colors.foreground,
    fontSize: theme.fontSize.base,
    fontWeight: theme.fontWeight.medium,
    lineHeight: 20,
  },
  reference: {
    color: theme.colors.foregroundMuted,
    fontSize: theme.fontSize.sm,
    fontWeight: theme.fontWeight.normal,
  },
  metaRow: {
    alignSelf: "stretch",
    flexDirection: "row",
    flexWrap: "wrap",
    alignItems: "center",
    columnGap: theme.spacing[1.5],
  },
  metaText: {
    color: theme.colors.foregroundMuted,
    fontSize: theme.fontSize.sm,
    fontWeight: theme.fontWeight.normal,
    flexShrink: 1,
  },
  metaSeparator: {
    color: theme.colors.foregroundExtraMuted,
    fontSize: theme.fontSize.sm,
  },
  author: {
    flexDirection: "row",
    alignItems: "center",
    gap: theme.spacing[1.5],
    flexShrink: 1,
    minWidth: 0,
  },
  avatar: {
    width: AVATAR_SIZE,
    height: AVATAR_SIZE,
    borderRadius: AVATAR_SIZE / 2,
    alignItems: "center",
    justifyContent: "center",
    overflow: "hidden",
    flexShrink: 0,
  },
  avatarImage: {
    width: AVATAR_SIZE,
    height: AVATAR_SIZE,
  },
  avatarText: {
    color: "#fff",
    fontSize: 9,
    fontWeight: theme.fontWeight.medium,
  },
  pill: {
    flexDirection: "row",
    alignItems: "center",
    gap: theme.spacing[1.5],
    paddingHorizontal: theme.spacing[2],
    paddingVertical: theme.spacing[1],
    borderRadius: theme.borderRadius.md,
    overflow: "hidden",
  },
  pillTint: {
    position: "absolute",
    top: 0,
    right: 0,
    bottom: 0,
    left: 0,
    opacity: 0.16,
  },
  pillTintOpen: {
    backgroundColor: theme.colors.statusSuccess,
  },
  pillTintMerged: {
    backgroundColor: theme.colors.statusMerged,
  },
  pillTintClosed: {
    backgroundColor: theme.colors.statusDanger,
  },
  pillTintNeutral: {
    backgroundColor: theme.colors.foregroundMuted,
  },
  pillText: {
    fontSize: theme.fontSize.sm,
    fontWeight: theme.fontWeight.medium,
  },
  pillTextOpen: {
    color: theme.colors.statusSuccess,
  },
  pillTextMerged: {
    color: theme.colors.statusMerged,
  },
  pillTextClosed: {
    color: theme.colors.statusDanger,
  },
  pillTextNeutral: {
    color: theme.colors.foregroundMuted,
  },
  skeletonTitle: {
    alignSelf: "stretch",
    height: 16,
    borderRadius: theme.borderRadius.sm,
    backgroundColor: theme.colors.surface3,
  },
  skeletonLine: {
    width: "55%",
    height: 12,
    borderRadius: theme.borderRadius.sm,
    backgroundColor: theme.colors.surface3,
  },
  skeletonPill: {
    width: 64,
    height: 22,
    borderRadius: theme.borderRadius.md,
    backgroundColor: theme.colors.surface3,
  },
}));

const PILL_TINT_STYLES = {
  open: styles.pillTintOpen,
  merged: styles.pillTintMerged,
  closed: styles.pillTintClosed,
  neutral: styles.pillTintNeutral,
} as const;

const PILL_TEXT_STYLES = {
  open: styles.pillTextOpen,
  merged: styles.pillTextMerged,
  closed: styles.pillTextClosed,
  neutral: styles.pillTextNeutral,
} as const;
