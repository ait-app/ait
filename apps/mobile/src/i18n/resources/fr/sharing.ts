const sharedSharing = {
  sharedWithMeCount: "Partagé avec moi ({{count}})",
  incomingHostHint:
    "Les hôtes activés ainsi que leurs projets et espaces partagés apparaissent dans vos listes.",
  showHostInHosts: "Afficher {{host}} dans les hôtes",
  sharedByMe: "Partagé par moi",
  sharedByMeCount: "Partagé par moi ({{count}})",
  section: "Partage",
  action: "Partager avec des groupes",
  manage: "Gérer",
  refresh: "Actualiser",
  stop: "Arrêter le partage",
  active: "Actif",
  loading: "Chargement des ressources partagées…",
  offline: "Cette ressource partagée est hors ligne.",
  emptyResources: "Cette ressource partagée ne contient aucun espace de travail.",
  hostSection: "Partage",
  hostTitle: "Partager ce nœud",
  hostHint: "Donnez à un groupe l’accès à tous les projets et espaces de travail de cet hôte.",
  shareTitle: "Partager {{name}}",
  resourceTitle: "Partager la ressource",
  intro:
    "Les membres du groupe peuvent ouvrir cette ressource dans AIT tant que le propriétaire est en ligne.",
  projectHostHint: "Partagez ce projet et ses espaces de travail sur l’hôte sélectionné.",
  hostUnavailable: "Cet hôte est indisponible. Connectez-vous-y pour gérer le partage.",
  scope: { node: "Nœud", project: "Projet", workspace: "Espace de travail" },
  scopeHint: {
    node: "Tout sur ce nœud",
    project: "Ce projet et ses espaces",
    workspace: "Cet espace uniquement",
  },
  status: {
    shared: "Partagé",
    rejected: "Non partagé · {{code}}",
    waiting: "En attente du service",
    groupAdmin: "Vous gérez ce groupe",
    groupMember: "Membre du groupe",
  },
  noGroupsTitle: "Aucun groupe disponible",
  noGroupsHint:
    "Connectez ce nœud au service en ligne dans Paramètres de l’hôte → Connexions, puis vérifiez que le propriétaire appartient à un groupe.",
  nodeSettingsOnly:
    "Ces paramètres nécessitent le partage complet du nœud. Ce partage donne uniquement accès à son projet ou à son espace de travail.",
  ownerOnly:
    "Seul le propriétaire du nœud peut gérer le partage. Les ressources qui vous sont partagées ne peuvent pas l’être à nouveau.",
  unsupported:
    "Ce nœud ne prend pas en charge la gestion du partage. Mettez-le à jour et réessayez.",
  loadError: "Impossible de charger les paramètres de partage.",
  updateError: "Impossible de mettre à jour le partage.",
  stopError: "Impossible d’arrêter le partage.",
  emptyIncomingTitle: "Rien ne vous a encore été partagé",
  emptyIncomingHint: "Les ressources partagées par les membres de vos groupes apparaîtront ici.",
  emptyOutgoingTitle: "Vous ne partagez rien",
  emptyOutgoingHint:
    "Utilisez le menu d’un espace de travail ou d’un projet, ou ouvrez les paramètres de l’hôte pour partager un nœud entier.",
} as const;

export const sharing = {
  ...sharedSharing,
  title: "Partage",
  offline: "Cette ressource partagée est hors ligne.",
  emptyResources: "Cette ressource partagée ne contient aucun espace de travail.",
  listError: "Impossible de charger les ressources partagées.",
};
