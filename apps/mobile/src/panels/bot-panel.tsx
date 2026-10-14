import React, { lazy, Suspense } from "react";
import { BotLoadingSpinner } from "@/bots/loading";
import { Text, View } from "react-native";
import { Bot } from "lucide-react-native";
import { withUnistyles } from "react-native-unistyles";
import { useTranslation } from "react-i18next";
import invariant from "tiny-invariant";
import { botStyles } from "@/bots/styles";
import { useBotDetail } from "@/bots/queries";
import { useAppSettings } from "@/hooks/use-settings";
import { usePaneContext, usePaneFocus } from "@/panels/pane-context";
import { definePanel, type PanelPresentation } from "@/panels/panel-registry";
import { useHostRuntimeIsConnected } from "@/runtime/host-runtime";
import { useSessionStore } from "@/stores/session-store";
import { useWorkspaceFields } from "@/stores/session-store-hooks";

const WorkspaceBotTab = lazy(() =>
  import("@/bots/workspace-bot-tab").then((module) => ({ default: module.WorkspaceBotTab })),
);

const ThemedBot = withUnistyles(Bot);
const presentation = {
  label: (t) => t("bots.title"),
  subtitle: (t) => t("bots.browse"),
  tooltip: (t) => t("bots.browse"),
  icon: ThemedBot,
} satisfies PanelPresentation;

function BotPanel() {
  const { t } = useTranslation();
  const { serverId, workspaceId, target, retargetCurrentTab } = usePaneContext();
  const { isInteractive } = usePaneFocus();
  const {
    settings: { experimentalFeatures },
  } = useAppSettings();
  const connected = useHostRuntimeIsConnected(serverId);
  const supported = useSessionStore(
    (state) => state.sessions[serverId]?.serverInfo?.features?.workspaceBots === true,
  );
  const cwd = useWorkspaceFields(
    serverId,
    workspaceId,
    (workspace) => workspace.workspaceDirectory,
  );
  invariant(target.kind === "bot", "BotPanel requires bot target");
  const message = !experimentalFeatures
    ? t("bots.disabled")
    : !connected
      ? t("bots.disconnected")
      : !supported
        ? t("bots.unsupported")
        : !cwd
          ? t("panels.file.directoryMissing")
          : null;
  if (message || !cwd)
    return (
      <View style={botStyles.empty}>
        <Text style={botStyles.hint}>{message}</Text>
      </View>
    );
  return (
    <Suspense fallback={<BotLoadingSpinner />}>
      <WorkspaceBotTab
        key={`${serverId}:${workspaceId}`}
        serverId={serverId}
        workspaceId={workspaceId}
        cwd={cwd}
        botId={target.botId}
        view={target.view}
        interactive={isInteractive}
        onBrowse={() => retargetCurrentTab({ kind: "bot", view: "list" })}
        onCreate={() => retargetCurrentTab({ kind: "bot", view: "create" })}
        onOpen={(botId) => retargetCurrentTab({ kind: "bot", botId })}
        onSettings={(botId) => retargetCurrentTab({ kind: "bot", botId, view: "settings" })}
      />
    </Suspense>
  );
}

export const botPanelRegistration = definePanel("bot", {
  component: BotPanel,
  presentation,
  useDescriptor(target, context) {
    const { t } = useTranslation();
    const { data } = useBotDetail(context.serverId, target.botId, false);
    const label =
      data?.workspaceId === context.workspaceId
        ? data.name
        : t(target.view === "create" ? "bots.new" : "bots.title");
    return {
      label,
      subtitle: label,
      tooltip: label,
      titleState: "ready",
      icon: ThemedBot,
      statusBucket: null,
    };
  },
});
