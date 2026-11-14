export const importSession = {
  chooseHostTitle: "Импорт с хоста",
  title: "Импортировать сессию",
  searchPlaceholder: "Поиск сессий...",
  scope: {
    host: "Сессии на {{host}}",
    workspace: "Это рабочее пространство",
  },
  filters: {
    all: "Все",
  },
  status: {
    connectHost: "Подключитесь к хосту, чтобы импортировать сессии",
    updateHost: "Обновите хост, чтобы импортировать сессии.",
    noProviders: "Нет включённых провайдеров с поддержкой импорта.",
    loading: "Загрузка недавних сессий...",
    failedProvider: "Не удалось загрузить сессии провайдера {{provider}}",
    failedImport: "Не удалось импортировать выбранную сессию.",
    workingDirectoryUnavailable:
      "Рабочая папка сессии недоступна на этом хосте: {{path}}. Восстановите папку и повторите импорт.",
  },
  actions: {
    refresh: "Обновить список сессий",
    showAll: "Показать все",
    loadMore: "Загрузить ещё",
  },
  preview: {
    untitledSession: "Сессия без названия",
    noPrompt: "Нет промпта для предварительного просмотра",
  },
  empty: {
    noRecent: "Нет недавних сессий для импорта.",
    noMatches: "Нет сессий, соответствующих запросу.",
    alreadyImported: "Все недавние сессии уже импортированы.",
    noProviderSessions: "Сессии провайдера {{provider}} не найдены.",
  },
  row: {
    importing: "Импортируется...",
  },
};
