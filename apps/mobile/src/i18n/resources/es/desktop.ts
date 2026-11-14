export const desktop = {
  windowControls: {
    minimize: "Minimizar ventana",
    maximize: "Maximizar ventana",
    restore: "Restaurar ventana",
    close: "Cerrar ventana",
  },
  quitting: {
    title: "Saliendo de Ait...",
    detail: "Deteniendo el demonio local.",
  },
  daemon: {
    listen: {
      host: "Dirección de escucha",
      port: "Puerto",
      savedHint: "Reinicia el daemon para aplicar los cambios.",
      override:
        "AIT_SERVER_LISTEN sustituye este ajuste: {{address}}. Elimina la variable de entorno para usar la dirección guardada.",
      invalid: "Introduce una dirección IPv4 o IPv6 (o localhost) y un puerto entre 0 y 65535.",
    },
    lifecycle: {
      owned: "Iniciado por esta sesión de escritorio",
      attached: "Conectado a un daemon existente",
      ownedMessage: "Este daemon fue iniciado por esta sesión de escritorio.",
      attachedMessage: "Este daemon no fue iniciado por esta sesión de escritorio.",
      stopTitle: "¿Detener el daemon local?",
      stopMessage:
        "{{ownership}}\nDirectorio principal: {{home}}\nPID del supervisor: {{pid}}\nSe interrumpirán las tareas de agentes en curso.",
      stop: "Detener daemon",
      stopping: "Deteniendo…",
      stopFailed: "No se pudo detener el daemon",
      pauseAttached:
        "¿Pausar la gestión automática del daemon? El daemon conectado seguirá funcionando.",
      pause: "Pausar gestión",
      workerUpdated: "Worker actualizado a {{version}}",
      supervisorRefresh:
        "El supervisor en ejecución conserva su código original. El programa que lo inició debe detenerlo y reiniciarlo para actualizarlo.",
    },
    title: "Daemon",
    status: {
      title: "Estado",
      builtInOnly: "Aquí solo se muestra el demonio de escritorio integrado.",
      running: "correr",
      notRunning: "no corriendo",
      pid: "PID{{pid}}",
    },
    management: {
      title: "Administrar demonio incorporado",
      hint: "Deje que Ait inicie y detenga el demonio incorporado",
      pauseTitle: "Pausar el demonio incorporado",
      pauseMessage:
        "Esto detendrá el demonio incorporado inmediatamente. Se detendrán los agentes en ejecución y los terminales conectados al demonio integrado.",
      pauseAndStop: "Pausa y para",
      registrationFailed:
        "Built-in daemon started, but Ait could not save the localhost connection. Toggle daemon management off and on again, or add localhost manually.",
      pausedStopFailed:
        "La gestión del demonio integrado se pausó, pero Ait no pudo detener el demonio.",
      updateFailed: "No se puede actualizar la gestión de demonios integrada.",
    },
    keepRunning: {
      title: "Mantener el demonio en ejecución después de salir",
      hint: "Los agentes y terminales siguen ejecutándose al salir de Ait; el próximo inicio se vuelve a conectar a ellos.",
    },
    logs: {
      title: "Archivo de registro",
      modalTitle: "RegistrosDaemon",
      unavailable: "Ruta de registro no disponible",
      empty: "(el archivo de registro está vacío)",
      copied: "Ruta de registro copiada.",
      copyFailed: "No se puede copiar la ruta del registro.",
      open: "Abrir registros",
      copyPath: "Copiar ruta",
    },
    fullStatus: {
      title: "Estado completo",
      modalTitle: "EstadoDaemon",
      view: "Ver estado",
      copied: "Estado copiado al portapapeles.",
      fetchFailed: "No se pudo recuperar el estado del demonio:{{message}}",
    },
    versionMismatch:
      "Las versiones de la aplicación y del demonio no coinciden. Actualice ambos a la misma versión para obtener la mejor experiencia.",
    loadFailed: "No se puede cargar el estado del demonio del escritorio.",
  },
  updates: {
    status: {
      checking: "Buscando actualizaciones de la aplicación...",
      installing: "Instalando actualización de la aplicación...",
      upToDate: "La aplicación está actualizada.",
      upToDateWithLastChecked: "Up to date. Last checked at {{time}}.",
      pending: "Le avisaremos cuando la actualización esté lista.",
      pendingWithLastChecked:
        "Le avisaremos cuando la actualización esté lista. Última comprobación a las {{time}}.",
      pendingWithVersion: "Actualización encontrada: {{version}}. Descargando...",
      pendingWithVersionAndLastChecked:
        "Actualización encontrada: {{version}}. Descargando... Última comprobación a las {{time}}.",
      availableWithVersion: "Actualización lista:{{version}}",
      availableWithVersionAndLastChecked:
        "Actualización lista:{{version}}. Última comprobación a las {{time}}.",
      available: "Una actualización de la aplicación está lista para instalarse.",
      availableWithLastChecked:
        "Una actualización de la aplicación está lista para instalarse. Última comprobación a las {{time}}.",
      installed: "Actualización de la aplicación instalada. Se requiere reinicio.",
      failed: "No se pudo actualizar la aplicación.",
      idle: "El estado de la actualización aún no se ha comprobado.",
    },
    installError: "No se puede instalar la actualización de la aplicación de escritorio.",
    callout: {
      installingTitle: "Instalando actualización",
      failedTitle: "La actualización falló",
      availableTitle: "Actualización disponible",
      genericError: "Algo salió mal.",
      whatsNew: "Qué hay de nuevo",
      installingAction: "Instalando...",
      installAndRestart: "Instalar y reiniciar",
      installingDescription: "Instalando y reiniciando...",
      versionReady: "{{version}}está listo para instalar.",
      newVersionReady: "Una nueva versión está lista para instalar.",
      restartWarning:
        "La actualización de la aplicación dejará de ejecutar agentes y cerrará sesiones de terminal.",
    },
  },
  settings: {
    loadFailed: "No se puede cargar la configuración del escritorio.",
    saveFailed: "No se puede guardar la configuración del escritorio.",
  },
  rosetta: {
    title: "Descargue la compilaciónApple Silicon",
    runningIntel: "Estás ejecutando la compilaciónIntelde Ait enRosettaenApple Silicon.",
    highCpu:
      "Esto provoca un uso elevado de la CPU. Descargue la compilaciónApple Siliconpara solucionarlo.",
    download: "Descargar",
  },
  permissions: {
    notifications: {
      allowed: "El sistema operativo permite las notificaciones.",
      denied: "Las notificaciones se niegan en la configuración del sistema.",
      notGranted: "Las notificaciones aún no han sido concedidas.",
      webOnly:
        "El estado de notificación de escritorio solo está disponible en tiempo de ejecución web.",
      supported: "Se admiten notificaciones de escritorio.",
      unsupported: "Las notificaciones de escritorio no son compatibles con esta plataforma.",
      apiUnavailable: "La API de notificación web no está disponible en este entorno.",
      requestsWebOnly:
        "Las solicitudes de notificación de escritorio solo están disponibles en tiempo de ejecución web.",
      requestUnavailable: "RequestPermission() de la API de notificación web no está disponible.",
      requestFailed: "No se pudo solicitar permiso de notificación:{{message}}",
      unexpectedState: "Estado de permiso de notificación inesperado:{{state}}",
    },
    microphone: {
      webOnly:
        "El estado del micrófono de escritorio solo está disponible en tiempo de ejecución web.",
      navigatorUnavailable: "Navigator no está disponible en este entorno.",
      granted: "Se concede acceso al micrófono.",
      denied: "El acceso al micrófono está denegado en la configuración del sistema.",
      notGranted: "Aún no se ha concedido el permiso para el micrófono.",
      unexpectedState: "Estado de permiso de micrófono inesperado:{{state}}",
      statusApiUnavailable:
        "La API de estado del micrófono no está disponible en este tiempo de ejecución. Utilice Solicitud para comprobar el acceso.",
      queryFailed: "No se pudo consultar el estado del micrófono:{{message}}",
      captureUnavailable: "La captura de micrófono no está disponible en este entorno.",
      permissionApiUnavailable:
        "La API de estado de permiso no está disponible. Utilice Solicitud para comprobar el acceso.",
      requestsWebOnly:
        "Las solicitudes de micrófono de escritorio solo están disponibles en tiempo de ejecución web.",
      captureApiUnavailable: "La API de captura de micrófono no está disponible en este entorno.",
      requestDenied: "El usuario o el sistema denegaron el permiso del micrófono.",
      noDevice: "No se encontró ningún dispositivo de micrófono.",
      requestFailed: "No se pudo solicitar el permiso del micrófono:{{message}}",
    },
    empty: {
      notifications: "El estado de la notificación aún no se ha verificado.",
      microphone: "El estado del micrófono aún no se ha comprobado.",
    },
    testNotification: {
      title: "Prueba de notificación Ait",
      body: "Si puede ver esto, las notificaciones de escritorio funcionan.",
      notDelivered:
        "La notificación no fue entregada. Verifique Configuración del sistema> Notificaciones.",
      failed: "No se pudo enviar la notificación.",
    },
  },
};
