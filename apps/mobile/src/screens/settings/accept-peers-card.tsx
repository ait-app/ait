import { useCallback } from "react";
import { Text, View } from "react-native";
import { useTranslation } from "react-i18next";
import { Switch } from "@/components/ui/switch";
import { useDaemonConfig } from "@/hooks/use-daemon-config";
import { useHostRuntimeIsConnected } from "@/runtime/host-runtime";
import { settingsStyles } from "@/styles/settings";
import { toErrorMessage } from "@/utils/error-messages";
import { showAlert } from "@/utils/show-alert";

export function AcceptPeersCard({ serverId }: { serverId: string }) {
  const { t } = useTranslation();
  const isConnected = useHostRuntimeIsConnected(serverId);
  const { config, patchConfig } = useDaemonConfig(serverId);

  const handleValueChange = useCallback(
    (next: boolean) => {
      void patchConfig({
        mcp: {
          acceptPeers: next,
        },
      }).catch((error) => {
        showAlert(t("common.errors.unableToSave"), toErrorMessage(error));
      });
    },
    [patchConfig, t],
  );

  if (!isConnected) return null;

  return (
    <View style={settingsStyles.card} testID="host-page-accept-peers-card">
      <View style={settingsStyles.row}>
        <View style={settingsStyles.rowContent}>
          <Text style={settingsStyles.rowTitle}>
            {t("settings.host.orchestration.acceptPeers.title")}
          </Text>
          <Text style={settingsStyles.rowHint}>
            {t("settings.host.orchestration.acceptPeers.hint")}
          </Text>
        </View>
        {/* Off unless the daemon says `true`: an older daemon sends no key, and the setting opens this host to other hosts' agents. */}
        <Switch
          value={config?.mcp.acceptPeers === true}
          onValueChange={handleValueChange}
          accessibilityLabel={t("settings.host.orchestration.acceptPeers.accessibilityLabel")}
        />
      </View>
    </View>
  );
}
