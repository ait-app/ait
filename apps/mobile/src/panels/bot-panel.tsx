import React, { lazy, Suspense } from "react";
import { BotLoadingSpinner } from "@/bots/loading";
import { Text, View } from "react-native";
import { useTranslation } from "react-i18next";
import invariant from "tiny-invariant";
import { botStyles } from "@/bots/styles";
import { useAppSettings } from "@/hooks/use-settings";
import { usePaneContext, usePaneFocus } from "@/panels/pane-context";
import { definePanel } from "@/panels/panel-registry";
import { botPanelPresentation } from "@/panels/panel-presentations";
import { useHostRuntimeIsConnected } from "@/runtime/host-runtime";
import { useSessionStore } from "@/stores/session-store";
import { useWorkspaceFields } from "@/stores/session-store-hooks";

const WorkspaceBotTab = lazy(() =>
  import("@/bots/workspace-bot-tab").then((module) => ({ default: module.WorkspaceBotTab })),
);

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
  presentation: botPanelPresentation,
});
