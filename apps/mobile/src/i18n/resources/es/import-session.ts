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
    noPrompt: "Sin vista previa inmediata",
  },
  empty: {
    noRecent: "No hay sesiones recientes para importar.",
    noMatches: "Ninguna sesión coincide con tu búsqueda.",
    alreadyImported: "Todas las sesiones recientes ya están importadas.",
    noProviderSessions: "No se encontraron sesiones{{provider}}.",
  },
  row: {
    importing: "Importador...",
  },
};
