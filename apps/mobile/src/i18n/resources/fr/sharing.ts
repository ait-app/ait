const sharedSharing = {
  sharedWithMeCount: "Partagé avec moi ({{count}})",
  incomingHostHint:
    "Les hôtes activés ainsi que leurs projets et espaces partagés apparaissent dans vos listes.",
  showHostInHosts: "Afficher {{host}} dans les hôtes",
  sharedByMeCount: "Partagé par moi ({{count}})",
  action: "Partager…",
  direct: "Partager avec des personnes",
  directHint: "Invitez par e-mail ou nom d’utilisateur.",
  openInvitations: "Gérer les invitations sur le Web",
  openInvitationsError: "Impossible d’ouvrir la page de partage.",
  invitation: {
    recipient: "Adresse e-mail ou nom d’utilisateur",
    generate: "Générer un lien d’invitation",
    copyLink: "Copier le lien",
    signIn: "Connectez-vous à AIT pour générer un lien d’invitation.",
    error: "Impossible de générer un lien d’invitation.",
  },
  directUnsupported: "Ce service ne prend pas en charge le partage direct.",
  manage: "Gérer",
  refresh: "Actualiser",
  stop: "Arrêter le partage",
  active: "Actif",
  loading: "Chargement des ressources partagées…",
  hostSection: "Partage",
  hostTitle: "Partager ce nœud",
  hostHint:
    "Donnez à des personnes ou à des groupes accès à tous les projets et espaces de travail de cet hôte.",
  shareTitle: "Partager {{name}}",
  resourceTitle: "Partager la ressource",
  intro:
    "Les personnes invitées et les membres des groupes peuvent ouvrir cette ressource lorsque l’hôte est en ligne.",
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
  emptyIncomingHint:
    "Les ressources partagées avec vous directement ou via un groupe apparaîtront ici.",
  emptyOutgoingTitle: "Vous ne partagez rien",
  emptyOutgoingHint:
    "Utilisez le menu d’un espace de travail ou d’un projet, ou ouvrez les paramètres de l’hôte pour partager un nœud entier.",
} as const;

export const sharing = {
  ...sharedSharing,
  title: "Partage",
  listError: "Impossible de charger les ressources partagées.",
};
