export const settingsLayout = {
  openInSidePane: {
    title: "Открывать расположение",
    destinations: { main: "Главная панель", side: "Сбоку", explorer: "Боковая панель проводника" },
    sources: {
      explorerFiles: {
        label: "Выбор файла в проводнике",
        description: "Открывать выбранные в проводнике файлы рядом с рабочей областью",
      },
      diffs: {
        label: "Открытие diff",
        description: "Открывать diff из проводника и бесед агентов рядом с рабочей областью",
      },
      chatFiles: {
        label: "Открытие файла из чата агента",
        description: "Открывать ссылки на файлы и файлы вызовов инструментов рядом с беседой",
      },
      diffFiles: {
        label: "Открытие файла из изменений",
        description: "Открывать исходные файлы, выбранные в diff, рядом с ним",
      },
      subagents: {
        label: "Открытие субагента",
        description: "Открывать субагентов рядом с родительским агентом",
      },
      pullRequests: {
        label: "Открытие pull request из изменений",
        description: "Открывать сведения о pull request рядом с изменениями",
      },
    },
  },
} as const;
