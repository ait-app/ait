export const importSession = {
  chooseHostTitle: "Importar do host",
  title: "Importar sessão",
  searchPlaceholder: "Buscar sessões...",
  scope: {
    host: "Sessões em {{host}}",
    workspace: "Este workspace",
  },
  filters: {
    all: "Tudo",
    label: "Filtro: {{provider}}",
    title: "Filtrar por provedor",
  },
  status: {
    connectHost: "Conecte-se a um host para importar sessões",
    updateHost: "Atualize o host para importar sessões.",
    noProviders: "Nenhum provedor importável está ativado.",
    loading: "Carregando sessões recentes...",
    failedProvider: "Não foi possível carregar as sessões de {{provider}}",
    failedImport: "Não foi possível importar a sessão selecionada.",
    openImportedFailed: "A sessão foi importada, mas não foi possível abrir o projeto dela.",
    workingDirectoryUnavailable:
      "O diretório de trabalho da sessão não está disponível neste host: {{path}}. Restaure o diretório e tente importar novamente.",
  },
  actions: {
    refresh: "Atualizar sessões",
    showAll: "Mostrar tudo",
    loadMore: "Carregar mais",
  },
  preview: {
    untitledSession: "Sessão sem título",
    noPrompt: "Nenhuma prévia de prompt",
  },
  sessionPreview: {
    title: "Prévia da sessão",
    open: "Prévia do histórico da sessão (somente leitura)",
    activeNotice:
      "Esta sessão tem um turno não concluído. A prévia é somente leitura e não a retoma.",
    loadOlder: "Carregar mensagens mais antigas",
    loadingOlder: "Carregando mensagens mais antigas...",
    empty: "Não há histórico exibível nesta sessão.",
    failed: "Não foi possível ler o histórico da sessão.",
    retry: "Tentar novamente",
    you: "Você",
    assistant: "Assistente",
    compacted: "O contexto anterior foi compactado",
    toolRunning: "em execução",
    toolCompleted: "concluído",
    toolFailed: "falhou",
    toolCanceled: "cancelado",
  },
  empty: {
    noRecent: "Nenhuma sessão recente para importar.",
    noMatches: "Nenhuma sessão corresponde à sua busca.",
    alreadyImported: "Todas as sessões recentes já foram importadas.",
    noProviderSessions: "Nenhuma sessão de {{provider}} encontrada.",
  },
  row: {
    importing: "Importando...",
  },
};
