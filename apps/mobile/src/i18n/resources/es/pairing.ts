export const pairing = {
  connectionMethods: {
    title: "Agregar conexión",
    direct: {
      title: "Conexión directa",
      description: "Red local o VPN.",
    },
    remoteSsh: {
      title: "SSH remoto",
      description: "Conéctate mediante el cliente SSH de escritorio.",
    },
    scanQr: {
      title: "Escanea el códigoQR",
      description: "Conexión de retransmisión cifrada.",
    },
    pasteLink: {
      title: "Pegar enlace de emparejamiento",
      description: "Conexión de retransmisión cifrada.",
    },
  },
  direct: {
    title: "Conexión directa",
    helper: "Ingrese la dirección de un servidor Ait.",
    fields: {
      host: "Host",
      port: "Puerto",
      password: "Contraseña",
      optional: "Opcional",
      useSsl: "Usar SSL",
      connectionUri: "URI de conexión",
    },
    advanced: {
      label: "Avanzado",
      show: "Mostrar avanzado",
      hide: "Ocultar avanzado",
    },
    passwordVisibility: {
      show: "Mostrar contraseña",
      hide: "Ocultar contraseña",
    },
    actions: {
      cancel: "Cancelar",
      connect: "Conectar",
      connecting: "Conectando...",
    },
    errors: {
      hostRequired: "Se requiereHost",
      invalidPort: "El puerto debe estar entre 1 y 65535",
      invalidConnection: "Conexión no válida",
      failedTitle: "La conexión falló",
      failedToConnect: "No pudimos conectarnos a{{endpoint}}.",
      noAdditionalDetails: "{{detail}}(no se proporcionan detalles adicionales)",
      timedOut: "Se agotó el tiempo de conexión. Verifique el host/porty su red.",
      refused: "Conexión rechazada. ¿El servidor se está ejecutando en esta dirección?",
      hostNotFound: "Hostno encontrado. Verifique el nombre de host e inténtelo nuevamente.",
      hostUnreachable: "Hostes inalcanzable. Verifique su red y firewall.",
      tlsError:
        "Error de TLS. Las conexiones directas utilizan SSL solo cuando hay un terminador TLS delante del demonio.",
      unableToConnect:
        "No se puede conectar. Verifique el host/porty que se pueda acceder al demonio.",
      details: "Detalles:{{detail}}",
    },
  },
  remoteSsh: {
    title: "SSH remoto",
    helper: "Conéctate a un daemon de Ait en el host remoto.",
    fields: {
      target: "Host SSH",
    },
    actions: {
      cancel: "Cancelar",
      connect: "Conectar",
      connecting: "Conectando...",
    },
    errors: {
      tokenRequired: "Introduce el token del servidor Ait remoto (32–256 caracteres).",
      targetRequired: "El host SSH es obligatorio",
      invalidTarget: "Introduce un host ssh:// válido",
      failedToConnect: "No se pudo conectar por SSH. {{detail}}",
    },
  },
  link: {
    title: "Pegar enlace de emparejamiento",
    helper: "Pegue el enlace de emparejamiento de su servidor.",
    label: "Enlace de emparejamiento",
    errors: {
      required: "Pegue un enlace de emparejamiento (.../#offer=...)",
      missingOffer: "El enlace debe incluir#offer=...",
      emptyOffer: "La carga útil de la oferta está vacía",
      invalid: "Enlace de emparejamiento no válido",
      unableToPair: "No se puede emparejar el host",
    },
    alert: {
      failedTitle: "El emparejamiento falló",
    },
    actions: {
      cancel: "Cancelar",
      pair: "Par",
      pairing: "Emparejamiento...",
    },
  },
  scan: {
    title: "EscanearQR",
    webUnavailableTitle: "No disponible en la web",
    webUnavailableBody:
      'El escaneoQRno es compatible con la compilación web. Utilice "Pegar enlace" en su lugar.',
    backToSettings: "Volver a configuración",
    cameraPermissionTitle: "Permiso de cámara",
    cameraPermissionBody:
      "Permita el acceso a la cámara para escanear el códigoQRde emparejamiento de su demonio.",
    grantPermission: "Conceder permiso",
    pairing: "Emparejamiento...",
    unableToPair: "No se puede emparejar el host",
    errorTitle: "Error",
  },
  device: {
    loadingOffer: "Cargando oferta de maridaje...",
    failedToLoadOffer: "No se pudo cargar la oferta de emparejamiento.",
    relayDisabled: "El relé no está habilitado. Habilite el relé para emparejar un dispositivo.",
    enableTitle: "¿Habilitar el relé?",
    enableDescription:
      "El relé permite conectar este dispositivo desde cualquier lugar. El tráfico de emparejamiento está cifrado de extremo a extremo.",
    relayDocs: "Cómo funciona el relé",
    relayDocsAccessibility: "Leer cómo funciona el relé de Ait",
    enableRelay: "Habilitar relé",
    enablingRelay: "Habilitando...",
    notNow: "Ahora no",
    directConnectionHint:
      "Sin relé, conéctese directamente por TCP, Tailscale u otra VPN. No se crea ningún código QR.",
    updateRequired: "Actualice el host para habilitar el relé desde Ait Desktop.",
    unavailable: "Oferta de maridaje no disponible.",
    hint: "Escanee este códigoQRcon Ait en su teléfono o copie el enlace a continuación.",
    securityWarning:
      "Trata este enlace de emparejamiento como una contraseña. Cualquiera que lo tenga puede acceder a este daemon.",
    qrUnavailable: "CódigoQRno disponible.",
    qrAccessibility: "Código QR de emparejamiento",
    retry: "Rever",
    copy: "Copiar",
    copied: "Copiado",
  },
};
