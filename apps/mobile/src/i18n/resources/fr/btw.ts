export const btw = {
  attachmentsUnsupported:
    "Les questions secondaires acceptent uniquement du texte. Supprimez les pièces jointes pour envoyer cette question.",
  contextLimit:
    "Cette conversation secondaire a atteint sa limite de contexte. Ouvrez-la comme conversation complète pour continuer.",
  commandDescription: "Poser la question dans une conversation secondaire",
  requiresConversation: "Commencez une conversation avant de poser une question secondaire.",
  expandedTitle: "Conversation secondaire",
  unsupportedProvider: "Ce fournisseur n’expose pas les conversations secondaires sur cet hôte.",
  title: "Question secondaire",
  source: "Depuis {{title}}",
  sourceFallback: "cette conversation",
  newConversation: "Nouvelle conversation secondaire",
  history: "Historique des conversations secondaires",
  expand: "Ouvrir comme conversation complète",
  close: "Fermer la conversation secondaire",
  question: "Question secondaire",
  send: "Envoyer la question secondaire",
  stop: "Arrêter la question secondaire",
  newPlaceholder: "Poser une question secondaire…",
  followUpPlaceholder: "Poser une question complémentaire…",
  emptyTitle: "Interroger cette conversation",
  emptyDescription:
    "Un espace séparé pour réfléchir pendant que votre conversation principale continue.",
  emptyHistory: "Aucune conversation secondaire pour le moment.",
  thinking: "Réflexion…",
  footer: "Les conversations secondaires restent séparées de votre conversation principale.",
  shortened:
    "Une partie du contexte ou de l’aperçu a été omise. Développez pour continuer avec la conversation enregistrée.",
  unsupportedHost:
    "Les conversations secondaires ne sont pas disponibles sur cet hôte. Mettez-le à jour et réessayez.",
  cancelFailed: "Impossible d’arrêter cette question secondaire. Elle est toujours en cours.",
  busy: "Une question secondaire est toujours en cours. Attendez sa réponse ou arrêtez-la d’abord.",
  invalidQuestion: "Saisissez une question dans la limite autorisée.",
  unknownTopic: "Cette conversation secondaire n’est plus disponible.",
  topicNotSettled: "Attendez cette réponse avant de poursuivre.",
  askFailed: "Impossible d’envoyer la question secondaire.",
  uncertain:
    "L’hôte n’a pas encore confirmé cette question. Actualisez son état avant de la renvoyer.",
  states: {
    cancelled: "Arrêtée",
    interrupted: "Interrompue. Vous pouvez démarrer une nouvelle conversation secondaire.",
  },
} as const;
