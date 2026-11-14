export const agentPanel = {
  states: {
    notFound: "Agentno encontrado",
    failedToLoad: "No se pudo cargar el agente",
    reconnecting: "Reconectando al host",
    updating: "Actualizando mensajes",
    timelineSyncFailed: "No se pudo actualizar el historial del agente.",
    timelineSyncRetrying: "Reintentando…",
    archivingTitle: "Agente de archivo...",
    archivingSubtitle: "Espere mientras archivamos este agente.",
  },
  unavailable: {
    selectedHost: "Anfitrión seleccionado",
    unknownHost:
      "No se puede abrir este agente porque{{serverLabel}}no está configurado en este dispositivo.",
    addHost:
      "Agregue el host en Configuración o abra un agente en un servidor configurado para continuar.",
    preparingSession: "Preparando sesión{{serverLabel}}...",
    connecting: "Conectando a{{serverLabel}}...",
    showSoon: "Le mostraremos a este agente en un momento.",
    showWhenOnline: "Le mostraremos a este agente una vez que el anfitrión esté en línea.",
    reconnectingTo: "Reconectándose a{{serverLabel}}...",
    showAgainWhenReachable:
      "Le mostraremos a este agente nuevamente tan pronto como podamos comunicarnos con el anfitrión.",
  },
  archived: {
    callout: "Este agente está archivado.",
    unarchive: "Desarchivar",
  },
};
