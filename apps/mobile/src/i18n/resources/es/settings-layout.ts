export const settingsLayout = {
  openInSidePane: {
    title: "Abrir ubicación",
    destinations: {
      main: "Panel principal",
      side: "Al lado",
      explorer: "Barra lateral del explorador",
    },
    sources: {
      explorerFiles: {
        label: "Seleccionar un archivo en el explorador",
        description: "Abrir junto a tu trabajo los archivos seleccionados en la barra lateral",
      },
      diffs: {
        label: "Abrir un diff",
        description:
          "Abrir junto a tu trabajo los diffs del explorador y de las conversaciones de agentes",
      },
      chatFiles: {
        label: "Abrir un archivo desde un chat de agente",
        description: "Abrir enlaces y archivos de llamadas de herramientas junto a la conversación",
      },
      diffFiles: {
        label: "Abrir un archivo desde Cambios",
        description: "Abrir al lado los archivos de origen seleccionados en un diff",
      },
      subagents: {
        label: "Abrir un subagente",
        description: "Abrir los subagentes junto a su agente padre",
      },
      pullRequests: {
        label: "Abrir un pull request desde Cambios",
        description: "Abrir los detalles del pull request junto a Cambios",
      },
    },
  },
} as const;
