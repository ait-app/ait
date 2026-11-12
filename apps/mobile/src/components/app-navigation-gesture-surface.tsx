import type { ReactNode } from "react";
import { StyleSheet, View } from "react-native";
import { GestureDetector } from "react-native-gesture-handler";
import { isWeb } from "@/constants/platform";
import { useOpenAgentListGesture } from "@/mobile-panels/gestures";

/** Native navigators must keep their ancestors across fold and window resizes. */
export function AppNavigationGestureSurface({
  children,
  isCompact,
  chromeEnabled,
}: {
  children: ReactNode;
  isCompact: boolean;
  chromeEnabled: boolean;
}) {
  const gesture = useOpenAgentListGesture(isCompact && chromeEnabled);
  if (isWeb && !isCompact) return children;
  return (
    <GestureDetector gesture={gesture} touchAction={isWeb ? "auto" : "pan-y"}>
      <View
        collapsable={false}
        style={styles.fill}
        testID={isCompact ? "compact-shell" : undefined}
      >
        {children}
      </View>
    </GestureDetector>
  );
}

const styles = StyleSheet.create({ fill: { flex: 1 } });
