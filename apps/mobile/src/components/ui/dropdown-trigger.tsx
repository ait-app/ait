import type { ReactElement, ReactNode } from "react";
import { View } from "react-native";
import { StyleSheet, withUnistyles } from "react-native-unistyles";
import { ChevronDown } from "lucide-react-native";
import { ICON_SIZE } from "@/styles/theme";
import { DropdownMenuTrigger, type DropdownMenuTriggerProps } from "@/components/ui/dropdown-menu";
import { mutedIconColorMapping } from "@/components/ui/icon-color";

const ThemedChevronDown = withUnistyles(ChevronDown);

interface DropdownTriggerProps extends Omit<DropdownMenuTriggerProps, "children"> {
  children?: ReactNode;
}

export function DropdownTrigger({ children, ...props }: DropdownTriggerProps): ReactElement {
  return (
    <DropdownMenuTrigger {...props}>
      <View style={styles.row}>
        {children}
        <View style={styles.chevronContainer}>
          <ThemedChevronDown size={ICON_SIZE.sm} uniProps={mutedIconColorMapping} />
        </View>
      </View>
    </DropdownMenuTrigger>
  );
}

const styles = StyleSheet.create((theme) => ({
  row: {
    flexDirection: "row",
    alignItems: "center",
    gap: theme.spacing[1],
  },
  chevronContainer: {
    transform: [{ translateY: 1 }],
  },
}));
