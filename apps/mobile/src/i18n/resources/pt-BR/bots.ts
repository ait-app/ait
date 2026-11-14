export const bots = {
  modeCapWarning:
    "Os modos que permitem aos agentes executar comandos também permitem que eles leiam as credenciais do Ait deste computador e contornem os limites do Ait do Bot. Permita-os apenas para trabalho que você deixaria um agente fazer sem supervisão.",
  commandReachWarning:
    "O Bot pode enviar prompts a todos os agentes deste workspace, inclusive os que você iniciou, e alterar as configurações deles. Esses agentes mantêm as próprias permissões, então um prompt do Bot pode alcançar recursos além dos limites do próprio Bot.",
  terminalsWarning:
    "Os terminais executam comandos neste computador, então podem ler as credenciais do Ait dele e contornar os limites do Ait do Bot. Ative-os apenas para trabalho que você deixaria um agente fazer sem supervisão.",
  noteInstructionsInInput:
    "Este provedor não oferece suporte a um prompt de sistema atualizável, então as instruções integradas e a memória do Bot são adicionadas à entrada da conversa.",
  noteNoReadOnlyMode:
    "Este provedor não tem modo somente leitura, então o Bot é executado no modo padrão. Gravações que exigem aprovação são negadas automaticamente, mas ações que o provedor permite sem aprovação não são bloqueadas, incluindo ler as credenciais do Ait deste computador e contornar os limites do Ait do Bot.",
  noteToolsNeedApproval:
    "Este provedor não pode pré-aprovar ferramentas do Ait, então chamadas de ferramentas do Ait que exigem aprovação são negadas automaticamente. O Bot pode conseguir apenas conversar, sem iniciar nem gerenciar agentes.",
  noteNoAitTools:
    "Este provedor não oferece suporte à conexão com ferramentas do Ait, então o Bot só pode conversar.",
  permissions: "Permissões",
  permissionsHint: "As alterações de permissões entram em vigor no próximo turno do Bot.",
  groupObserve: "Observar (sempre ativo): ver agentes, atividade e provedores",
  groupAgents: "Agentes: criar agentes, enviar prompts e reconfigurá-los",
  groupStop: "Parar: cancelar e parar agentes",
  groupSchedules: "Agendamentos: criar e gerenciar agendamentos",
  groupScripts: "Scripts: executar scripts do workspace",
  groupTerminals: "Terminais: criar terminais e digitar neles",
  permissionRiskHint: "Can run commands.",
  modeCap: "Default agent mode",
  modeNotAllowed: "Not allowed",
  modeCapHint:
    "Choose the mode of the agents the Bot starts for each provider. The Bot can only use this mode; choose Not allowed to stop it using a provider.",
  providerUnavailable: "{{provider}} (indisponível)",
  readOnlyHint:
    "O Bot é executado no modo mais restrito que o provedor oferece (no Claude, o modo padrão, em que suas próprias regras de permissão continuam valendo) e deixa as alterações para os agentes que ele inicia.",
  title: "Bot",
  chat: "Chat",
  settings: "Configurações",
  provider: "Provedor",
  model: "Modelo",
  mode: "Modo de permissão",
  thinking: "Raciocínio",
  defaultOption: "Padrão do provedor",
  save: "Salvar",
  setupTitle: "Configurar o Bot do espaço de trabalho",
  setupHint:
    "O Bot gerencia este espaço de trabalho e continua em execução depois que você fecha esta aba.",
  setup: "Configurar Bot",
  clearMemory: "Limpar memória salva",
  notConfigured:
    "Este espaço de trabalho ainda não tem um Bot. Somente o proprietário do host pode configurá-lo.",
  disabled: "Ative os recursos experimentais em Configurações → Sobre para usar Bots.",
  unsupported: "Atualize este host para usar Bots.",
  disconnected: "Conecte-se a este host para usar Bots.",
  message: "Mensagem",
  cancel: "Cancelar turno",
  emptyChat: "Envie uma mensagem para iniciar uma conversa.",
  session: "Bot session",
  reset: "Nova conversa",
  resetConfirm:
    "Iniciar uma nova conversa? As mensagens existentes são mantidas e as configurações mais recentes do Bot serão aplicadas.",
  configurationHint:
    "As alterações de modelo se aplicam a novas conversas. Inicie uma nova conversa para usá-las aqui.",
  memory: "Memória",
  memoryHint:
    "Preferências que o Bot lê antes de cada resposta, como o idioma do título de PRs ou o estilo de resposta.",
  saveMemory: "Salvar memória",
  memoryConflict: "A memória foi alterada em outro lugar. Recarregue-a antes de salvar novamente.",
  resumeToEdit: "Retome o Bot para fazer alterações.",
  reload: "Recarregar",
  pause: "Pausar",
  remove: "Remover Bot",
  removeConfirm:
    "Remover este Bot? O histórico de conversas não ficará mais visível, a memória é mantida e as conexões do Telegram e do Feishu serão desconectadas.",
  paused: "Pausado",
  resume: "Retomar",
  directoryMissing:
    "O diretório do espaço de trabalho não foi encontrado, então o Bot está pausado. Restaure o diretório e retome.",
  requestFailed: "Falha na solicitação",
  tooLong: "As mensagens devem ter no máximo 16 KiB.",
  memoryTooLong: "A memória deve ter no máximo 32 KiB.",
  channels: "Canais",
  addChannel: "Adicionar canal",
  channelType: "Tipo de canal",
  telegram: "Telegram",
  feishu: "Feishu",
  lark: "Lark",
  tokenRef: "Referência do token do Bot",
  secretRef: "Referência do segredo do app",
  appId: "ID do app",
  allowedSenders: "Remetentes permitidos (separados por vírgula ou quebra de linha)",
  allowGroups: "Permitir chats em grupo",
  channelRefHint:
    "As credenciais vêm das variáveis de ambiente do processo que inicia o daemon. Informe apenas a referência da variável, como env:AIT_SERVER_CREDENTIAL_TELEGRAM, e pelo menos um remetente permitido.",
  channelEnabled: "Ativado",
  channelConnected: "Conectado",
  channelIdle: "Inativo",
  feishuScanHint:
    "Escaneie um QR code com o app Feishu. O Feishu cria um bot para você e o Ait o conecta; só você pode enviar mensagens até alterar isso.",
  feishuScanStart: "Mostrar QR code",
  feishuScanWaiting:
    "Escaneie este código com o app Feishu e aprove o novo bot. Mantenha esta página aberta até terminar.",
  feishuScanOpen: "Abrir no Feishu",
  feishuScanQrLabel: "QR code de configuração do Feishu",
  feishuScanRetry: "Tentar novamente",
  feishuScanDenied: "A solicitação foi recusada no Feishu.",
  feishuScanExpired: "Este QR code expirou. Comece de novo para obter outro.",
  feishuScanLark:
    "Este código QR é para o Feishu. Para uma conta Lark, selecione Lark e insira as credenciais do app manualmente.",
  feishuScanUnavailable:
    "Não foi possível concluir a configuração com o Feishu. Tente novamente ou informe as credenciais manualmente.",
  feishuManualEntry: "Informar credenciais manualmente",
  feishuScanInstead: "Escanear um QR code",
  feishuAppSecret: "App Secret",
  feishuAppSecretHint:
    "Cole o App Secret. O Ait o verifica com o {{platform}} e o guarda de forma privada neste host. Uma referência env:AIT_SERVER_CREDENTIAL_* também funciona.",
  feishuManualStep1: "Crie um app personalizado em {{developerUrl}} e ative o recurso de bot.",
  feishuManualStep2:
    "Em Permissões, adicione im:message:send_as_bot, im:message.p2p_msg:readonly, im:message.group_at_msg:readonly.",
  feishuManualStep3:
    "Copie aqui o App ID e o App Secret, adicione seu Open ID do {{platform}} (ou_...) como remetente permitido e adicione o canal.",
  feishuManualStep4:
    "Depois, em Eventos e callbacks do {{platform}}, escolha Conexão longa e assine im.message.receive_v1. O {{platform}} só consegue salvar isso quando o canal está conectado.",
  feishuManualStep5: "Publique uma versão do app para que possam enviar mensagens ao bot.",
  telegramGuideStep1: "Escaneie este código ou use o botão para abrir o BotFather no Telegram.",
  telegramGuideStep2: "Envie /newbot e escolha um nome e um nome de usuário terminado em bot.",
  telegramGuideStep3: "Copie o token enviado pelo BotFather e cole abaixo.",
  telegramBotFatherOpen: "Abrir BotFather",
  telegramBotFatherQrLabel: "QR code que abre o BotFather",
  telegramToken: "Token do bot",
  telegramTokenHint:
    "O Ait verifica o token com o Telegram e o guarda de forma privada neste host.",
  telegramContinue: "Continuar",
  telegramPairWaiting:
    "Escaneie este código ou abra o link e toque em Start na conversa com @{{bot}}. Assim você se torna o remetente permitido. Mantenha esta página aberta até terminar.",
  telegramPairOpen: "Abrir no Telegram",
  telegramPairQrLabel: "QR code que abre seu novo bot",
  telegramInvalidToken: "O Telegram recusou este token. Copie-o novamente do BotFather.",
  telegramAlreadyConnected:
    "Este bot já está conectado a um canal neste host. Exclua esse canal primeiro ou crie um novo bot.",
  telegramWebhookConflict:
    "Este bot tem um webhook configurado. Remova-o com deleteWebhook e tente novamente.",
  telegramPollingConflict:
    "Outro programa está recebendo as mensagens deste bot. Pare-o e tente novamente.",
  telegramExpired: "Este link expirou. Continue para obter outro.",
  telegramUnavailable:
    "Não foi possível conectar ao Telegram. Verifique a conexão deste host e tente novamente.",
  telegramUseEnvironment: "Usar uma variável de ambiente",
  telegramUseGuided: "Usar a configuração guiada",
  weixin: "WeChat",
  qq: "QQ",
  weixinScanHint:
    "Escaneie um código QR com o WeChat para conectar um bot. Só você pode enviar mensagens a ele, em conversas privadas.",
  weixinScanWaiting:
    "Escaneie este código com o WeChat e confirme no celular. Mantenha esta página aberta até terminar.",
  weixinScanOpen: "Abrir no WeChat",
  weixinScanQrLabel: "Código QR de configuração do WeChat",
  weixinScanUnavailable: "Não foi possível concluir a configuração com o WeChat. Tente novamente.",
  qqScanHint:
    "Escaneie um código QR com o QQ. O QQ cria um bot e o Ait o conecta; só você pode enviar mensagens até mudar isso.",
  qqScanWaiting:
    "Escaneie este código com o QQ e aprove o novo bot. Mantenha esta página aberta até terminar.",
  qqScanOpen: "Abrir no QQ",
  qqScanQrLabel: "Código QR de configuração do QQ",
  qqScanUnavailable:
    "Não foi possível concluir a configuração com o QQ. Tente novamente ou insira as credenciais manualmente.",
  scanAlreadyConnected:
    "Este bot já está conectado a um canal neste host. Exclua esse canal e tente novamente.",
  qqManualStep1: "Crie um bot em q.qq.com e copie aqui o App ID e o App Secret.",
  qqManualStep2:
    "Depois de adicionar o canal, obtenha um código de pareamento na linha dele e envie ao bot no QQ. Assim você se torna um remetente permitido.",
  qqAppSecretHint: "O Ait verifica o App Secret com o QQ e o armazena de forma privada neste host.",
  qqPairButton: "Obter código de pareamento",
  qqPairHint:
    "Código: {{code}}. Envie ao bot em uma conversa privada ou mencione o bot com ele em um grupo do QQ. Funciona uma vez e expira em 10 minutos.",
  imessage: "iMessage",
  webhook: "Webhook",
  webhookHint:
    "O Ait Hub cria um endereço HTTPS e um segredo para este Bot. Qualquer sistema que envie solicitações HTTP pode usá-los para mandar mensagens ao Bot e ler a resposta.",
  webhookCreate: "Criar webhook",
  webhookUrl: "Endereço",
  webhookSecret: "Segredo",
  webhookSecretOnce: "O segredo só aparece desta vez. Copie agora; se perder, crie um novo.",
  webhookExample: "Exemplo",
  webhookExampleHint:
    "A chamada responde com um endereço de status. Consulte-o com o mesmo segredo (adicione ?wait=30 para aguardar a resposta) ou inclua callback_url na chamada para receber a resposta nesse endereço. Chamadas com o mesmo thread continuam uma conversa.",
  webhookCopy: "Copiar",
  webhookCopied: "Copiado",
  webhookDone: "Concluir",
  webhookRotate: "Criar novo segredo",
  webhookRotateConfirm:
    "O segredo atual deixa de funcionar imediatamente. Quem o usa precisará do novo.",
  webhookCenterOffline:
    "Este host não está conectado ao Ait Hub. Entre e ative a conexão com o Hub, depois tente novamente.",
  webhookCenterUnsupported: "Este Ait Hub ainda não oferece suporte a webhooks.",
  webhookAutomationUnavailable: "A automação não está ativada neste Ait Hub.",
  webhookLimitReached:
    "Sua conta atingiu o limite de webhooks. Exclua um que não usa mais e tente novamente.",
  webhookCenterError: "O Ait Hub não conseguiu configurar o webhook. Tente novamente mais tarde.",
  imessageGuideStep1: "Crie um projeto Photon Beta em app.beta.photon.codes.",
  imessageGuideStep2:
    "Nesse projeto, crie uma chave de API com project:read, project:write, platforms:read e platforms:write.",
  imessageGuideStep3:
    "Informe abaixo o Project ID, a chave de API e seu próprio número de telefone ou e-mail do iMessage.",
  imessagePhotonOpen: "Abrir Photon",
  imessagePrivacyNotice:
    "As mensagens com este bot passam pelo serviço da Photon. Só há suporte a mensagens diretas, não a conversas em grupo.",
  imessageProjectId: "Project ID",
  imessageApiKey: "Chave de API",
  imessageApiKeyHint: "O Ait verifica a chave com a Photon e a guarda de forma privada neste host.",
  imessageHandle: "Seu número de telefone ou e-mail do iMessage",
  imessageHandleHint:
    "O endereço usado para enviar iMessages. Escreva telefones no formato internacional, como +14155550123.",
  imessageContinue: "Continuar",
  imessagePairPhoneNumber: "Enviar para",
  imessagePairCode: "Código",
  imessagePairWaiting:
    "Envie este código como iMessage, não como SMS, exatamente de {{handle}}. Escanear o QR code ou abrir o link preenche o código para você. Mantenha esta página aberta até terminar.",
  imessagePairExpires: "O código expira em {{minutes}} min.",
  imessagePairOpen: "Abrir no Mensagens",
  imessagePairQrLabel: "QR code que abre o Mensagens com o código preenchido",
  imessageInvalidKey:
    "A Photon recusou este Project ID ou esta chave de API. Verifique ambos e se a chave tem as quatro permissões.",
  imessageInvalidHandle:
    "Informe um telefone no formato internacional, como +14155550123, ou um e-mail do iMessage.",
  imessageDedicatedLine:
    "Este projeto Photon usa linhas dedicadas, que não têm suporte. Use um projeto com linhas compartilhadas.",
  imessageAlreadyConnected:
    "Este projeto Photon já está conectado a um canal neste host. Exclua esse canal primeiro ou use outro projeto.",
  imessageExpired: "Este código expirou. Continue para obter outro.",
  imessageUnavailable:
    "Não foi possível conectar à Photon. Verifique a conexão deste host e tente novamente.",
  deleteChannel: "Excluir canal",
  deleteChannelConfirm: "Excluir este canal? A conexão externa será desconectada.",
  notifications: {
    title: "Notificações",
    wokeBy: "Atualização do espaço de trabalho",
    silent: "Nada a relatar.",
    wakeScope: "Acordar o Bot para",
    wakeScopeOwn: "Apenas o próprio trabalho (padrão)",
    wakeScopeAll: "Tudo neste espaço de trabalho",
    wakeScopeHint:
      "O próprio trabalho abrange os agentes e agendamentos que o Bot iniciou. Com Tudo, qualquer outro evento de agente, agendamento ou terminal deste espaço de trabalho também inicia um turno do Bot, o que usa mais chamadas ao modelo.",
    recent: "Notificações recentes",
    empty: "Nenhuma notificação ainda.",
    state: {
      logged: "Fora do escopo de ativação",
      pending: "Aguardando para acordar o Bot",
      woke: "Acordou o Bot",
      other: "Outro status",
    },
    skipped: {
      over_budget: "Não entregue: limite de ativações por hora atingido",
      inbox_full: "Não entregue: notificações demais aguardando",
      paused: "Não entregue: o Bot estava pausado",
      observed: "Não repetida: o Bot já tinha verificado este agente nessa conversa",
      blocked: "Não entregue: essa conversa foi interrompida e precisa ser redefinida",
      reported: "Não repetida: o agente já havia relatado este resultado na tarefa",
      other: "Não acordou o Bot",
    },
    kind: {
      agent_finished: "Agente concluído",
      agent_failed: "Agente falhou",
      agent_permission: "Agente precisa de permissão",
      schedule_succeeded: "Execução agendada concluída",
      schedule_failed: "Execução agendada falhou",
      terminal_attention: "Terminal precisa de atenção",
      ticket_blocked: "Agente está bloqueado em uma tarefa",
      ticket_done: "Agente concluiu sua parte de uma tarefa",
      ticket_failed: "Agente não conseguiu concluir sua parte de uma tarefa",
      other: "Atualização do espaço de trabalho",
    },
  },
  tickets: {
    title: "Tarefas",
    pillOpen: "{{count}} em andamento",
    pillWaiting: "Aguardando você: {{count}}",
    pillLabel: "Tarefas do Bot: {{open}} em andamento, {{waiting}} aguardando você",
    empty:
      "Nenhuma tarefa em andamento. Uma tarefa aparece quando o Bot passa trabalho para um agente.",
    showClosed: "Mostrar fechadas recentemente",
    hideClosed: "Ocultar tarefas fechadas",
    closedHeading: "Fechadas recentemente",
    noClosed: "Nenhuma tarefa fechada.",
    agentCount: "Agentes: {{count}}",
    agentFallback: "Agente {{id}}",
    agents: "Agentes",
    noAgents: "Nenhum agente atribuído ainda.",
    request: "Pedido",
    history: "Histórico",
    noReports: "Nada relatado ainda.",
    outcome: "Resultado",
    noOutcome: "Nenhum resultado registrado.",
    outcomePlaceholder: "Resultado (opcional)",
    markDone: "Marcar como concluída",
    cancel: "Cancelar tarefa",
    cancelConfirm:
      "Cancelar esta tarefa? Os agentes dela continuam em execução; pare-os separadamente se precisar. Você pode reabrir a tarefa depois.",
    keep: "Manter tarefa",
    reopen: "Reabrir",
    openLimit: "Há tarefas demais em andamento. Feche as concluídas antes de reabrir esta.",
    conflict: "Esta tarefa mudou nesse meio-tempo. Confira o status e tente de novo.",
    linked: "Tarefa: {{title}}",
    untitled: "Tarefa sem título",
    movedTo: "agora {{status}}",
    status: {
      open: "Em andamento",
      waiting: "Aguardando você",
      done: "Concluída",
      failed: "Falhou",
      cancelled: "Cancelada",
      other: "Outro status",
    },
    waiting: {
      permission: "Um agente precisa da sua permissão",
      blocked: "Um agente está bloqueado e precisa da sua resposta",
      other: "Precisa da sua atenção",
    },
    source: {
      agent: "Agente",
      bot: "Bot",
      user: "Você",
      system: "Ait",
      other: "Outro",
    },
    report: {
      progress: "Progresso",
      blocked: "Bloqueado",
      done: "Concluiu sua parte",
      failed: "Desistiu da sua parte",
      agent_finished: "Agente concluiu um turno",
      agent_failed: "Agente falhou",
      agent_permission: "Agente precisa de permissão",
      note: "Nota",
      status: "Status alterado",
      other: "Atualização",
    },
  },
  status: {
    queued: "Na fila",
    sending: "Iniciando…",
    running: "Trabalhando…",
    needs_approval: "Aprovação necessária",
    completed: "Concluído",
    failed: "Falhou",
    uncertain: "Interrompido: inicie uma nova conversa para continuar",
    cancelled: "Cancelado",
  },
};
