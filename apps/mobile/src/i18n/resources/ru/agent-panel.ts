export const agentPanel = {
  states: {
    notFound: "Агент не найден",
    failedToLoad: "Не удалось загрузить агента",
    reconnecting: "Повторное подключение к хосту",
    updating: "Обновление сообщений",
    timelineSyncFailed: "Не удалось обновить историю агента.",
    timelineSyncRetrying: "Повторная попытка…",
    archivingTitle: "Агент перемещается в архив...",
    archivingSubtitle: "Подождите, пока агент будет перемещён в архив.",
  },
  unavailable: {
    selectedHost: "Выбранный хост",
    unknownHost:
      "Невозможно открыть этого агента, поскольку {{serverLabel}} не настроен на этом устройстве.",
    addHost:
      "Добавьте хост в настройках или откройте агента на настроенном хосте, чтобы продолжить.",
    preparingSession: "Подготовка сессии на хосте {{serverLabel}}...",
    connecting: "Подключение к {{serverLabel}}...",
    showSoon: "Агент скоро появится.",
    showWhenOnline: "Агент появится, как только хост будет в сети.",
    reconnectingTo: "Повторное подключение к {{serverLabel}}...",
    showAgainWhenReachable: "Агент снова появится, как только хост станет доступен.",
  },
  archived: {
    callout: "Этот агент находится в архиве",
    unarchive: "Разархивировать",
  },
};
