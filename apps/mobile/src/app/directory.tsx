import { View } from "react-native";
import { useTranslation } from "react-i18next";
import { StyleSheet } from "react-native-unistyles";
import { MenuHeader } from "@/components/headers/menu-header";
import { AccountDirectoryPanel } from "@/components/account-directory-panel";

export default function AccountDirectoryRoute() {
  const { t } = useTranslation();
  return (
    <View style={styles.container}>
      <MenuHeader title={t("onlineService.directoryTitle")} />
      <AccountDirectoryPanel />
    </View>
  );
}
const styles = StyleSheet.create((theme) => ({
  container: { flex: 1, backgroundColor: theme.colors.surface0 },
}));
