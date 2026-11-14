export const settingsLayout = {
  openInSidePane: {
    title: "Abrir local",
    destinations: {
      main: "Painel principal",
      side: "Ao lado",
      explorer: "Barra lateral do explorador",
    },
    sources: {
      explorerFiles: {
        label: "Selecionar um arquivo no explorador",
        description: "Abrir ao lado do trabalho os arquivos selecionados na barra lateral",
      },
      diffs: {
        label: "Abrir um diff",
        description: "Abrir diffs do explorador e de conversas de agentes ao lado do trabalho",
      },
      chatFiles: {
        label: "Abrir um arquivo de um chat de agente",
        description:
          "Abrir links de arquivos e arquivos de chamadas de ferramentas ao lado da conversa",
      },
      diffFiles: {
        label: "Abrir um arquivo de Alterações",
        description: "Abrir ao lado os arquivos de origem selecionados em um diff",
      },
      subagents: {
        label: "Abrir um subagente",
        description: "Abrir subagentes ao lado do agente pai",
      },
      pullRequests: {
        label: "Abrir um pull request de Alterações",
        description: "Abrir detalhes do pull request ao lado de Alterações",
      },
    },
  },
} as const;
