const sharedSharing = {
  sharedWithMeCount: "Compartido conmigo ({{count}})",
  incomingHostHint:
    "Los hosts habilitados y sus proyectos y espacios compartidos aparecen en tus listas.",
  showHostInHosts: "Mostrar {{host}} en Hosts",
  sharedByMeCount: "Compartido por mí ({{count}})",
  action: "Compartir con grupos",
  manage: "Gestionar",
  refresh: "Actualizar",
  stop: "Dejar de compartir",
  active: "Activo",
  loading: "Cargando recursos compartidos…",
  hostSection: "Compartir",
  hostTitle: "Compartir este nodo",
  hostHint: "Da a un grupo acceso a todos los proyectos y espacios de trabajo de este host.",
  shareTitle: "Compartir {{name}}",
  resourceTitle: "Compartir recurso",
  intro:
    "Los miembros del grupo pueden abrir este recurso en AIT mientras el propietario esté conectado.",
  projectHostHint: "Comparte este proyecto y sus espacios de trabajo en el host seleccionado.",
  hostUnavailable: "Este host no está disponible. Conéctate para gestionar el uso compartido.",
  scope: { node: "Nodo", project: "Proyecto", workspace: "Espacio de trabajo" },
  scopeHint: {
    node: "Todo en este nodo",
    project: "Este proyecto y sus espacios",
    workspace: "Solo este espacio de trabajo",
  },
  status: {
    shared: "Compartido",
    rejected: "No compartido · {{code}}",
    waiting: "Esperando al servicio",
    groupAdmin: "Gestionas este grupo",
    groupMember: "Miembro del grupo",
  },
  noGroupsTitle: "No hay grupos disponibles",
  noGroupsHint:
    "Conecta este nodo al servicio en línea desde Ajustes del host → Conexiones y comprueba que el propietario pertenezca a un grupo.",
  nodeSettingsOnly:
    "Estos ajustes requieren compartir el nodo completo. Esta compartición solo da acceso a su proyecto o espacio de trabajo.",
  ownerOnly:
    "Solo el propietario del nodo puede gestionar el uso compartido. Los recursos compartidos contigo no se pueden volver a compartir.",
  unsupported:
    "Este nodo no admite la gestión del uso compartido. Actualízalo y vuelve a intentarlo.",
  loadError: "No se pudieron cargar los ajustes para compartir.",
  updateError: "No se pudo actualizar la compartición.",
  stopError: "No se pudo detener la compartición.",
  emptyIncomingTitle: "Aún no se ha compartido nada contigo",
  emptyIncomingHint: "Aquí aparecerán los recursos compartidos por miembros de tus grupos.",
  emptyOutgoingTitle: "No estás compartiendo nada",
  emptyOutgoingHint:
    "Usa el menú de un espacio de trabajo o proyecto, o abre los ajustes del host para compartir un nodo completo.",
} as const;

export const sharing = {
  ...sharedSharing,
  title: "Compartidos",
  listError: "No se pudieron cargar los recursos compartidos.",
};
