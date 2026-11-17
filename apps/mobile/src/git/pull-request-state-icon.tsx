import { GitMerge, GitPullRequest, GitPullRequestClosed } from "lucide-react-native";
import { withUnistyles } from "react-native-unistyles";
import {
  statusDangerIconColorMapping,
  statusSuccessIconColorMapping,
} from "@/components/ui/icon-color";
import type { Theme } from "@/styles/theme";
import type { PrHint } from "@/git/pr-hint";

const ThemedGitPullRequest = withUnistyles(GitPullRequest);
const ThemedGitMerge = withUnistyles(GitMerge);
const ThemedGitPullRequestClosed = withUnistyles(GitPullRequestClosed);

const mergedMapping = (theme: Theme) => ({ color: theme.colors.statusMerged });

const PRESENTATION = {
  open: { Icon: ThemedGitPullRequest, color: statusSuccessIconColorMapping },
  merged: { Icon: ThemedGitMerge, color: mergedMapping },
  closed: { Icon: ThemedGitPullRequestClosed, color: statusDangerIconColorMapping },
} as const;

/** The canonical state glyph and color shared by every pull-request badge. */
export function PullRequestStateIcon({
  state,
  size,
  strokeWidth,
}: {
  state: PrHint["state"];
  size: number;
  strokeWidth?: number;
}) {
  const { Icon, color } = PRESENTATION[state];
  return <Icon size={size} strokeWidth={strokeWidth} uniProps={color} />;
}
