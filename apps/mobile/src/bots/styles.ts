import { StyleSheet } from "react-native-unistyles";

export const botStyles = StyleSheet.create((theme) => ({
  root: { flex: 1, minHeight: 0, backgroundColor: theme.colors.surface0 },
  content: { padding: 20, gap: 18, maxWidth: 900, width: "100%", alignSelf: "center" },
  toolbar: {
    flexDirection: "row",
    flexWrap: "wrap",
    alignItems: "center",
    gap: 8,
    padding: 12,
    borderBottomWidth: 1,
    borderBottomColor: theme.colors.border,
  },
  row: { flexDirection: "row", flexWrap: "wrap", alignItems: "center", gap: 12 },
  grow: { flex: 1, minWidth: 0 },
  title: { fontSize: theme.fontSize.lg, fontWeight: "600", color: theme.colors.foreground },
  text: { fontSize: theme.fontSize.base, color: theme.colors.foreground },
  hint: { fontSize: theme.fontSize.sm, color: theme.colors.foregroundMuted },
  error: { fontSize: theme.fontSize.sm, color: theme.colors.destructive },
  card: {
    padding: 16,
    gap: 12,
    borderWidth: 1,
    borderColor: theme.colors.border,
    borderRadius: 8,
    backgroundColor: theme.colors.surface1,
  },
  multiline: { minHeight: 130, textAlignVertical: "top" },
  composer: { padding: 12, gap: 8, borderTopWidth: 1, borderTopColor: theme.colors.border },
  messageInput: { minHeight: 70, maxHeight: 180, textAlignVertical: "top" },
  empty: { padding: 24, alignItems: "center", gap: 16 },
}));
