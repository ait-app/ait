import { useTranslation } from "react-i18next";
import { Pressable } from "react-native";
import { StyleSheet, useUnistyles } from "react-native-unistyles";
import { ArrowLeft } from "lucide-react-native";
import { ScreenHeader } from "./screen-header";
import { ScreenTitle } from "./screen-title";

interface BackHeaderProps {
  title?: string;
  onBack: () => void;
}

export function BackHeader({ title, onBack }: BackHeaderProps) {
  const { theme } = useUnistyles();
  const { t } = useTranslation();

  return (
    <ScreenHeader
      left={
        <>
          <Pressable
            onPress={onBack}
            style={styles.backButton}
            accessibilityRole="button"
            accessibilityLabel={t("common.actions.back")}
          >
            <ArrowLeft size={theme.iconSize.lg} color={theme.colors.foregroundMuted} />
          </Pressable>
          {title && <ScreenTitle>{title}</ScreenTitle>}
        </>
      }
      leftStyle={styles.left}
    />
  );
}

const styles = StyleSheet.create((theme) => ({
  left: {
    gap: theme.spacing[2],
  },
  backButton: {
    padding: {
      xs: theme.spacing[3],
      md: theme.spacing[2],
    },
    borderRadius: theme.borderRadius.lg,
  },
}));
