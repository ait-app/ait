export const importSession = {
  title: "Import session",
  chooseHostTitle: "Import from host",
  searchPlaceholder: "Search sessions...",
  scope: {
    host: "Sessions on {{host}}",
    workspace: "This workspace",
  },
  filters: {
    all: "All",
    label: "Filter: {{provider}}",
    title: "Filter by provider",
  },
  status: {
    connectHost: "Connect to a host to import sessions",
    updateHost: "Update the host to import sessions.",
    noProviders: "No importable providers are enabled.",
    loading: "Loading recent sessions...",
    failedProvider: "Could not load {{provider}} sessions",
    failedImport: "Could not import selected session.",
    openImportedFailed: "The session was imported, but its project could not be opened.",
    workingDirectoryUnavailable:
      "The session’s working directory is unavailable on this host: {{path}}. Restore the directory and try importing again.",
  },
  actions: {
    refresh: "Refresh sessions",
    showAll: "Show all",
    loadMore: "Load more",
  },
  preview: {
    untitledSession: "Untitled session",
    noPrompt: "No prompt preview",
  },
  sessionPreview: {
    title: "Session preview",
    open: "Preview session history (read-only)",
    activeNotice:
      "This session has an unfinished turn. Preview is read-only and does not resume it.",
    loadOlder: "Load older messages",
    loadingOlder: "Loading older messages...",
    empty: "No displayable history in this session.",
    failed: "Could not read the session history.",
    retry: "Retry",
    you: "You",
    assistant: "Assistant",
    compacted: "Earlier context was compacted",
    toolRunning: "running",
    toolCompleted: "completed",
    toolFailed: "failed",
    toolCanceled: "canceled",
  },
  empty: {
    noRecent: "No recent sessions to import.",
    noMatches: "No sessions match your search.",
    alreadyImported: "All recent sessions are already imported.",
    noProviderSessions: "No {{provider}} sessions found.",
  },
  row: {
    importing: "Importing...",
  },
};
