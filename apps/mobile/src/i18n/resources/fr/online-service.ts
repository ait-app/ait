export const onlineService = {
  title: "Service en ligne",
  connectionDescription: "Connectez-vous et reliez vos hôtes en ligne.",
  loginDescription: "Connectez-vous pour accéder aux ordinateurs reliés à votre service en ligne.",
  accountTitle: "Compte",
  accountInfo: "Informations du compte",
  accountInfoHint:
    "Les informations du compte sont actualisées toutes les 15 secondes tant que cette page est visible.",
  loginName: "Nom de connexion",
  accountExpiry: "Expiration du compte",
  neverExpires: "N’expire jamais",
  accountInfoFailed:
    "Impossible de charger les informations du compte. Utilisez Actualiser pour réessayer.",
  signIn: "Se connecter",
  unifiedLogin: "Se connecter / S’inscrire",
  loginPortal: "Portail de connexion unifié",
  loginPortalHint:
    "Ouvrez la page de connexion unifiée dans le navigateur par défaut de votre système.",
  openInBrowser: "Ouvrir dans le navigateur",
  loginPortalOpenFailed: "Impossible d’ouvrir la page de connexion unifiée. Réessayez.",
  unifiedHint:
    "Poursuivez dans votre navigateur avec votre adresse e-mail, Google ou WeChat. Vous pouvez aussi y vérifier votre adresse ou réinitialiser votre mot de passe.",
  browserWaiting: "En attente de la connexion dans le navigateur…",
  expiresAt: "L’accès au compte expire le {{date}}",
  signOut: "Se déconnecter",
  deleteAccount: "Supprimer le compte",
  deleteAccountTitle: "Supprimer votre compte ?",
  deleteAccountMessage:
    "Votre compte sera supprimé et vous serez déconnecté. Les hôtes reliés perdront l’accès au service en ligne. Cette action est irréversible.",
  deleteAccountInBrowser:
    "Terminez la suppression de votre compte dans le navigateur. Vous serez déconnecté ici après confirmation.",
  signOutHint:
    "La déconnexion libère le bail du daemon associé à ce client. Les autres hôtes restent synchronisés jusqu’à leur arrêt individuel.",
  refresh: "Actualiser",
  serviceSettings: "Paramètres du service",
  hideServiceSettings: "Masquer les paramètres du service",
  serviceUrl: "URL du service",
  serviceUrlHint: "URL du service (laissez vide pour utiliser la valeur par défaut)",
  connected: "Connecté au compte",
  connecting: "Connexion au compte…",
  reconnecting: "Rétablissement de la connexion…",
  retrying: "Connexion au service en ligne…",
  onlineHosts: "Hôtes en ligne",
  staleHosts: "La liste des hôtes est obsolète",
  hostCount: "{{count}} hôtes en ligne",
  selected: "Sélectionné",
  emptyHosts:
    "Connectez-vous au même compte dans Desktop sur un autre ordinateur. Son daemon intégré sera connecté automatiquement. Activez la synchronisation dans les paramètres de connexion pour les autres hôtes.",
  disconnectRemoteHost: "Déconnecter l’hôte distant",
  hostListHint: "Les hôtes en ligne sont ajoutés automatiquement à la liste.",
  hostSettingsHint:
    "Desktop synchronise son daemon intégré après la connexion. La déconnexion supprime son autorisation serveur ; reconnectez-le explicitement pour l’autoriser de nouveau. Les autres hôtes nécessitent une synchronisation manuelle.",
  requestFailed: "La requête a échoué.",
  managedHost: "Hôte sans surveillance",
  managedHostHint:
    "Cet hôte maintient sa propre connexion serveur. La déconnexion efface son autorisation et exige une nouvelle connexion pour le reconnecter.",
  syncTitle: "Synchroniser avec le service en ligne",
  syncDescription:
    "Connectez cet hôte à votre service en ligne pour y accéder depuis vos autres appareils.",
  signInFirst: "Connectez-vous d’abord dans App → Service en ligne.",
  hostDisconnected: "Connectez-vous d’abord à cet hôte.",
  updateHost: "Mettez cet hôte à jour pour activer la synchronisation du service en ligne.",
  hostOnline: "Connecté au service en ligne",
  hostConnecting: "Connexion au service en ligne…",
  hostOffline: "Non connecté au service en ligne",
  syncHost: "Synchronisation de l’hôte",
  syncHostHint:
    "Autorisez {{name}} à rester en ligne indépendamment. La déconnexion de l’application ne le déconnecte pas. Déconnecter cet hôte ferme toutes les connexions serveur et supprime son autorisation.",
  syncAction: "Se connecter au service en ligne",
  syncing: "Synchronisation…",
  stopSync: "Arrêter la synchronisation",
} as const;
