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
    helper: "أدخل عنوان خادم Ait.",
    fields: {
      host: "Host",
      port: "ميناء",
      password: "كلمة المرور",
      optional: "خياري",
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
      cancel: "يلغي",
      connect: "يتصل",
      connecting: "جارٍ الاتصال...",
    },
    errors: {
      hostRequired: "مطلوب Host",
      invalidPort: "يجب أن يكون المنفذ بين 1 و65535",
      invalidConnection: "اتصال غير صالح",
      failedTitle: "فشل الاتصال",
      failedToConnect: "فشلنا في الاتصال بـ{{endpoint}}.",
      noAdditionalDetails: "{{detail}}(لم يتم تقديم تفاصيل إضافية)",
      timedOut: "انتهت مهلة الاتصال. تحقق من المضيف /port وشبكتك.",
      refused: "رفض اتصال. هل الخادم يعمل على هذا العنوان؟",
      hostNotFound: "لم يتم العثور على Host. تحقق من اسم المضيف وحاول مرة أخرى.",
      hostUnreachable: "Host غير قابل للوصول. تحقق من شبكتك وجدار الحماية.",
      tlsError: "خطأ TLS. تستخدم الاتصالات المباشرة SSL فقط عندما يكون فاصل TLS أمام البرنامج الخفي.",
      unableToConnect:
        "غير قادر على الاتصال. تحقق من المضيف /port ومن إمكانية الوصول إلى البرنامج الخفي.",
      details: "التفاصيل:{{detail}}",
    },
  },
  remoteSsh: {
    title: "SSH عن بُعد",
    helper: "الاتصال بخادم Ait يعمل على المضيف البعيد.",
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
