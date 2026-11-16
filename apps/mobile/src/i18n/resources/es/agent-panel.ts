export const agentPanel = {
  states: {
    notFound: "Agente no encontrado",
    failedToLoad: "No se pudo cargar el agente",
    reconnecting: "Reconectando al host",
    updating: "Actualizando mensajes",
    timelineSyncFailed: "No se pudo actualizar el historial del agente.",
    timelineSyncRetrying: "Reintentando…",
    archivingTitle: "Archivando agente...",
    archivingSubtitle: "Espere mientras archivamos este agente.",
  },
  unavailable: {
    selectedHost: "Host seleccionado",
    unknownHost:
      "No se puede abrir este agente porque {{serverLabel}} no está configurado en este dispositivo.",
    addHost:
      "Agregue el host en Configuración o abra un agente en un servidor configurado para continuar.",
    preparingSession: "Preparando la sesión de {{serverLabel}}...",
    connecting: "Conectando a {{serverLabel}}...",
    showSoon: "Mostraremos este agente en un momento.",
    showWhenOnline: "Mostraremos este agente cuando el host esté en línea.",
    reconnectingTo: "Reconectando a {{serverLabel}}...",
    showAgainWhenReachable: "Volveremos a mostrar este agente en cuanto se pueda acceder al host.",
  },
  archived: {
    callout: "Este agente está archivado.",
    unarchive: "Desarchivar",
  },
};
