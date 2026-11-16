export const agentPanel = {
  states: {
    notFound: "Agent not found",
    failedToLoad: "Failed to load agent",
    reconnecting: "Reconnecting to host",
    updating: "Updating messages",
    timelineSyncFailed: "Couldn't refresh agent history.",
    timelineSyncRetrying: "Retrying…",
    archivingTitle: "Archiving agent...",
    archivingSubtitle: "Please wait while we archive this agent.",
  },
  unavailable: {
    selectedHost: "Selected host",
    unknownHost: "Cannot open this agent because {{serverLabel}} is not configured on this device.",
    addHost: "Add the host in Settings or open an agent on a configured server to continue.",
    preparingSession: "Preparing {{serverLabel}} session...",
    connecting: "Connecting to {{serverLabel}}...",
    showSoon: "We will show this agent in a moment.",
    showWhenOnline: "We will show this agent once the host is online.",
    reconnectingTo: "Reconnecting to {{serverLabel}}...",
    showAgainWhenReachable: "We will show this agent again as soon as the host is reachable.",
  },
  archived: {
    callout: "This agent is archived",
    unarchive: "Unarchive",
  },
};
