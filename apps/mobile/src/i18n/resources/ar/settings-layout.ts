export const settingsLayout = {
  openInSidePane: {
    title: "موضع الفتح",
    destinations: {
      main: "اللوحة الرئيسية",
      side: "على الجانب",
      explorer: "الشريط الجانبي للمستكشف",
    },
    sources: {
      explorerFiles: {
        label: "اختيار ملف في المستكشف",
        description: "فتح الملفات المحددة في الشريط الجانبي للمستكشف بجانب عملك",
      },
      diffs: {
        label: "فتح فرق",
        description: "فتح الفروقات من المستكشف ومحادثات الوكلاء بجانب عملك",
      },
      chatFiles: {
        label: "فتح ملف من محادثة وكيل",
        description: "فتح روابط الملفات وملفات استدعاء الأدوات بجانب المحادثة",
      },
      diffFiles: {
        label: "فتح ملف من التغييرات",
        description: "فتح الملفات المصدر المحددة من فرق بجانبه",
      },
      subagents: { label: "فتح وكيل فرعي", description: "فتح الوكلاء الفرعيين بجانب وكيلهم الأب" },
      pullRequests: {
        label: "فتح طلب سحب من التغييرات",
        description: "فتح تفاصيل طلب السحب بجانب التغييرات",
      },
    },
  },
} as const;
