export const importSession = {
  chooseHostTitle: "Importar desde el host",
  title: "Importar sesión",
  searchPlaceholder: "Buscar sesiones...",
  scope: {
    host: "Sesiones en {{host}}",
    workspace: "Este espacio de trabajo",
  },
  filters: {
    all: "Todo",
    label: "Filtro: {{provider}}",
    title: "Filtrar por proveedor",
  },
  status: {
    connectHost: "Conéctese a un host para importar sesiones",
    updateHost: "Actualice el host para importar sesiones.",
    noProviders: "No hay proveedores importables habilitados.",
    loading: "Cargando sesiones recientes...",
    failedProvider: "No se pudieron cargar las sesiones de {{provider}}",
    failedImport: "No se pudo importar la sesión seleccionada.",
    workingDirectoryUnavailable:
      "El directorio de trabajo de la sesión no está disponible en este host: {{path}}. Restáuralo e inténtalo de nuevo.",
  },
  actions: {
    refresh: "Actualizar sesiones",
    showAll: "Mostrar todo",
    loadMore: "Cargar más",
  },
  preview: {
    untitledSession: "Sesión sin título",
    noPrompt: "Sin vista previa del prompt",
  },
  sessionPreview: {
    title: "Vista previa de la sesión",
    open: "Vista previa del historial de la sesión (solo lectura)",
    activeNotice:
      "Esta sesión tiene un turno sin terminar. La vista previa es de solo lectura y no la reanuda.",
    loadOlder: "Cargar mensajes anteriores",
    loadingOlder: "Cargando mensajes anteriores...",
    empty: "No hay historial visible en esta sesión.",
    failed: "No se pudo leer el historial de la sesión.",
    retry: "Reintentar",
    you: "Tú",
    assistant: "Asistente",
    compacted: "El contexto anterior fue compactado",
    toolRunning: "en ejecución",
    toolCompleted: "completado",
    toolFailed: "fallido",
    toolCanceled: "cancelado",
  },
  empty: {
    noRecent: "No hay sesiones recientes para importar.",
    noMatches: "Ninguna sesión coincide con tu búsqueda.",
    alreadyImported: "Todas las sesiones recientes ya están importadas.",
    noProviderSessions: "No se encontraron sesiones de {{provider}}.",
  },
  row: {
    importing: "Importando...",
  },
};
