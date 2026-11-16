const labelDeletePluralForms = {
  deleteMessage_zero: "سيؤدي هذا إلى إزالة التسمية من {{count}} مساحة عمل على هذا المضيف.",
  deleteMessage_two: "سيؤدي هذا إلى إزالة التسمية من مساحتَي عمل ({{count}}) على هذا المضيف.",
  deleteMessage_few: "سيؤدي هذا إلى إزالة التسمية من {{count}} مساحات عمل على هذا المضيف.",
  deleteMessage_many: "سيؤدي هذا إلى إزالة التسمية من {{count}} مساحة عمل على هذا المضيف.",
};

const scheduleLimitPluralForms = {
  limit_zero: "{{count}} مرة تشغيل",
  limit_two: "مرتا تشغيل",
  limit_few: "{{count}} مرات تشغيل",
  limit_many: "{{count}} مرة تشغيل",
};

const scheduleCadencePluralForms = {
  milliseconds: {
    milliseconds_zero: "كل {{count}} مللي ثانية",
    milliseconds_two: "كل مللي ثانيتين",
    milliseconds_few: "كل {{count}} مللي ثوانٍ",
    milliseconds_many: "كل {{count}} مللي ثانية",
  },
  seconds: {
    seconds_zero: "كل {{count}} ثانية",
    seconds_two: "كل ثانيتين",
    seconds_few: "كل {{count}} ثوانٍ",
    seconds_many: "كل {{count}} ثانية",
  },
  minutes: {
    minutes_zero: "كل {{count}} دقيقة",
    minutes_two: "كل دقيقتين",
    minutes_few: "كل {{count}} دقائق",
    minutes_many: "كل {{count}} دقيقة",
  },
  hours: {
    hours_zero: "كل {{count}} ساعة",
    hours_two: "كل ساعتين",
    hours_few: "كل {{count}} ساعات",
    hours_many: "كل {{count}} ساعة",
  },
  days: {
    days_zero: "كل {{count}} يوم",
    days_two: "كل يومين",
    days_few: "كل {{count}} أيام",
    days_many: "كل {{count}} يومًا",
  },
};

export { labelDeletePluralForms, scheduleLimitPluralForms, scheduleCadencePluralForms };
