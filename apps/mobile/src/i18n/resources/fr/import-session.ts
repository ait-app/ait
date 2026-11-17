export const importSession = {
  chooseHostTitle: "Importer depuis l’hôte",
  title: "Importer une session",
  searchPlaceholder: "Rechercher des sessions...",
  scope: {
    host: "Sessions sur {{host}}",
    workspace: "Cet espace de travail",
  },
  filters: {
    all: "Tous",
    label: "Filtre : {{provider}}",
    title: "Filtrer par fournisseur",
  },
  status: {
    connectHost: "Connectez-vous à un hôte pour importer des sessions",
    updateHost: "Mettez à jour l'hôte pour importer des sessions.",
    noProviders: "Aucun fournisseur importable n'est activé.",
    loading: "Chargement des sessions récentes...",
    failedProvider: "Impossible de charger les sessions {{provider}}",
    failedImport: "Impossible d'importer la session sélectionnée.",
    workingDirectoryUnavailable:
      "Le dossier de travail de la session est indisponible sur cet hôte : {{path}}. Restaurez-le puis réessayez l’importation.",
  },
  actions: {
    refresh: "Actualiser les sessions",
    showAll: "Tout afficher",
    loadMore: "Charger plus",
  },
  preview: {
    untitledSession: "Session sans titre",
    noPrompt: "Aucun aperçu du prompt",
  },
  sessionPreview: {
    title: "Aperçu de la session",
    open: "Aperçu de l'historique de la session (lecture seule)",
    activeNotice:
      "Cette session a un tour inachevé. L'aperçu est en lecture seule et ne la reprend pas.",
    loadOlder: "Charger les messages plus anciens",
    loadingOlder: "Chargement des messages plus anciens...",
    empty: "Aucun historique affichable dans cette session.",
    failed: "Impossible de lire l'historique de la session.",
    retry: "Réessayer",
    you: "Vous",
    assistant: "Assistant",
    compacted: "Le contexte précédent a été compacté",
    toolRunning: "en cours",
    toolCompleted: "terminé",
    toolFailed: "échoué",
    toolCanceled: "annulé",
  },
  empty: {
    noRecent: "Aucune session récente à importer.",
    noMatches: "Aucune session ne correspond à votre recherche.",
    alreadyImported: "Toutes les sessions récentes sont déjà importées.",
    noProviderSessions: "Aucune session {{provider}} trouvée.",
  },
  row: {
    importing: "Importation...",
  },
};
