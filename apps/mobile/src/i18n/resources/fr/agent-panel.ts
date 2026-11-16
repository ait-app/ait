export const agentPanel = {
  states: {
    notFound: "Agent introuvable",
    failedToLoad: "Échec du chargement de l'agent",
    reconnecting: "Reconnexion à l’hôte",
    updating: "Mise à jour des messages",
    timelineSyncFailed: "Impossible d’actualiser l’historique de l’agent.",
    timelineSyncRetrying: "Nouvelle tentative…",
    archivingTitle: "Archivage de l'agent...",
    archivingSubtitle: "Veuillez patienter pendant que nous archivons cet agent.",
  },
  unavailable: {
    selectedHost: "Hôte sélectionné",
    unknownHost:
      "Impossible d'ouvrir cet agent car {{serverLabel}} n'est pas configuré sur cet appareil.",
    addHost:
      "Ajoutez l'hôte dans Paramètres ou ouvrez un agent sur un serveur configuré pour continuer.",
    preparingSession: "Préparation de la session de {{serverLabel}}...",
    connecting: "Connexion à {{serverLabel}}...",
    showSoon: "Nous montrerons cet agent dans un instant.",
    showWhenOnline: "Nous afficherons cet agent une fois que l'hôte sera en ligne.",
    reconnectingTo: "Reconnexion à {{serverLabel}}...",
    showAgainWhenReachable: "Nous afficherons à nouveau cet agent dès que l'hôte sera joignable.",
  },
  archived: {
    callout: "Cet agent est archivé",
    unarchive: "Désarchiver",
  },
};
