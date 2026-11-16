export const pairing = {
  connectionMethods: {
    title: "إضافة اتصال",
    direct: {
      title: "اتصال مباشر",
      description: "الشبكة المحلية أو VPN.",
    },
    remoteSsh: {
      title: "SSH عن بُعد",
      description: "الاتصال عبر عميل SSH لسطح المكتب.",
    },
  },
  direct: {
    title: "اتصال مباشر",
    helper: "أدخل عنوان الخادم ورمز الوصول.",
    fields: {
      host: "المضيف",
      port: "المنفذ",
      password: "رمز الخادم",
      optional: "مطلوب",
      useSsl: "استخدم طبقة المقابس الآمنة",
      connectionUri: "معرف URI للاتصال",
    },
    advanced: {
      label: "متقدم",
      show: "عرض متقدم",
      hide: "إخفاء المتقدمة",
    },
    passwordVisibility: {
      show: "إظهار كلمة المرور",
      hide: "إخفاء كلمة المرور",
    },
    actions: {
      cancel: "إلغاء",
      connect: "اتصال",
      connecting: "جارٍ الاتصال...",
    },
    errors: {
      hostRequired: "المضيف مطلوب",
      invalidPort: "يجب أن يكون المنفذ بين 1 و65535",
      invalidConnection: "اتصال غير صالح",
      failedTitle: "فشل الاتصال",
      failedToConnect: "فشلنا في الاتصال بـ{{endpoint}}.",
      noAdditionalDetails: "{{detail}} (لم يتم تقديم تفاصيل إضافية)",
      timedOut: "انتهت مهلة الاتصال. تحقق من المضيف/المنفذ وشبكتك.",
      refused: "تم رفض الاتصال. هل الخادم يعمل على هذا العنوان؟",
      hostNotFound: "لم يتم العثور على المضيف. تحقق من اسم المضيف وحاول مرة أخرى.",
      hostUnreachable: "تعذر الوصول إلى المضيف. تحقق من شبكتك وجدار الحماية.",
      tlsError:
        "خطأ TLS. تستخدم الاتصالات المباشرة SSL فقط عندما توجد نقطة إنهاء TLS أمام البرنامج الخفي.",
      unableToConnect: "تعذر الاتصال. تحقق من المضيف/المنفذ ومن إمكانية الوصول إلى البرنامج الخفي.",
      details: "التفاصيل: {{detail}}",
    },
  },
  remoteSsh: {
    title: "SSH عن بُعد",
    helper: "الاتصال بخادم Ait البعيد عبر SSH (منفذ الخادم الافتراضي: 7316).",
    fields: {
      target: "مضيف SSH",
    },
    actions: {
      cancel: "إلغاء",
      connect: "اتصال",
      connecting: "جارٍ الاتصال...",
    },
    errors: {
      tokenRequired: "أدخل رمز خادم Ait البعيد (32–256 حرفًا).",
      targetRequired: "مضيف SSH مطلوب",
      invalidTarget: "أدخل مضيف ssh:// صالحًا",
      failedToConnect: "تعذر الاتصال عبر SSH. {{detail}}",
    },
  },
};
