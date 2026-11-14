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
  },
  status: {
    connectHost: "Connect to a host to import sessions",
    updateHost: "Update the host to import sessions.",
    noProviders: "No importable providers are enabled.",
    loading: "Loading recent sessions...",
    failedProvider: "Could not load {{provider}} sessions",
    failedImport: "Could not import selected session.",
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
