export const bots = {
  modeCapWarning:
    "Los modos que permiten a los agentes ejecutar comandos también les permiten leer las credenciales de Ait de este equipo y eludir los límites de Ait del Bot. Permítelos solo para trabajo que dejarías hacer a un agente sin supervisión.",
  commandReachWarning:
    "El Bot puede enviar instrucciones a todos los agentes de este espacio de trabajo, incluidos los que iniciaste tú, y cambiar su configuración. Esos agentes conservan sus propios permisos, por lo que una instrucción del Bot puede alcanzar capacidades más allá de los límites del propio Bot.",
  terminalsWarning:
    "Las terminales ejecutan comandos en este equipo, por lo que pueden leer sus credenciales de Ait y eludir los límites de Ait del Bot. Actívalas solo para trabajo que dejarías hacer a un agente sin supervisión.",
  noteInstructionsInInput:
    "Este proveedor no admite un prompt de sistema actualizable, así que las instrucciones integradas y la memoria del Bot se añaden a la entrada de la conversación.",
  noteNoReadOnlyMode:
    "Este proveedor no tiene modo de solo lectura, así que el Bot se ejecuta en su modo predeterminado. Las escrituras que requieren aprobación se deniegan automáticamente, pero las acciones que el proveedor permite sin aprobación no se bloquean, incluido leer las credenciales de Ait de este equipo y eludir los límites de Ait del Bot.",
  noteToolsNeedApproval:
    "Este proveedor no puede preaprobar herramientas de Ait, así que las llamadas a herramientas de Ait que requieren aprobación se deniegan automáticamente. Puede que el Bot solo pueda conversar, sin iniciar ni gestionar agentes.",
  noteNoAitTools:
    "Este proveedor no admite la conexión con herramientas de Ait, así que el Bot solo puede conversar.",
  permissions: "Permisos",
  permissionsHint: "Los cambios de permisos se aplican en el siguiente turno del Bot.",
  groupObserve: "Observar (siempre activo): ver agentes, actividad y proveedores",
  groupAgents: "Agentes: crear agentes, enviarles instrucciones y reconfigurarlos",
  groupStop: "Detener: cancelar y detener agentes",
  groupSchedules: "Programaciones: crear y gestionar programaciones",
  groupScripts: "Scripts: ejecutar scripts del espacio de trabajo",
  groupTerminals: "Terminales: crear terminales y escribir en ellas",
  permissionRiskHint: "Puede ejecutar comandos.",
  modeCap: "Modo de agente predeterminado",
  modeNotAllowed: "No permitido",
  modeCapHint:
    "Elige el modo de los agentes que el Bot inicia para cada proveedor. El Bot solo puede usar este modo; elige No permitido para impedir que use un proveedor.",
  providerUnavailable: "{{provider}} (no disponible)",
  readOnlyHint:
    "El Bot se ejecuta en el modo más estricto que ofrece su proveedor (en Claude, el modo predeterminado, donde tus propias reglas de permiso siguen aplicándose) y deja los cambios a los agentes que inicia.",
  title: "Bot",
  chat: "Chat",
  settings: "Ajustes",
  provider: "Proveedor",
  model: "Modelo",
  mode: "Modo de permisos",
  thinking: "Razonamiento",
  defaultOption: "Predeterminado del proveedor",
  save: "Guardar",
  setupTitle: "Configurar el Bot del espacio de trabajo",
  setupHint:
    "El Bot gestiona este espacio de trabajo y sigue funcionando después de cerrar esta pestaña.",
  setup: "Configurar Bot",
  clearMemory: "Borrar memoria guardada",
  notConfigured:
    "Este espacio de trabajo aún no tiene un Bot. Solo el propietario del host puede configurarlo.",
  disabled: "Activa las funciones experimentales en Ajustes → Acerca de para usar Bots.",
  unsupported: "Actualiza este host para usar Bots.",
  disconnected: "Conéctate a este host para usar Bots.",
  message: "Mensaje",
  cancel: "Cancelar turno",
  emptyChat: "Envía un mensaje para iniciar una conversación.",
  conversation: "Conversación",
  conversationOwner: "Ait (tú)",
  conversationOtherChannel: "Otro canal",
  conversationReadOnly:
    "Solo lectura: esta conversación ocurre en {{channel}}. Aquí solo tu conversación de Ait admite mensajes.",
  session: "Sesión del Bot",
  reset: "Nueva conversación",
  resetConfirm:
    "¿Iniciar una nueva conversación? Los mensajes existentes se conservan y se aplicará la última configuración del Bot.",
  configurationHint:
    "Los cambios de modelo se aplican a las conversaciones nuevas. Inicia una nueva conversación para usarlos aquí.",
  memory: "Memoria",
  memoryHint:
    "Preferencias que el Bot lee antes de cada respuesta, como el idioma del título de los PR o el estilo de respuesta.",
  saveMemory: "Guardar memoria",
  memoryConflict: "La memoria cambió en otro lugar. Recárgala antes de volver a guardar.",
  resumeToEdit: "Reanuda el Bot para hacer cambios.",
  reload: "Recargar",
  pause: "Pausar",
  remove: "Quitar Bot",
  removeConfirm:
    "¿Quitar este Bot? Su historial de conversación dejará de ser visible, su memoria se conserva y se desconectarán las conexiones de Telegram y Feishu.",
  paused: "En pausa",
  resume: "Reanudar",
  directoryMissing:
    "Falta el directorio del espacio de trabajo, por lo que el Bot está en pausa. Restaura el directorio y reanuda.",
  requestFailed: "La solicitud falló",
  tooLong: "Los mensajes deben tener como máximo 16 KiB.",
  memoryTooLong: "La memoria debe tener como máximo 32 KiB.",
  channels: "Canales",
  addChannel: "Añadir canal",
  channelType: "Tipo de canal",
  telegram: "Telegram",
  feishu: "Feishu",
  lark: "Lark",
  tokenRef: "Referencia del token del Bot",
  secretRef: "Referencia del secreto de la app",
  appId: "ID de la app",
  allowedSenders: "Remitentes permitidos (separados por comas o saltos de línea)",
  allowGroups: "Permitir chats de grupo",
  channelRefHint:
    "Las credenciales provienen de las variables de entorno del proceso que inicia el daemon. Introduce solo la referencia de la variable, como env:AIT_SERVER_CREDENTIAL_TELEGRAM, y al menos un remitente permitido.",
  channelEnabled: "Activado",
  channelConnected: "Conectado",
  channelIdle: "Inactivo",
  feishuScanHint:
    "Escanea un código QR con la app de Feishu. Feishu crea un bot para ti y Ait lo conecta; solo tú podrás escribirle hasta que lo cambies.",
  feishuScanStart: "Mostrar código QR",
  feishuScanWaiting:
    "Escanea este código con la app de Feishu y aprueba el nuevo bot. Mantén esta página abierta hasta que termine.",
  feishuScanOpen: "Abrir en Feishu",
  feishuScanQrLabel: "Código QR de configuración de Feishu",
  feishuScanRetry: "Reintentar",
  feishuScanDenied: "La solicitud se rechazó en Feishu.",
  feishuScanExpired: "Este código QR caducó. Empieza de nuevo para obtener otro.",
  feishuScanLark:
    "Este código QR es para Feishu. Para una cuenta de Lark, selecciona Lark e introduce las credenciales de la app manualmente.",
  feishuScanUnavailable:
    "No se pudo completar la configuración con Feishu. Reintenta o introduce las credenciales manualmente.",
  feishuManualEntry: "Introducir credenciales manualmente",
  feishuScanInstead: "Escanear un código QR",
  feishuAppSecret: "App Secret",
  feishuAppSecretHint:
    "Pega el App Secret. Ait lo verifica con {{platform}} y lo guarda de forma privada en este host. También sirve una referencia env:AIT_SERVER_CREDENTIAL_*.",
  feishuManualStep1: "Crea una app personalizada en {{developerUrl}} y activa la capacidad de bot.",
  feishuManualStep2:
    "En Permisos, añade im:message:send_as_bot, im:message.p2p_msg:readonly, im:message.group_at_msg:readonly.",
  feishuManualStep3:
    "Copia aquí el App ID y el App Secret, añade tu Open ID de {{platform}} (ou_...) como remitente permitido y añade el canal.",
  feishuManualStep4:
    "Después, en Eventos y devoluciones de llamada de {{platform}}, elige Conexión larga y suscríbete a im.message.receive_v1. {{platform}} solo puede guardarlo cuando el canal está conectado.",
  feishuManualStep5: "Publica una versión de la app para que se pueda escribir al bot.",
  telegramGuideStep1: "Escanea este código o usa el botón para abrir BotFather en Telegram.",
  telegramGuideStep2: "Envía /newbot y elige un nombre y un nombre de usuario que termine en bot.",
  telegramGuideStep3: "Copia el token que envía BotFather y pégalo abajo.",
  telegramBotFatherOpen: "Abrir BotFather",
  telegramBotFatherQrLabel: "Código QR que abre BotFather",
  telegramToken: "Token del bot",
  telegramTokenHint:
    "Ait verifica el token con Telegram y lo guarda de forma privada en este host.",
  telegramContinue: "Continuar",
  telegramPairWaiting:
    "Escanea este código o abre el enlace y pulsa Start en el chat con @{{bot}}. Así serás el remitente permitido. Mantén esta página abierta hasta que termine.",
  telegramPairOpen: "Abrir en Telegram",
  telegramPairQrLabel: "Código QR que abre tu nuevo bot",
  telegramInvalidToken: "Telegram rechazó este token. Cópialo de nuevo desde BotFather.",
  telegramAlreadyConnected:
    "Este bot ya está conectado a un canal en este host. Elimina ese canal primero o crea un bot nuevo.",
  telegramWebhookConflict:
    "Este bot tiene un webhook configurado. Elimínalo con deleteWebhook y reintenta.",
  telegramPollingConflict:
    "Otro programa está recibiendo los mensajes de este bot. Detenlo y reintenta.",
  telegramExpired: "Este enlace caducó. Pulsa Continuar para obtener otro.",
  telegramUnavailable:
    "No se pudo conectar con Telegram. Comprueba la conexión de este host y reintenta.",
  telegramUseEnvironment: "Usar una variable de entorno",
  telegramUseGuided: "Usar la configuración guiada",
  weixin: "WeChat",
  qq: "QQ",
  weixinScanHint:
    "Escanea un código QR con WeChat para conectar un bot. Solo tú puedes escribirle, en chats privados.",
  weixinScanWaiting:
    "Escanea este código con WeChat y confirma en tu teléfono. Mantén esta página abierta hasta que termine.",
  weixinScanOpen: "Abrir en WeChat",
  weixinScanQrLabel: "Código QR de configuración de WeChat",
  weixinScanUnavailable: "No se pudo completar la configuración con WeChat. Inténtalo de nuevo.",
  qqScanHint:
    "Escanea un código QR con QQ. QQ crea un bot y Ait lo conecta; solo tú puedes escribirle hasta que lo cambies.",
  qqScanWaiting:
    "Escanea este código con QQ y aprueba el nuevo bot. Mantén esta página abierta hasta que termine.",
  qqScanOpen: "Abrir en QQ",
  qqScanQrLabel: "Código QR de configuración de QQ",
  qqScanUnavailable:
    "No se pudo completar la configuración con QQ. Inténtalo de nuevo o introduce las credenciales manualmente.",
  scanAlreadyConnected:
    "Este bot ya está conectado a un canal en este host. Elimina ese canal e inténtalo de nuevo.",
  qqManualStep1: "Crea un bot en q.qq.com y copia aquí su App ID y App Secret.",
  qqManualStep2:
    "Después de añadir el canal, obtén un código de vinculación en su fila y envíaselo al bot en QQ. Así serás un remitente permitido.",
  qqAppSecretHint: "Ait comprueba el App Secret con QQ y lo guarda de forma privada en este host.",
  qqPairButton: "Obtener código de vinculación",
  qqPairHint:
    "Código: {{code}}. Envíaselo al bot en un chat privado o menciona al bot con él en un grupo de QQ. Funciona una vez y caduca en 10 minutos.",
  imessage: "iMessage",
  webhook: "Webhook",
  webhookHint:
    "Ait Hub crea una dirección HTTPS y un secreto para este Bot. Cualquier sistema que pueda enviar una solicitud HTTP puede usarlos para enviarle mensajes al Bot y leer su respuesta.",
  webhookCreate: "Crear webhook",
  webhookUrl: "Dirección",
  webhookSecret: "Secreto",
  webhookSecretOnce:
    "El secreto solo se muestra esta vez. Cópialo ahora; si lo pierdes, crea uno nuevo.",
  webhookExample: "Ejemplo",
  webhookExampleHint:
    "La llamada responde con una dirección de estado. Consúltala con el mismo secreto (añade ?wait=30 para esperar la respuesta), o incluye callback_url en la llamada para recibir la respuesta allí. Las llamadas con el mismo thread continúan una conversación.",
  webhookCopy: "Copiar",
  webhookCopied: "Copiado",
  webhookDone: "Listo",
  webhookRotate: "Crear nuevo secreto",
  webhookRotateConfirm:
    "El secreto actual deja de funcionar de inmediato. Quienes lo usen necesitarán el nuevo.",
  webhookCenterOffline:
    "Este host no está conectado a Ait Hub. Inicia sesión y activa la conexión con Hub, y vuelve a intentarlo.",
  webhookCenterUnsupported: "Este Ait Hub todavía no admite webhooks.",
  webhookAutomationUnavailable: "La automatización no está activada en este Ait Hub.",
  webhookLimitReached:
    "Tu cuenta alcanzó el límite de webhooks. Elimina uno que ya no uses y vuelve a intentarlo.",
  webhookCenterError: "Ait Hub no pudo configurar el webhook. Vuelve a intentarlo más tarde.",
  imessageGuideStep1: "Crea un proyecto de Photon Beta en app.beta.photon.codes.",
  imessageGuideStep2:
    "En ese proyecto, crea una clave de API con project:read, project:write, platforms:read y platforms:write.",
  imessageGuideStep3:
    "Introduce abajo el Project ID, la clave de API y tu propio número de teléfono o correo de iMessage.",
  imessagePhotonOpen: "Abrir Photon",
  imessagePrivacyNotice:
    "Los mensajes con este bot pasan por el servicio de Photon. Solo se admiten mensajes directos, no chats de grupo.",
  imessageProjectId: "Project ID",
  imessageApiKey: "Clave de API",
  imessageApiKeyHint: "Ait verifica la clave con Photon y la guarda de forma privada en este host.",
  imessageHandle: "Tu número de teléfono o correo de iMessage",
  imessageHandleHint:
    "La dirección desde la que envías iMessages. Escribe los teléfonos en formato internacional, como +14155550123.",
  imessageContinue: "Continuar",
  imessagePairPhoneNumber: "Enviar a",
  imessagePairCode: "Código",
  imessagePairWaiting:
    "Envía este código como iMessage, no como SMS, exactamente desde {{handle}}. Al escanear el código QR o abrir el enlace se rellena automáticamente. Mantén esta página abierta hasta que termine.",
  imessagePairExpires: "El código caduca en {{minutes}} min.",
  imessagePairOpen: "Abrir en Mensajes",
  imessagePairQrLabel: "Código QR que abre Mensajes con el código ya escrito",
  imessageInvalidKey:
    "Photon rechazó este Project ID o clave de API. Comprueba ambos y que la clave tenga los cuatro permisos.",
  imessageInvalidHandle:
    "Introduce un teléfono en formato internacional, como +14155550123, o un correo de iMessage.",
  imessageDedicatedLine:
    "Este proyecto de Photon usa líneas dedicadas, que no son compatibles. Usa un proyecto con líneas compartidas.",
  imessageAlreadyConnected:
    "Este proyecto de Photon ya está conectado a un canal en este host. Elimina ese canal primero o usa otro proyecto.",
  imessageExpired: "Este código caducó. Pulsa Continuar para obtener otro.",
  imessageUnavailable:
    "No se pudo conectar con Photon. Comprueba la conexión de este host y reintenta.",
  deleteChannel: "Eliminar canal",
  deleteChannelConfirm: "¿Eliminar este canal? Su conexión externa se desconectará.",
  notifications: {
    title: "Notificaciones",
    wokeBy: "Novedades del espacio de trabajo",
    silent: "Nada que informar.",
    wakeScope: "Despertar al Bot por",
    wakeScopeOwn: "Solo su propio trabajo (predeterminado)",
    wakeScopeAll: "Todo en este espacio de trabajo",
    wakeScopeHint:
      "Su propio trabajo abarca los agentes y las tareas programadas que inició el Bot. Con Todo, cualquier otro evento de agente, tarea programada o terminal de este espacio de trabajo también inicia un turno del Bot, lo que consume más llamadas al modelo.",
    recent: "Notificaciones recientes",
    empty: "Aún no hay notificaciones.",
    state: {
      logged: "Fuera del alcance de activación",
      pending: "Esperando para despertar al Bot",
      woke: "Despertó al Bot",
      other: "Otro estado",
    },
    skipped: {
      over_budget: "No entregada: se alcanzó el límite de activaciones por hora",
      inbox_full: "No entregada: demasiadas notificaciones en espera",
      paused: "No entregada: el Bot estaba en pausa",
      observed: "No repetida: el Bot ya había revisado este agente en esa conversación",
      blocked: "No entregada: esa conversación se interrumpió y necesita reiniciarse",
      reported: "No repetida: el agente ya había informado este resultado en su tarea",
      other: "No despertó al Bot",
    },
    kind: {
      agent_finished: "Agente terminado",
      agent_failed: "Agente con error",
      agent_permission: "El agente necesita permiso",
      schedule_succeeded: "Ejecución programada correcta",
      schedule_failed: "Ejecución programada fallida",
      terminal_attention: "La terminal requiere atención",
      ticket_blocked: "El agente está bloqueado en una tarea",
      ticket_done: "El agente terminó su parte de una tarea",
      ticket_failed: "El agente no pudo terminar su parte de una tarea",
      other: "Novedad del espacio de trabajo",
    },
  },
  tickets: {
    title: "Tareas",
    pillOpen: "{{count}} en curso",
    pillWaiting: "Esperándote: {{count}}",
    pillLabel: "Tareas del Bot: {{open}} en curso, {{waiting}} esperándote",
    empty: "No hay tareas en curso. Aparece una tarea cuando el Bot encarga trabajo a un agente.",
    showClosed: "Mostrar las cerradas recientemente",
    hideClosed: "Ocultar las tareas cerradas",
    closedHeading: "Cerradas recientemente",
    noClosed: "No hay tareas cerradas.",
    agentCount: "Agentes: {{count}}",
    agentFallback: "Agente {{id}}",
    agents: "Agentes",
    noAgents: "Todavía no hay ningún agente asignado.",
    request: "Solicitud",
    history: "Historial",
    noReports: "Todavía no hay informes.",
    outcome: "Resultado",
    noOutcome: "No se registró ningún resultado.",
    outcomePlaceholder: "Resultado (opcional)",
    markDone: "Marcar como hecha",
    cancel: "Cancelar tarea",
    cancelConfirm:
      "¿Cancelar esta tarea? Sus agentes siguen en marcha; detenlos por separado si hace falta. Puedes reabrir la tarea más tarde.",
    keep: "Conservar tarea",
    reopen: "Reabrir",
    openLimit: "Hay demasiadas tareas en curso. Cierra las terminadas antes de reabrir esta.",
    conflict: "Esta tarea cambió mientras tanto. Revisa su estado y vuelve a intentarlo.",
    linked: "Tarea: {{title}}",
    untitled: "Tarea sin título",
    movedTo: "ahora {{status}}",
    status: {
      open: "En curso",
      waiting: "Esperándote",
      done: "Hecha",
      failed: "Fallida",
      cancelled: "Cancelada",
      other: "Otro estado",
    },
    waiting: {
      permission: "Un agente necesita tu permiso",
      blocked: "Un agente está bloqueado y necesita tu respuesta",
      other: "Necesita tu atención",
    },
    source: {
      agent: "Agente",
      bot: "Bot",
      user: "Tú",
      system: "Ait",
      other: "Otro",
    },
    report: {
      progress: "Progreso",
      blocked: "Bloqueado",
      done: "Terminó su parte",
      failed: "Abandonó su parte",
      agent_finished: "El agente terminó un turno",
      agent_failed: "Agente con error",
      agent_permission: "El agente necesita permiso",
      note: "Nota",
      status: "Cambio de estado",
      other: "Novedad",
    },
  },
  status: {
    queued: "En cola",
    sending: "Iniciando…",
    running: "Trabajando…",
    needs_approval: "Requiere aprobación",
    completed: "Completado",
    failed: "Falló",
    uncertain: "Interrumpido: inicia una nueva conversación para continuar",
    cancelled: "Cancelado",
  },
};
