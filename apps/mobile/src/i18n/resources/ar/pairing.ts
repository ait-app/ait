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
    scanQr: {
      title: "مسح رمز QR",
      description: "اتصال التتابع المشفر.",
    },
    pasteLink: {
      title: "الصق رابط الاقتران",
      description: "اتصال التتابع المشفر.",
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
  link: {
    title: "الصق رابط الاقتران",
    helper: "الصق رابط الاقتران من الخادم الخاص بك.",
    label: "رابط الاقتران",
    errors: {
      required: "الصق رابط الاقتران (.../#offer=...)",
      missingOffer: "يجب أن يتضمن الرابط #offer=...",
      emptyOffer: "حمولة العرض فارغة",
      invalid: "رابط الاقتران غير صالح",
      unableToPair: "غير قادر على إقران المضيف",
    },
    alert: {
      failedTitle: "فشل الاقتران",
    },
    actions: {
      cancel: "يلغي",
      pair: "زوج",
      pairing: "الاقتران...",
    },
  },
  scan: {
    title: "مسح QR",
    webUnavailableTitle: "غير متوفر على شبكة الإنترنت",
    webUnavailableBody: 'فحص QR غير مدعوم في بناء الويب. استخدم "لصق الرابط" بدلاً من ذلك.',
    backToSettings: "العودة إلى الإعدادات",
    cameraPermissionTitle: "إذن الكاميرا",
    cameraPermissionBody: "اسمح بالوصول إلى الكاميرا لمسح رمز الاقتران QR من البرنامج الخفي لديك.",
    grantPermission: "منح الإذن",
    pairing: "الاقتران...",
    unableToPair: "غير قادر على إقران المضيف",
    errorTitle: "خطأ",
  },
  device: {
    loadingOffer: "جارٍ تحميل عرض الإقران...",
    failedToLoadOffer: "فشل تحميل عرض الاقتران.",
    relayDisabled: "لم يتم تمكين التتابع. تمكين التتابع لإقران جهاز.",
    enableTitle: "تمكين التتابع؟",
    enableDescription:
      "يتيح التتابع لهذا الجهاز الاتصال من أي مكان. حركة مرور الإقران مشفرة من طرف إلى طرف.",
    relayDocs: "كيفية عمل التتابع",
    relayDocsAccessibility: "اقرأ كيفية عمل تتابع Ait",
    enableRelay: "تمكين التتابع",
    enablingRelay: "جارٍ التمكين...",
    notNow: "ليس الآن",
    directConnectionHint:
      "بدون التتابع، اتصل مباشرة عبر TCP أو Tailscale أو شبكة VPN أخرى. لن يتم إنشاء رمز QR.",
    updateRequired: "حدّث المضيف لتمكين التتابع من Ait Desktop.",
    unavailable: "عرض الاقتران غير متاح.",
    hint: "قم بمسح رمز QR هذا باستخدام Ait على هاتفك، أو انسخ الرابط أدناه.",
    securityWarning:
      "تعامل مع رابط الاقتران هذا ككلمة مرور. يمكن لأي شخص يملكه الوصول إلى هذا البرنامج الخفي.",
    qrUnavailable: "رمز QR غير متاح.",
    qrAccessibility: "رمز QR للإقران",
    retry: "أعد المحاولة",
    copy: "ينسخ",
    copied: "منقول",
  },
};
