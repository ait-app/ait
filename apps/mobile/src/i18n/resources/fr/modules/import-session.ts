export const importSession = {
  chooseHostTitle: "Importer depuis l’hôte",
  title: "Session d'importation",
  searchPlaceholder: "Rechercher des sessions...",
  scope: {
    host: "Sessions sur {{host}}",
    workspace: "Cet espace de travail",
  },
  filters: {
    all: "Tous",
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
    refresh: "Sessions de rafraîchissement",
    showAll: "Tout afficher",
    loadMore: "Charger plus",
  },
  preview: {
    untitledSession: "Séance sans titre",
    noPrompt: "Aucun aperçu rapide",
  },
  empty: {
    noRecent: "Aucune session récente à importer.",
    noMatches: "Aucune session ne correspond à votre recherche.",
    alreadyImported: "Toutes les sessions récentes sont déjà importées.",
    noProviderSessions: "Aucune session{{provider}}trouvée.",
  },
  row: {
    importing: "Importation...",
  },
};
