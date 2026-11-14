export const onlineService = {
  title: "Servicio en línea",
  connectionDescription: "Inicia sesión y conéctate a tus hosts en línea.",
  loginDescription: "Inicia sesión para acceder a los equipos conectados a tu servicio en línea.",
  accountTitle: "Cuenta",
  accountInfo: "Información de la cuenta",
  accountInfoHint:
    "Los datos de la cuenta se actualizan cada 15 segundos mientras esta página está visible.",
  loginName: "Nombre de inicio de sesión",
  accountExpiry: "La cuenta caduca el",
  neverExpires: "Nunca caduca",
  accountInfoFailed:
    "No se pudo cargar la información de la cuenta. Usa Actualizar para volver a intentarlo.",
  signIn: "Iniciar sesión",
  unifiedLogin: "Iniciar sesión / Registrarse",
  loginPortal: "Portal de inicio de sesión unificado",
  loginPortalHint:
    "Abre la página de inicio de sesión unificado en el navegador predeterminado del sistema.",
  openInBrowser: "Abrir en el navegador",
  loginPortalOpenFailed:
    "No se pudo abrir la página de inicio de sesión unificado. Inténtalo de nuevo.",
  unifiedHint:
    "Continúa en el navegador con correo electrónico, Google o WeChat. Allí también puedes verificar tu correo o restablecer la contraseña.",
  browserWaiting: "Esperando el inicio de sesión en el navegador…",
  expiresAt: "El acceso a la cuenta caduca: {{date}}",
  signOut: "Cerrar sesión",
  deleteAccount: "Eliminar cuenta",
  deleteAccountTitle: "¿Quieres eliminar tu cuenta?",
  deleteAccountMessage:
    "Se eliminará tu cuenta y se cerrará la sesión. Los hosts conectados perderán el acceso al servicio en línea. Esta acción no se puede deshacer.",
  deleteAccountInBrowser:
    "Termina de eliminar la cuenta en el navegador. La sesión se cerrará aquí cuando confirmes.",
  signOutHint:
    "Cerrar sesión libera la concesión del daemon vinculado a este cliente. Los demás hosts siguen sincronizados hasta que los detengas individualmente.",
  refresh: "Actualizar",
  serviceSettings: "Ajustes del servicio",
  hideServiceSettings: "Ocultar ajustes del servicio",
  serviceUrl: "URL del servicio",
  serviceUrlHint: "URL del servicio (déjala en blanco para usar la predeterminada)",
  connected: "Conectado a la cuenta",
  connecting: "Conectando a la cuenta…",
  reconnecting: "Restableciendo la conexión…",
  retrying: "Conectando al servicio en línea…",
  onlineHosts: "Hosts en línea",
  staleHosts: "La lista de hosts está desactualizada",
  hostCount: "{{count}} hosts en línea",
  selected: "Seleccionado",
  emptyHosts:
    "Inicia sesión con la misma cuenta en Desktop desde otro equipo. Su daemon integrado se conectará automáticamente. Activa la sincronización en los ajustes de conexión para los demás hosts.",
  disconnectRemoteHost: "Desconectar host remoto",
  hostListHint: "Los hosts en línea se añaden automáticamente a la lista.",
  hostSettingsHint:
    "Desktop sincroniza su daemon integrado después de iniciar sesión. Al desconectar se elimina la autorización del servidor; vuelve a conectarlo explícitamente para autorizarlo. Los demás hosts requieren sincronización manual.",
  requestFailed: "La solicitud falló.",
  managedHost: "Host desatendido",
  managedHostHint:
    "Este host mantiene su propia conexión con el servidor. Al desconectarlo se borra su autorización y se necesita un nuevo inicio de sesión para volver a conectar.",
  syncTitle: "Sincronizar con el servicio en línea",
  syncDescription:
    "Conecta este host a tu servicio en línea para acceder desde tus otros dispositivos.",
  signInFirst: "Inicia sesión primero en Aplicación → Servicio en línea.",
  hostDisconnected: "Conéctate primero a este host.",
  updateHost: "Actualiza este host para activar la sincronización del servicio en línea.",
  hostOnline: "Conectado al servicio en línea",
  hostConnecting: "Conectando al servicio en línea…",
  hostOffline: "No conectado al servicio en línea",
  syncHost: "Sincronización del host",
  syncHostHint:
    "Autoriza {{name}} para que permanezca conectado de forma independiente. Cerrar sesión en la aplicación no lo desconecta. Desconectar este host cierra todas las conexiones y elimina su autorización.",
  syncAction: "Conectar al servicio en línea",
  syncing: "Sincronizando…",
  stopSync: "Detener sincronización",
} as const;
