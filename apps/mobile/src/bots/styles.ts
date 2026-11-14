import { StyleSheet } from "react-native-unistyles";

export const botStyles = StyleSheet.create((theme) => ({
  root: { flex: 1, minHeight: 0, backgroundColor: theme.colors.surface0 },
  content: { padding: 20, gap: 18, maxWidth: 900, width: "100%", alignSelf: "center" },
  viewSwitch: {
    flex: 1,
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "flex-end",
    gap: theme.spacing[1],
    paddingHorizontal: theme.spacing[2],
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
  permissionBlock: { gap: 8 },
  warning: { fontSize: theme.fontSize.sm, color: theme.colors.statusWarning },
  mono: {
    fontFamily: theme.fontFamily.mono,
    fontSize: theme.fontSize.sm,
    color: theme.colors.foreground,
  },
  link: { textDecorationLine: "underline" },
  multiline: { minHeight: 130, textAlignVertical: "top" },
  empty: { padding: 24, alignItems: "center", gap: 16 },
  disabled: { opacity: theme.opacity[50] },
  checkbox: {
    width: 22,
    height: 22,
    borderRadius: theme.borderRadius.sm,
    borderWidth: 1,
    borderColor: theme.colors.border,
    alignItems: "center",
    justifyContent: "center",
  },
  checkboxChecked: { backgroundColor: theme.colors.accent, borderColor: theme.colors.accent },
}));

/** The chat shares the session page's rail, so a Bot reads like any other conversation. */
export const botChatStyles = StyleSheet.create((theme) => ({
  transcript: { flex: 1, minHeight: 0, position: "relative" },
  listContent: {
    flexGrow: 1,
    width: "100%",
    maxWidth: theme.contentMaxWidth,
    alignSelf: "center",
    paddingHorizontal: { xs: theme.spacing[3], md: theme.spacing[4] },
    paddingTop: theme.spacing[4],
    paddingBottom: theme.spacing[4],
  },
  emptyChat: { flex: 1, alignItems: "center", justifyContent: "center" },
  // Picks the conversation the transcript shows, above it and aligned with its content.
  conversationSwitch: {
    width: "100%",
    maxWidth: theme.contentMaxWidth,
    alignSelf: "center",
    paddingHorizontal: { xs: theme.spacing[3], md: theme.spacing[4] },
    paddingTop: theme.spacing[3],
  },
  turnStatus: {
    flexDirection: "row",
    alignItems: "center",
    gap: theme.spacing[2],
    paddingVertical: theme.spacing[2],
  },
  turnStatusText: { fontSize: theme.fontSize.sm, color: theme.colors.foregroundMuted },
  turnFooter: { paddingBottom: theme.spacing[4] },
  // A wake turn's source sits where a user message would, styled as a notice, not a bubble.
  wake: {
    gap: theme.spacing[2],
    padding: theme.spacing[3],
    marginBottom: theme.spacing[2],
    borderWidth: 1,
    borderColor: theme.colors.border,
    borderRadius: 8,
    backgroundColor: theme.colors.surface1,
  },
  wakeTitle: { flexDirection: "row", alignItems: "center", gap: theme.spacing[2] },
  wakeEntry: { gap: theme.spacing[1] },
  wakeEntryLine: { flexDirection: "row", alignItems: "center", gap: theme.spacing[2] },
  wakeSummary: { paddingLeft: theme.spacing[6] },
  composerRail: {
    width: "100%",
    maxWidth: theme.contentMaxWidth,
    alignSelf: "center",
    gap: theme.spacing[2],
    paddingHorizontal: theme.spacing[4],
    paddingBottom: theme.spacing[4],
  },
  notice: { flexDirection: "row", alignItems: "center", gap: theme.spacing[2] },
}));

/** Ticket rows in the chat's track panel and the ticket sheet, on the same rails as the chat. */
export const botTicketStyles = StyleSheet.create((theme) => ({
  // Title over facts, as subagent rows lay out, so a long title keeps the row's full width.
  rowText: { flexGrow: 1, flexShrink: 1, flexBasis: "auto", minWidth: 0 },
  rowTitle: { fontSize: theme.fontSize.base, color: theme.colors.foreground },
  rowFacts: { fontSize: theme.fontSize.sm, color: theme.colors.foregroundMuted },
  rowWaiting: { fontSize: theme.fontSize.sm, color: theme.colors.statusWarning },
  heading: {
    fontSize: theme.fontSize.sm,
    fontWeight: "600",
    color: theme.colors.foregroundMuted,
  },
  body: { gap: theme.spacing[4] },
  section: { gap: theme.spacing[2] },
  statusLine: { flexDirection: "row", alignItems: "center", gap: theme.spacing[2] },
  report: { gap: theme.spacing[1] },
  footer: { gap: theme.spacing[2] },
  actions: {
    flexDirection: "row",
    flexWrap: "wrap",
    justifyContent: "flex-end",
    gap: theme.spacing[2],
  },
  wakeTicket: { paddingLeft: theme.spacing[6] },
}));
