export const pairing = {
  connectionMethods: {
    title: "Добавить подключение",
    direct: {
      title: "Прямое подключение",
      description: "Локальная сеть или VPN.",
    },
    remoteSsh: {
      title: "Удалённый SSH",
      description: "Подключение через SSH-клиент настольного приложения.",
    },
  },
  direct: {
    title: "Прямое подключение",
    helper: "Введите адрес сервера и токен доступа.",
    fields: {
      host: "Хост",
      port: "Порт",
      password: "Токен сервера",
      optional: "Обязательно",
      useSsl: "Использовать SSL",
      connectionUri: "URI подключения",
    },
    advanced: {
      label: "Дополнительные параметры",
      show: "Показать дополнительные параметры",
      hide: "Скрыть дополнительные параметры",
    },
    passwordVisibility: {
      show: "Показать пароль",
      hide: "Скрыть пароль",
    },
    actions: {
      cancel: "Отмена",
      connect: "Подключиться",
      connecting: "Подключение...",
    },
    errors: {
      hostRequired: "Требуется хост",
      invalidPort: "Порт должен быть в диапазоне от 1 до 65535.",
      invalidConnection: "Неверное соединение",
      failedTitle: "Соединение не удалось",
      failedToConnect: "Не удалось подключиться к {{endpoint}}.",
      noAdditionalDetails: "{{detail}} (дополнительные сведения отсутствуют)",
      timedOut: "Время подключения истекло. Проверьте адрес хоста, порт и сеть.",
      refused: "В подключении отказано. Демон запущен по этому адресу?",
      hostNotFound: "Хост не найден. Проверьте имя хоста и повторите попытку.",
      hostUnreachable: "Хост недоступен. Проверьте сеть и брандмауэр.",
      tlsError:
        "Ошибка TLS. Прямые соединения используют SSL только тогда, когда перед демоном находится терминатор TLS.",
      unableToConnect:
        "Не удалось подключиться. Проверьте адрес хоста и порт, а также доступность демона.",
      details: "Подробности: {{detail}}",
    },
  },
  remoteSsh: {
    title: "Удалённый SSH",
    helper: "Подключитесь к удалённому серверу Ait по SSH (порт сервера по умолчанию: 7316).",
    fields: {
      target: "Хост SSH",
    },
    actions: {
      cancel: "Отмена",
      connect: "Подключить",
      connecting: "Подключение...",
    },
    errors: {
      tokenRequired: "Введите токен удалённого сервера Ait (32–256 символов).",
      targetRequired: "Укажите хост SSH",
      invalidTarget: "Укажите корректный хост ssh://",
      failedToConnect: "Не удалось подключиться по SSH. {{detail}}",
    },
  },
};
