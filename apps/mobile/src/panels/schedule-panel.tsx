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
  subtitle: (t) => t("schedules.browse"),
  tooltip: (t) => t("schedules.browse"),
  icon: ThemedCalendarClock,
} satisfies PanelPresentation;

function SchedulePanel() {
  const { t } = useTranslation();
  const { serverId, workspaceId, target, retargetCurrentTab, openTab } = usePaneContext();
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
      key={`${serverId}:${workspace.id}`}
      serverId={serverId}
      workspace={workspace}
      scheduleId={target.scheduleId}
      view={target.view}
      filter={target.filter}
      onBrowse={(filter = target.filter ?? "upcoming") =>
        retargetCurrentTab({
          kind: "schedule",
          view: "list",
          ...(filter !== "upcoming" ? { filter } : {}),
        })
      }
      interactive={isInteractive}
      onOpen={(scheduleId) =>
        retargetCurrentTab({
          kind: "schedule",
          scheduleId,
          ...(target.filter ? { filter: target.filter } : {}),
        })
      }
      onCreate={() =>
        retargetCurrentTab({
          kind: "schedule",
          view: "create",
          ...(target.filter ? { filter: target.filter } : {}),
        })
      }
      onEditTask={(scheduleId) =>
        retargetCurrentTab({
          kind: "schedule",
          scheduleId,
          view: "edit",
          ...(target.filter ? { filter: target.filter } : {}),
        })
      }
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
            : target.view === "create"
              ? "schedules.new"
              : "schedules.browse",
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
