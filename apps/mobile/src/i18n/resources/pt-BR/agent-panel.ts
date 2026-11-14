export const agentPanel = {
  states: {
    notFound: "Agente não encontrado",
    failedToLoad: "Falha ao carregar agente",
    reconnecting: "Reconectando ao host",
    updating: "Atualizando mensagens",
    timelineSyncFailed: "Não foi possível atualizar o histórico do agente.",
    timelineSyncRetrying: "Tentando novamente…",
    archivingTitle: "Arquivando agente...",
    archivingSubtitle: "Aguarde enquanto arquivamos este agente.",
  },
  unavailable: {
    selectedHost: "Host selecionado",
    unknownHost:
      "Não é possível abrir este agente porque {{serverLabel}} não está configurado neste dispositivo.",
    addHost:
      "Adicione o host nas Configurações ou abra um agente em um servidor configurado para continuar.",
    preparingSession: "Preparando sessão de {{serverLabel}}...",
    connecting: "Conectando a {{serverLabel}}...",
    showSoon: "Mostraremos este agente em instantes.",
    showWhenOnline: "Mostraremos este agente quando o host estiver online.",
    reconnectingTo: "Reconectando a {{serverLabel}}...",
    showAgainWhenReachable: "Mostraremos este agente novamente assim que o host estiver acessível.",
  },
  archived: {
    callout: "Este agente está arquivado",
    unarchive: "Desarquivar",
  },
};
