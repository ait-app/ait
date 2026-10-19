import { Text, View } from "react-native";
import { StyleSheet } from "react-native-unistyles";
import { RotateCw } from "lucide-react-native";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { CopyErrorLogButton } from "@/components/copy-error-log-button";

export function PullRequestPaneError({
  onRetry,
  message,
}: {
  onRetry: () => void;
  message?: string;
}) {
  const { t } = useTranslation();
  return (
    <View style={styles.root} testID="pr-pane-error">
      <Text style={styles.message}>{message ?? t("workspace.git.diff.failedRefresh")}</Text>
      <CopyErrorLogButton
        error={message ?? t("workspace.git.diff.failedRefresh")}
        source="pull request"
      />
      <Button variant="ghost" size="xs" leftIcon={RotateCw} onPress={onRetry}>
        {t("common.actions.retry")}
      </Button>
    </View>
  );
}

const styles = StyleSheet.create((theme) => ({
  root: {
    flex: 1,
    alignItems: "center",
    justifyContent: "center",
    gap: theme.spacing[3],
    paddingHorizontal: theme.spacing[4],
    backgroundColor: theme.colors.surfaceSidebar,
  },
  message: {
    fontSize: theme.fontSize.sm,
    color: theme.colors.foregroundMuted,
    textAlign: "center",
  },
}));
