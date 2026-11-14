export const importSession = {
  chooseHostTitle: "الاستيراد من المضيف",
  title: "جلسة الاستيراد",
  searchPlaceholder: "البحث في الجلسات...",
  scope: {
    host: "الجلسات على {{host}}",
    workspace: "مساحة العمل هذه",
  },
  filters: {
    all: "الجميع",
  },
  status: {
    connectHost: "اتصل بمضيف لاستيراد الجلسات",
    updateHost: "قم بتحديث المضيف لاستيراد الجلسات.",
    noProviders: "لم يتم تمكين أي موفري خدمات قابلين للاستيراد.",
    loading: "جارٍ تحميل الجلسات الأخيرة...",
    failedProvider: "تعذر تحميل جلسات {{provider}}",
    failedImport: "تعذر استيراد الجلسة المحددة.",
    workingDirectoryUnavailable:
      "مجلد العمل للجلسة غير متاح على هذا المضيف: {{path}}. استعد المجلد وحاول الاستيراد مرة أخرى.",
  },
  actions: {
    refresh: "تحديث الجلسات",
    showAll: "عرض الكل",
    loadMore: "تحميل المزيد",
  },
  preview: {
    untitledSession: "جلسة بلا عنوان",
    noPrompt: "لا توجد معاينة سريعة",
  },
  empty: {
    noRecent: "لا توجد جلسات حديثة لاستيرادها.",
    noMatches: "لا توجد جلسات تطابق بحثك.",
    alreadyImported: "تم بالفعل استيراد كافة الجلسات الأخيرة.",
    noProviderSessions: "لم يتم العثور على جلسات{{provider}}.",
  },
  row: {
    importing: "جارٍ الاستيراد...",
  },
};
