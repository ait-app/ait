export const newWorkspace = {
  title: "Новое рабочее пространство",
  create: "Создать",
  isolation: {
    local: "Локально",
    worktree: "Новый worktree",
    sandboxedWorktree: "Изолированный worktree",
    label: "Изоляция",
  },
  errors: {
    hostDisconnected: "Хост не подключён",
    createWorktreeFailed: "Не удалось создать worktree.",
    composerStateRequired: "Редактор сообщения ещё не готов.",
    selectModel: "Выберите модель",
  },
  tooltips: {
    project: "Выберите проект",
    host: "Выберите хост",
    isolation: "Выберите уровень изоляции",
    startingRef: "Выберите, с чего начать",
    launch: "Выберите, что запустить",
  },
  refPicker: {
    startingRef: "Исходная точка",
    intoBase: "в ветку {{baseRef}}",
    searching: "Идет поиск...",
    noMatchingRefs: "Нет подходящих веток или PR.",
    searchPlaceholder: "Поиск веток и PR",
    title: "Начать с",
  },
  launch: {
    title: "Что запустить",
    chat: "Чат",
    terminal: "Терминал",
    manageProfiles: "Управление профилями терминала",
    submit: "Запустить",
    promptPlaceholder: "Промпт для {{name}}",
    commandPlaceholder:
      "Введите команду или оставьте поле пустым, чтобы открыть терминал без запуска команды",
  },
};
