import { Text, View } from "react-native";
import { CalendarClock } from "lucide-react-native";
import { withUnistyles } from "react-native-unistyles";
import { useTranslation } from "react-i18next";
import invariant from "tiny-invariant";
import { WorkspaceScheduleTab } from "@/components/schedules/workspace-schedule-tab";
import { usePaneContext, usePaneFocus } from "@/panels/pane-context";
import { definePanel, type PanelPresentation } from "@/panels/panel-registry";
import { useWorkspaceFields } from "@/stores/session-store-hooks";
import { useScheduleDetail } from "@/schedules/use-workspace-schedules";
import { resolveScheduleTitle } from "@/utils/schedule-format";

const ThemedCalendarClock = withUnistyles(CalendarClock);
const schedulePanelPresentation = {
  label: (t) => t("schedules.product.schedule"),
  subtitle: (t) => t("schedules.new"),
  tooltip: (t) => t("schedules.new"),
  icon: ThemedCalendarClock,
} satisfies PanelPresentation;

function SchedulePanel() {
  const { t } = useTranslation();
  const { serverId, workspaceId, target, closeCurrentTab, retargetCurrentTab, openTab } =
    usePaneContext();
  const { isInteractive } = usePaneFocus();
  invariant(target.kind === "schedule", "SchedulePanel requires schedule target");
  const workspace = useWorkspaceFields(serverId, workspaceId, (descriptor) => ({
    id: descriptor.id,
    name: descriptor.title?.trim() || descriptor.name,
    cwd: descriptor.workspaceDirectory,
  }));

  if (!workspace?.cwd) {
    return (
      <View>
        <Text>{t("panels.file.directoryMissing")}</Text>
      </View>
    );
  }

  return (
    <WorkspaceScheduleTab
      key={`${serverId}:${workspace.id}:${target.scheduleId ?? target.view ?? "new"}`}
      serverId={serverId}
      workspace={workspace}
      onClose={closeCurrentTab}
      scheduleId={target.scheduleId}
      view={target.view}
      onBrowse={() => retargetCurrentTab({ kind: "schedule", view: "list" })}
      interactive={isInteractive}
      onOpen={(scheduleId) => retargetCurrentTab({ kind: "schedule", scheduleId })}
      onCreate={() => retargetCurrentTab({ kind: "schedule" })}
      onOpenAgent={(agentId) => openTab({ kind: "agent", agentId })}
    />
  );
}

export const schedulePanelRegistration = definePanel("schedule", {
  component: SchedulePanel,
  presentation: schedulePanelPresentation,
  useDescriptor(target, context) {
    const { t } = useTranslation();
    const { data } = useScheduleDetail(context.serverId, target.scheduleId, false);
    const label = data
      ? resolveScheduleTitle(data, t)
      : t(
          target.scheduleId
            ? "schedules.product.schedule"
            : target.view === "list"
              ? "schedules.browse"
              : "schedules.new",
        );
    return {
      label,
      subtitle: label,
      tooltip: label,
      titleState: "ready",
      icon: ThemedCalendarClock,
      statusBucket: null,
    };
  },
});
