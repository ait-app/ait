import React, { type ReactNode } from "react";
import { Clock3 } from "lucide-react-native";
import { Text, View } from "react-native";
import { StyleSheet, withUnistyles } from "react-native-unistyles";
import { useTranslation } from "react-i18next";
import { GitHubIcon } from "@/components/icons/github-icon";
import { definePaneContribution, type ClientForgeViewModule } from "@/git/client-forge-module";
import { foregroundMutedColorMapping } from "@/git/pull-request-panel/section-kit";
import { GithubMergeFactsSchema, type GithubMergeFacts } from "./github";

function renderGithubHeaderMeta(facts: GithubMergeFacts): ReactNode {
  if (!facts.isInMergeQueue) {
    return null;
  }
  return <GithubMergeQueueBadge />;
}

const ThemedClock3 = withUnistyles(Clock3);

function GithubMergeQueueBadge() {
  const { t } = useTranslation();
  return (
    <View style={styles.mergeQueueBadge} testID="pr-pane-merge-queue">
      <ThemedClock3 size={11} uniProps={foregroundMutedColorMapping} />
      <Text style={styles.mergeQueueText}>{t("common.status.queued")}</Text>
    </View>
  );
}

export const githubForgeView = {
  id: "github",
  icon: GitHubIcon,
  brandColor: null,
  paneContributions: [
    definePaneContribution(GithubMergeFactsSchema, {
      renderHeaderMeta: renderGithubHeaderMeta,
    }),
  ],
} satisfies ClientForgeViewModule;

const styles = StyleSheet.create((theme) => ({
  mergeQueueBadge: {
    flexDirection: "row",
    alignItems: "center",
    gap: theme.spacing[1],
  },
  mergeQueueText: {
    fontSize: theme.fontSize.sm,
    color: theme.colors.foregroundMuted,
  },
}));
