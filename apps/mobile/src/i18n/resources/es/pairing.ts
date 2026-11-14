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
};
