import { useCallback } from "react";
import { View } from "react-native";
import { StyleSheet } from "react-native-unistyles";
import { Globe, Mail } from "lucide-react-native";
import { Button } from "@/components/ui/button";
import { GitHubIcon } from "@/components/icons/github-icon";
import { openExternalUrl } from "@/utils/open-external-url";

const renderGitHubIcon = (color: string) => <GitHubIcon color={color} size={14} />;

export function CommunityLinks() {
  const handleOpenGitHub = useCallback(() => {
    void openExternalUrl("https://github.com/ait-app/ait");
  }, []);

  const handleOpenEmail = useCallback(() => {
    void openExternalUrl("mailto:dong@necoex.com");
  }, []);

  const handleOpenHomepage = useCallback(() => {
    void openExternalUrl("https://ait-app.com");
  }, []);

  return (
    <View style={styles.row}>
      <Button
        variant="ghost"
        size="sm"
        leftIcon={renderGitHubIcon}
        onPress={handleOpenGitHub}
        testID="community-links-github-star"
      >
        Star
      </Button>
      <Button
        variant="ghost"
        size="sm"
        leftIcon={Mail}
        onPress={handleOpenEmail}
        testID="community-links-email"
      >
        Email
      </Button>
      <Button
        variant="ghost"
        size="sm"
        leftIcon={Globe}
        onPress={handleOpenHomepage}
        testID="community-links-homepage"
      >
        Homepage
      </Button>
    </View>
  );
}

const styles = StyleSheet.create(() => ({
  row: {
    flexDirection: "row",
    justifyContent: "center",
    alignItems: "center",
    gap: 0,
  },
}));
