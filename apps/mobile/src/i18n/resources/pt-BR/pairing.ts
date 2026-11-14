export const pairing = {
  connectionMethods: {
    title: "Adicionar conexão",
    direct: {
      title: "Conexão direta",
      description: "Rede local ou VPN.",
    },
    remoteSsh: {
      title: "SSH remoto",
      description: "Conecte-se pelo cliente SSH do desktop.",
    },
    scanQr: {
      title: "Escanear QR code",
      description: "Conexão relay criptografada.",
    },
    pasteLink: {
      title: "Colar link de pareamento",
      description: "Conexão relay criptografada.",
    },
  },
  direct: {
    title: "Conexão direta",
    helper: "Informe o endereço de um servidor Ait.",
    fields: {
      host: "Host",
      port: "Porta",
      password: "Senha",
      optional: "Opcional",
      useSsl: "Usar SSL",
      connectionUri: "URI da conexão",
    },
    advanced: {
      label: "Avançado",
      show: "Mostrar avançado",
      hide: "Ocultar avançado",
    },
    passwordVisibility: {
      show: "Mostrar senha",
      hide: "Ocultar senha",
    },
    actions: {
      cancel: "Cancelar",
      connect: "Conectar",
      connecting: "Conectando...",
    },
    errors: {
      hostRequired: "Host é obrigatório",
      invalidPort: "A porta deve estar entre 1 e 65535",
      invalidConnection: "Conexão inválida",
      failedTitle: "Falha na conexão",
      failedToConnect: "Não foi possível conectar a {{endpoint}}.",
      noAdditionalDetails: "{{detail}} (nenhum detalhe adicional fornecido)",
      timedOut: "A conexão expirou. Verifique o host/porta e sua rede.",
      refused: "Conexão recusada. O servidor está em execução neste endereço?",
      hostNotFound: "Host não encontrado. Verifique o hostname e tente novamente.",
      hostUnreachable: "Host inacessível. Verifique sua rede e firewall.",
      tlsError:
        "Erro de TLS. Conexões diretas usam SSL apenas quando há um terminador TLS na frente do daemon.",
      unableToConnect:
        "Não foi possível conectar. Verifique o host/porta e se o daemon está acessível.",
      details: "Detalhes: {{detail}}",
    },
  },
  remoteSsh: {
    title: "SSH remoto",
    helper: "Conecte-se a um daemon Ait no host remoto.",
    fields: {
      target: "Host SSH",
    },
    actions: {
      cancel: "Cancelar",
      connect: "Conectar",
      connecting: "Conectando...",
    },
    errors: {
      tokenRequired: "Insira o token do servidor Ait remoto (32–256 caracteres).",
      targetRequired: "O host SSH é obrigatório",
      invalidTarget: "Insira um host ssh:// válido",
      failedToConnect: "Não foi possível conectar por SSH. {{detail}}",
    },
  },
  link: {
    title: "Colar link de pareamento",
    helper: "Cole o link de pareamento do seu servidor.",
    label: "Link de pareamento",
    errors: {
      required: "Cole um link de pareamento (.../#offer=...)",
      missingOffer: "O link deve incluir #offer=...",
      emptyOffer: "O payload da oferta está vazio",
      invalid: "Link de pareamento inválido",
      unableToPair: "Não foi possível parear host",
    },
    alert: {
      failedTitle: "Falha no pareamento",
    },
    actions: {
      cancel: "Cancelar",
      pair: "Parear",
      pairing: "Pareando...",
    },
  },
  scan: {
    title: "Escanear QR",
    webUnavailableTitle: "Indisponível na web",
    webUnavailableBody:
      'Escanear QR não é compatível com a build web. Use "Colar link" em vez disso.',
    backToSettings: "Voltar para Configurações",
    cameraPermissionTitle: "Permissão da câmera",
    cameraPermissionBody:
      "Permita acesso à câmera para escanear o QR code de pareamento do daemon.",
    grantPermission: "Conceder permissão",
    pairing: "Pareando...",
    unableToPair: "Não foi possível parear host",
    errorTitle: "Erro",
  },
  device: {
    loadingOffer: "Carregando oferta de pareamento...",
    failedToLoadOffer: "Falha ao carregar oferta de pareamento.",
    relayDisabled: "Relay não está ativado. Ative o relay para parear um dispositivo.",
    enableTitle: "Ativar o relay?",
    enableDescription:
      "O relay permite conectar este dispositivo de qualquer lugar. O tráfego de pareamento é criptografado de ponta a ponta.",
    relayDocs: "Como o relay funciona",
    relayDocsAccessibility: "Leia como o relay do Ait funciona",
    enableRelay: "Ativar relay",
    enablingRelay: "Ativando...",
    notNow: "Agora não",
    directConnectionHint:
      "Sem relay, conecte diretamente por TCP, Tailscale ou outra VPN. Nenhum código QR é criado.",
    updateRequired: "Atualize o host para ativar o relay pelo Ait Desktop.",
    unavailable: "Oferta de pareamento indisponível.",
    hint: "Escaneie este QR code com o Ait no seu celular ou copie o link abaixo.",
    securityWarning:
      "Trate este link de pareamento como uma senha. Qualquer pessoa com o link pode acessar este daemon.",
    qrUnavailable: "QR code indisponível.",
    qrAccessibility: "Código QR de pareamento",
    retry: "Tentar novamente",
    copy: "Copiar",
    copied: "Copiado",
  },
};
