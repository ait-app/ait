export const btw = {
  attachmentsUnsupported:
    "Las preguntas laterales solo admiten texto. Quita los archivos adjuntos para enviar esta pregunta.",
  contextLimit:
    "Esta conversación lateral ha alcanzado el límite de contexto. Ábrela como conversación completa para continuar.",
  commandDescription: "Pregunta en una conversación lateral separada",
  requiresConversation: "Inicia una conversación antes de hacer una pregunta lateral.",
  expandedTitle: "Conversación lateral",
  unsupportedProvider: "Este proveedor no ofrece conversaciones laterales en este host.",
  title: "Pregunta lateral",
  source: "De {{title}}",
  sourceFallback: "esta conversación",
  newConversation: "Nueva conversación lateral",
  history: "Historial de conversaciones laterales",
  expand: "Abrir como conversación completa",
  close: "Cerrar conversación lateral",
  question: "Pregunta lateral",
  send: "Enviar pregunta lateral",
  stop: "Detener pregunta lateral",
  newPlaceholder: "Haz una pregunta lateral…",
  followUpPlaceholder: "Haz una pregunta de seguimiento…",
  emptyTitle: "Pregunta sobre esta conversación",
  emptyDescription: "Un espacio separado para pensar mientras continúa tu conversación principal.",
  emptyHistory: "Aún no hay conversaciones laterales.",
  thinking: "Pensando…",
  footer: "Las conversaciones laterales permanecen separadas de la conversación principal.",
  shortened:
    "Se omitió parte del contexto o la vista previa. Expande para continuar con la conversación guardada.",
  unsupportedHost:
    "Las conversaciones laterales no están disponibles en este host. Actualízalo e inténtalo de nuevo.",
  cancelFailed: "No se pudo detener esta pregunta lateral. Sigue en ejecución.",
  busy: "Todavía hay una pregunta lateral en ejecución. Espera su respuesta o detenla primero.",
  invalidQuestion: "Escribe una pregunta dentro del límite de preguntas laterales.",
  unknownTopic: "Esta conversación lateral ya no está disponible.",
  topicNotSettled: "Espera esta respuesta antes de continuar.",
  askFailed: "No se pudo enviar la pregunta lateral.",
  uncertain:
    "El host aún no ha confirmado esta pregunta. Actualiza su estado antes de volver a enviarla.",
  states: {
    cancelled: "Detenida",
    interrupted: "Interrumpida. Puedes iniciar una nueva conversación lateral.",
  },
} as const;
