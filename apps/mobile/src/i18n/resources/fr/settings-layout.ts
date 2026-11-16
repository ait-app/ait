export const settingsLayout = {
  openInSidePane: {
    title: "Ouvrir l’emplacement",
    destinations: {
      main: "Panneau principal",
      side: "Sur le côté",
      explorer: "Barre latérale de l’explorateur",
    },
    sources: {
      explorerFiles: {
        label: "Sélectionner un fichier dans l’explorateur",
        description: "Ouvrir à côté de votre travail les fichiers sélectionnés dans l’explorateur",
      },
      diffs: {
        label: "Ouvrir un diff",
        description:
          "Ouvrir les diffs de l’explorateur et des conversations d’agents à côté de votre travail",
      },
      chatFiles: {
        label: "Ouvrir un fichier depuis un chat d’agent",
        description:
          "Ouvrir les liens de fichiers et les fichiers d’appels d’outils à côté de la conversation",
      },
      diffFiles: {
        label: "Ouvrir un fichier depuis Modifications",
        description: "Ouvrir à côté les fichiers source sélectionnés dans un diff",
      },
      subagents: {
        label: "Ouvrir un sous-agent",
        description: "Ouvrir les sous-agents à côté de leur agent parent",
      },
      pullRequests: {
        label: "Ouvrir une pull request depuis Modifications",
        description: "Ouvrir les détails de la pull request à côté des modifications",
      },
    },
  },
} as const;
