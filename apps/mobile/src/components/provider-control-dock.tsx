import { useEffect, useState, type ReactNode } from "react";
import { Pressable, ScrollView, Text, View, type PressableStateCallbackType } from "react-native";
import { useTranslation } from "react-i18next";
import { StyleSheet } from "react-native-unistyles";
import { composerPillStyles } from "@/composer/pill-styles";

export type ProviderDockItem = {
  id: string;
  label: string;
  content: ReactNode | ((disclosure: { expanded: boolean; collapse: () => void }) => ReactNode);
};

// Only disclosure identity is retained; native state and draft text remain in their owners.
const disclosures = new Map<string, string | null>();
const revealed = new Map<string, string>();

function remember(owner: string, expanded: string | null) {
  disclosures.delete(owner);
  disclosures.set(owner, expanded);
  if (disclosures.size > 256) disclosures.delete(disclosures.keys().next().value!);
}

export function ProviderControlDock({
  owner,
  items,
  initial,
  status,
  reveal,
}: {
  owner: string;
  items: ProviderDockItem[];
  initial?: string;
  status?: ReactNode;
  reveal?: { id: string; revision: string } | null;
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
  useEffect(() => {
    if (!reveal || revealed.get(owner) === reveal.revision) return;
    if (!items.some((item) => item.id === reveal.id)) return;
    setExpanded(reveal.id);
    remember(owner, reveal.id);
    revealed.delete(owner);
    revealed.set(owner, reveal.revision);
    if (revealed.size > 256) revealed.delete(revealed.keys().next().value!);
  }, [owner, reveal, items]);
  if (!items.length && !status) return null;
  return (
    <View style={styles.dock} testID="provider-control-dock">
      {items.map((item) => (
        <View
          key={item.id}
          style={expanded === item.id ? styles.content : styles.hidden}
          testID={`provider-control-content-${item.id}`}
        >
          <ScrollView
            style={styles.viewport}
            contentContainerStyle={styles.viewportContent}
            nestedScrollEnabled
          >
            {typeof item.content === "function"
              ? item.content({
                  expanded: expanded === item.id,
                  collapse: () => {
                    setExpanded(null);
                    remember(owner, null);
                  },
                })
              : item.content}
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
        {status}
        {items.map((item) => (
          <Pressable
            key={item.id}
            style={({ pressed, hovered }: PressableStateCallbackType & { hovered?: boolean }) => [
              composerPillStyles.body,
              (pressed || hovered || expanded === item.id) && composerPillStyles.bodyActive,
            ]}
            accessibilityRole="button"
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
            <Text
              numberOfLines={1}
              style={[
                composerPillStyles.label,
                expanded === item.id && composerPillStyles.labelActive,
                styles.label,
              ]}
            >
              {item.label}
            </Text>
          </Pressable>
        ))}
      </ScrollView>
    </View>
  );
}

const styles = StyleSheet.create(() => ({
  dock: { gap: 6, minWidth: 0 },
  row: { flexDirection: "row", alignItems: "center", gap: 6 },
  content: { minWidth: 0 },
  hidden: { display: "none" },
  // Outset focus rings need room inside the scrolling clip. The negative margin keeps
  // content aligned with the pills while the padding reserves that room on every edge.
  viewport: { maxHeight: 240, marginHorizontal: -4 },
  viewportContent: { padding: 4 },
  label: { maxWidth: 200 },
}));
