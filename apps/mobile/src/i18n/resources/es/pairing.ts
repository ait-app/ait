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
  },
  direct: {
    title: "Conexión directa",
    helper: "Ingrese la dirección del servidor y el token de acceso.",
    fields: {
      host: "Host",
      port: "Puerto",
      password: "Token del servidor",
      optional: "Obligatorio",
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
      hostRequired: "El host es obligatorio",
      invalidPort: "El puerto debe estar entre 1 y 65535",
      invalidConnection: "Conexión no válida",
      failedTitle: "La conexión falló",
      failedToConnect: "No pudimos conectarnos a {{endpoint}}.",
      noAdditionalDetails: "{{detail}} (no se proporcionan detalles adicionales)",
      timedOut: "Se agotó el tiempo de conexión. Verifique el host/puerto y su red.",
      refused: "Conexión rechazada. ¿El servidor se está ejecutando en esta dirección?",
      hostNotFound: "Host no encontrado. Verifique el nombre de host e inténtelo nuevamente.",
      hostUnreachable: "El host no es accesible. Verifique su red y firewall.",
      tlsError:
        "Error de TLS. Las conexiones directas utilizan SSL solo cuando hay un terminador TLS delante del demonio.",
      unableToConnect:
        "No se puede conectar. Verifique el host/puerto y que se pueda acceder al demonio.",
      details: "Detalles: {{detail}}",
    },
  },
  remoteSsh: {
    title: "SSH remoto",
    helper: "Conéctate al servidor Ait remoto por SSH (puerto predeterminado del servidor: 7316).",
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
};
