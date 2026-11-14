import { Bot, CalendarClock, FileDiff, Files, GitPullRequest } from "lucide-react-native";
import { withUnistyles } from "react-native-unistyles";
import type { PanelPresentation } from "@/panels/panel-registry";

// Static presentations live apart from the panel components so that the new-tab launcher can read
// them without importing the panel registry, which imports every panel.

const ThemedBot = withUnistyles(Bot);
const ThemedCalendarClock = withUnistyles(CalendarClock);
const ThemedFileDiff = withUnistyles(FileDiff);
const ThemedFiles = withUnistyles(Files);
const ThemedGitPullRequest = withUnistyles(GitPullRequest);

export const botPanelPresentation = {
  label: (t) => t("bots.title"),
  subtitle: (t) => t("bots.title"),
  tooltip: (t) => t("bots.title"),
  icon: ThemedBot,
} satisfies PanelPresentation;

export const filesPanelPresentation = {
  label: (t) => t("panels.files.label"),
  subtitle: (t) => t("panels.files.subtitle"),
  tooltip: (t) => t("panels.files.tooltip"),
  icon: ThemedFiles,
} satisfies PanelPresentation;

export const pullRequestPanelPresentation = {
  label: (t) => t("panels.pullRequest.label"),
  subtitle: (t) => t("panels.pullRequest.subtitle"),
  tooltip: (t) => t("panels.pullRequest.label"),
  icon: ThemedGitPullRequest,
} satisfies PanelPresentation;

export const schedulePanelPresentation = {
  label: (t) => t("schedules.product.schedule"),
  subtitle: (t) => t("schedules.browse"),
  tooltip: (t) => t("schedules.browse"),
  icon: ThemedCalendarClock,
} satisfies PanelPresentation;

export const workingDiffPanelPresentation = {
  label: (t) => t("panels.diff.diffLabel"),
  subtitle: (t) => t("panels.diff.changesSubtitle"),
  tooltip: (t) => t("panels.diff.changesSubtitle"),
  icon: ThemedFileDiff,
} satisfies PanelPresentation;

export const changesTreePanelPresentation = {
  label: (t) => t("panels.diff.changesLabel"),
  subtitle: (t) => t("panels.diff.changesSubtitle"),
  tooltip: (t) => t("panels.diff.changesSubtitle"),
  icon: ThemedFileDiff,
} satisfies PanelPresentation;
