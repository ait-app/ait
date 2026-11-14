import { useMemo, type ReactNode } from "react";
import { Text, View } from "react-native";
import { useSafeAreaInsets } from "react-native-safe-area-context";
import { StyleSheet } from "react-native-unistyles";
import { SidebarMenuToggle } from "@/components/headers/menu-header";
import { IconButtonChromeProvider } from "@/components/ui/icon-button-chrome";
import { HEADER_INNER_HEIGHT_MOBILE, HEADER_TOP_PADDING_MOBILE } from "@/constants/layout";

interface CompactHeaderProps {
  title?: string;
  titleTestID?: string;
  /** A line under the title. */
  subtitle?: ReactNode;
  /** Show a placeholder instead of the title while it loads. */
  loading?: boolean;
  /** Icon actions inherit the island chrome through the shared button primitives. */
  actions?: ReactNode;
}

/**
 * The phone header: the sidebar menu button and the screen's actions sit in islands, with the title
 * left-aligned between them. Neither the header nor its islands draw a background, so it reads the
 * same in flow and floating over content (see `FloatingHeaderLayout`, whose fade gives the icons
 * contrast). Its height matches the old flat header, so anything
 * offset by the header height still lines up.
 */
export function CompactHeader({
  title,
  titleTestID,
  subtitle,
  loading = false,
  actions,
}: CompactHeaderProps) {
  const insets = useSafeAreaInsets();
  const rootStyle = useMemo(
    () => [styles.root, { paddingTop: insets.top + HEADER_TOP_PADDING_MOBILE }],
    [insets.top],
  );

  return (
    <IconButtonChromeProvider value="header-island">
      <View style={rootStyle}>
        <View style={styles.row}>
          <View style={styles.island}>
            <SidebarMenuToggle />
          </View>
          <View style={styles.titleGroup}>
            <CompactHeaderTitle
              loading={loading}
              title={title}
              titleTestID={titleTestID}
              subtitle={subtitle}
            />
          </View>
          {actions ? <View style={styles.island}>{actions}</View> : null}
        </View>
      </View>
    </IconButtonChromeProvider>
  );
}

function CompactHeaderTitle({
  loading,
  title,
  titleTestID,
  subtitle,
}: Pick<CompactHeaderProps, "title" | "titleTestID" | "subtitle"> & { loading: boolean }) {
  if (loading) {
    return <View style={styles.titleSkeleton} />;
  }
  return (
    <>
      {title ? (
        <Text
          style={subtitle ? styles.title : styles.singleLineTitle}
          numberOfLines={1}
          testID={titleTestID}
        >
          {title}
        </Text>
      ) : null}
      {subtitle}
    </>
  );
}

const styles = StyleSheet.create((theme) => ({
  root: {
    paddingHorizontal: theme.spacing[3],
  },
  row: {
    height: HEADER_INNER_HEIGHT_MOBILE,
    flexDirection: "row",
    alignItems: "center",
    gap: theme.spacing[2],
  },
  titleGroup: {
    flex: 1,
    minWidth: 0,
    // Both title lines receive the remaining width; text alignment does not size the slot.
    alignItems: "stretch",
  },
  island: {
    flexDirection: "row",
    alignItems: "center",
  },
  title: {
    fontSize: theme.fontSize.base,
    fontWeight: theme.fontWeight.medium,
    color: theme.colors.foreground,
    textAlign: "left",
  },
  // A title without a subtitle has the header to itself, so it reads one step larger.
  singleLineTitle: {
    fontSize: theme.fontSize.lg,
    fontWeight: theme.fontWeight.medium,
    color: theme.colors.foreground,
    textAlign: "left",
  },
  titleSkeleton: {
    width: 140,
    maxWidth: "100%",
    height: 22,
    borderRadius: theme.borderRadius.full,
    backgroundColor: theme.colors.surface3,
    opacity: 0.25,
  },
}));
