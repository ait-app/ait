import { useEffect, useState, type ReactNode } from "react";
import { ScrollView, Text, View } from "react-native";
import { ChevronDown, ChevronUp } from "lucide-react-native";
import { useTranslation } from "react-i18next";
import { StyleSheet } from "react-native-unistyles";
import { Button } from "@/components/ui/button";

export type ProviderDockItem = { id: string; label: string; content: ReactNode };

// Only disclosure identity is retained; native state and draft text remain in their owners.
const disclosures = new Map<string, string | null>();

function remember(owner: string, expanded: string | null) {
  disclosures.delete(owner);
  disclosures.set(owner, expanded);
  if (disclosures.size > 256) disclosures.delete(disclosures.keys().next().value!);
}

export function ProviderControlDock({
  owner,
  items,
  initial,
}: {
  owner: string;
  items: ProviderDockItem[];
  initial?: string;
}) {
  const { t } = useTranslation();
  const [expanded, setExpanded] = useState<string | null>(() =>
    disclosures.has(owner) ? disclosures.get(owner)! : (initial ?? null),
  );
  useEffect(() => {
    if (expanded && !items.some((item) => item.id === expanded)) {
      setExpanded(null);
      remember(owner, null);
    }
  }, [expanded, items, owner]);
  if (!items.length) return null;
  return (
    <View style={styles.dock} testID="provider-control-dock">
      {items.map((item) => (
        <View
          key={item.id}
          style={expanded === item.id ? styles.content : styles.hidden}
          testID={`provider-control-content-${item.id}`}
        >
          <ScrollView style={styles.viewport} nestedScrollEnabled>
            {item.content}
          </ScrollView>
        </View>
      ))}
      <ScrollView
        horizontal
        showsHorizontalScrollIndicator={false}
        contentContainerStyle={styles.row}
        accessibilityLabel={t("providerControl.runtimeControls")}
        testID="provider-control-row"
      >
        {items.map((item) => (
          <Button
            key={item.id}
            variant={expanded === item.id ? "secondary" : "ghost"}
            size="sm"
            leftIcon={expanded === item.id ? ChevronUp : ChevronDown}
            accessibilityLabel={item.label}
            accessibilityState={{ expanded: expanded === item.id }}
            aria-expanded={expanded === item.id}
            testID={`provider-control-toggle-${item.id}`}
            onPress={() => {
              const next = expanded === item.id ? null : item.id;
              setExpanded(next);
              remember(owner, next);
            }}
          >
            <Text numberOfLines={1} style={styles.label}>
              {item.label}
            </Text>
          </Button>
        ))}
      </ScrollView>
    </View>
  );
}

const styles = StyleSheet.create((theme) => ({
  dock: { gap: 6, minWidth: 0 },
  row: { flexDirection: "row", alignItems: "center", gap: 6 },
  content: { minWidth: 0 },
  hidden: { display: "none" },
  viewport: { maxHeight: 240 },
  label: { color: theme.colors.foreground, fontSize: 12, maxWidth: 200 },
}));
