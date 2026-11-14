const labelDeletePluralForms = {
  deleteMessage_few: "Метка будет снята с {{count}} рабочих пространств на этом хосте.",
  deleteMessage_many: "Метка будет снята с {{count}} рабочих пространств на этом хосте.",
};

const scheduleLimitPluralForms = {
  limit_few: "{{count}} запуска",
  limit_many: "{{count}} запусков",
};

const scheduleCadencePluralForms = {
  milliseconds: {
    milliseconds_few: "Каждые {{count}} миллисекунды",
    milliseconds_many: "Каждые {{count}} миллисекунд",
  },
  seconds: {
    seconds_few: "Каждые {{count}} секунды",
    seconds_many: "Каждые {{count}} секунд",
  },
  minutes: {
    minutes_few: "Каждые {{count}} минуты",
    minutes_many: "Каждые {{count}} минут",
  },
  hours: {
    hours_few: "Каждые {{count}} часа",
    hours_many: "Каждые {{count}} часов",
  },
  days: {
    days_few: "Каждые {{count}} дня",
    days_many: "Каждые {{count}} дней",
  },
};

export { labelDeletePluralForms, scheduleLimitPluralForms, scheduleCadencePluralForms };
