import React, { lazy, Suspense } from "react";
import { BotLoadingSpinner } from "@/bots/loading";
import { Text, View } from "react-native";
import { Bot } from "lucide-react-native";
import { withUnistyles } from "react-native-unistyles";
import { useTranslation } from "react-i18next";
import invariant from "tiny-invariant";
import { botStyles } from "@/bots/styles";
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
        view={target.view}
        interactive={isInteractive}
        onChat={() => retargetCurrentTab({ kind: "bot" })}
        onSettings={() => retargetCurrentTab({ kind: "bot", view: "settings" })}
      />
    </Suspense>
  );
}

export const botPanelRegistration = definePanel("bot", {
  component: BotPanel,
  presentation,
  useDescriptor() {
    const { t } = useTranslation();
    const label = t("bots.title");
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
