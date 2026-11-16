export const desktop = {
  windowControls: {
    minimize: "Minimizar janela",
    maximize: "Maximizar janela",
    restore: "Restaurar janela",
    close: "Fechar janela",
  },
  quitting: {
    title: "Saindo do Ait...",
    detail: "Parando o daemon local.",
  },
  daemon: {
    listen: {
      host: "Endereço de escuta",
      port: "Porta",
      savedHint: "Reinicie o daemon para aplicar as alterações.",
      override:
        "AIT_SERVER_LISTEN substitui esta configuração: {{address}}. Remova a variável de ambiente para usar o endereço salvo.",
      invalid: "Informe um endereço IPv4 ou IPv6 (ou localhost) e uma porta de 0 a 65535.",
    },
    lifecycle: {
      owned: "Iniciado por esta sessão do Desktop",
      attached: "Conectado a um daemon existente",
      ownedMessage: "Este daemon foi iniciado por esta sessão do Desktop.",
      attachedMessage: "Este daemon não foi iniciado por esta sessão do Desktop.",
      stopTitle: "Parar o daemon local?",
      stopMessage:
        "{{ownership}}\nDiretório pessoal: {{home}}\nPID do supervisor: {{pid}}\nOs trabalhos de agentes em execução serão interrompidos.",
      stop: "Parar daemon",
      stopping: "Parando…",
      stopFailed: "Não foi possível parar o daemon",
      pauseAttached:
        "Pausar o gerenciamento automático do daemon? O daemon conectado continuará em execução.",
      pause: "Pausar gerenciamento",
      workerUpdated: "Worker atualizado para {{version}}",
      supervisorRefresh:
        "O supervisor em execução mantém seu código original. O programa que o iniciou deve pará-lo e iniciá-lo novamente para atualizá-lo.",
    },
    title: "Daemon",
    status: {
      title: "Status",
      builtInOnly: "Apenas o daemon desktop integrado é exibido aqui",
      running: "em execução",
      notRunning: "não está em execução",
      pid: "PID {{pid}}",
    },
    management: {
      title: "Gerenciar daemon integrado",
      hint: "Permitir que o Ait inicie e pare o daemon integrado",
      pauseTitle: "Pausar daemon integrado",
      pauseMessage:
        "Isso vai parar o daemon integrado imediatamente. Agentes e terminais em execução conectados ao daemon integrado serão interrompidos.",
      pauseAndStop: "Pausar e parar",
      registrationFailed:
        "O daemon integrado foi iniciado, mas o Ait não conseguiu salvar a conexão localhost. Desative e reative o gerenciamento do daemon ou adicione localhost manualmente.",
      pausedStopFailed:
        "O gerenciamento do daemon integrado foi pausado, mas o Ait não conseguiu parar o daemon.",
      updateFailed: "Não foi possível atualizar o gerenciamento do daemon integrado.",
    },
    keepRunning: {
      title: "Manter daemon em execução ao sair",
      hint: "Agentes e terminais continuam em execução ao sair do Ait; a próxima inicialização se reconecta a eles.",
    },
    logs: {
      title: "Arquivo de log",
      modalTitle: "Logs do daemon",
      unavailable: "Caminho do log indisponível",
      empty: "(arquivo de log vazio)",
      copied: "Caminho do log copiado.",
      copyFailed: "Não foi possível copiar o caminho do log.",
      open: "Abrir logs",
      copyPath: "Copiar caminho",
    },
    fullStatus: {
      title: "Status completo",
      modalTitle: "Status do daemon",
      view: "Ver status",
      copied: "Status copiado para a área de transferência.",
      fetchFailed: "Falha ao buscar status do daemon: {{message}}",
    },
    versionMismatch:
      "As versões do app e do daemon não coincidem. Atualize ambos para a mesma versão para a melhor experiência.",
    loadFailed: "Não foi possível carregar o status do daemon desktop.",
  },
  updates: {
    status: {
      checking: "Verificando atualizações do app...",
      installing: "Instalando atualização do app...",
      upToDate: "O app está atualizado.",
      upToDateWithLastChecked: "Atualizado. Última verificação às {{time}}.",
      pending: "Avisaremos quando a atualização estiver pronta.",
      pendingWithLastChecked:
        "Avisaremos quando a atualização estiver pronta. Última verificação às {{time}}.",
      pendingWithVersion: "Atualização encontrada: {{version}}. Baixando...",
      pendingWithVersionAndLastChecked:
        "Atualização encontrada: {{version}}. Baixando... Última verificação às {{time}}.",
      availableWithVersion: "Atualização pronta: {{version}}",
      availableWithVersionAndLastChecked:
        "Atualização pronta: {{version}}. Última verificação às {{time}}.",
      available: "Há uma atualização do app pronta para instalar.",
      availableWithLastChecked:
        "Há uma atualização do app pronta para instalar. Última verificação às {{time}}.",
      installed: "Atualização do app instalada. Reinicialização obrigatória.",
      failed: "Falha ao atualizar o app.",
      idle: "O status de atualização ainda não foi verificado.",
    },
    installError: "Não foi possível instalar a atualização do app desktop.",
    callout: {
      installingTitle: "Instalando atualização",
      failedTitle: "Falha na atualização",
      availableTitle: "Atualização disponível",
      genericError: "Algo deu errado.",
      whatsNew: "Novidades",
      installingAction: "Instalando...",
      installAndRestart: "Instalar e reiniciar",
      installingDescription: "Instalando e reiniciando...",
      versionReady: "{{version}} está pronta para instalar.",
      newVersionReady: "Uma nova versão está pronta para instalar.",
      restartWarning:
        "Atualizar o app vai interromper agentes em execução e fechar sessões de terminal.",
    },
  },
  settings: {
    loadFailed: "Não foi possível carregar as configurações do desktop.",
    saveFailed: "Não foi possível salvar as configurações do desktop.",
  },
  rosetta: {
    title: "Baixar a build para Apple Silicon",
    runningIntel: "Você está executando a build Intel do Ait pelo Rosetta no Apple Silicon.",
    highCpu: "Isso causa alto uso de CPU. Baixe a build para Apple Silicon para corrigir.",
    download: "Baixar",
  },
  permissions: {
    notifications: {
      allowed: "As notificações são permitidas pelo sistema operacional.",
      denied: "As notificações estão negadas nas configurações do sistema.",
      notGranted: "As notificações ainda não foram concedidas.",
      webOnly: "O status de notificações desktop só está disponível no runtime web.",
      supported: "Notificações desktop são compatíveis.",
      unsupported: "Notificações desktop não são compatíveis nesta plataforma.",
      apiUnavailable: "A Web Notification API não está disponível neste ambiente.",
      requestsWebOnly: "Solicitações de notificação desktop só estão disponíveis no runtime web.",
      requestUnavailable: "Web Notification API requestPermission() está indisponível.",
      requestFailed: "Falha ao solicitar permissão de notificação: {{message}}",
      unexpectedState: "Estado inesperado de permissão de notificação: {{state}}",
    },
    microphone: {
      webOnly: "O status do microfone desktop só está disponível no runtime web.",
      navigatorUnavailable: "Navigator não está disponível neste ambiente.",
      granted: "O acesso ao microfone foi concedido.",
      denied: "O acesso ao microfone está negado nas configurações do sistema.",
      notGranted: "A permissão do microfone ainda não foi concedida.",
      unexpectedState: "Estado inesperado de permissão do microfone: {{state}}",
      statusApiUnavailable:
        "A API de status do microfone não está disponível neste runtime. Use Solicitar para verificar o acesso.",
      queryFailed: "Falha ao consultar o status do microfone: {{message}}",
      captureUnavailable: "A captura do microfone não está disponível neste ambiente.",
      permissionApiUnavailable:
        "A API de status de permissão não está disponível. Use Solicitar para verificar o acesso.",
      requestsWebOnly: "Solicitações de microfone desktop só estão disponíveis no runtime web.",
      captureApiUnavailable: "A API de captura do microfone não está disponível neste ambiente.",
      requestDenied: "A permissão do microfone foi negada pelo usuário ou pelo sistema.",
      noDevice: "Nenhum dispositivo de microfone foi encontrado.",
      requestFailed: "Falha ao solicitar permissão do microfone: {{message}}",
    },
    empty: {
      notifications: "O status das notificações ainda não foi verificado.",
      microphone: "O status do microfone ainda não foi verificado.",
    },
    testNotification: {
      title: "Teste de notificação do Ait",
      body: "Se você consegue ver isto, as notificações desktop funcionam.",
      notDelivered: "A notificação não foi entregue. Verifique Ajustes do Sistema > Notificações.",
      failed: "Falha ao enviar notificação.",
    },
  },
};
