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
  },
  direct: {
    title: "Conexão direta",
    helper: "Informe o endereço do servidor e o token de acesso.",
    fields: {
      host: "Host",
      port: "Porta",
      password: "Token do servidor",
      optional: "Obrigatório",
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
    helper: "Conecte-se ao servidor Ait remoto via SSH (porta padrão do servidor: 7316).",
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
};
