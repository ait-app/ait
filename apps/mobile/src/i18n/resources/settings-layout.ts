export const settingsLayoutAr = {
  openInSidePane: {
    title: "فتح الموضع",
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

export const settingsLayoutEs = {
  openInSidePane: {
    title: "Abrir ubicación",
    destinations: {
      main: "Panel principal",
      side: "Al lado",
      explorer: "Barra lateral del explorador",
    },
    sources: {
      explorerFiles: {
        label: "Seleccionar un archivo en el explorador",
        description: "Abrir junto a tu trabajo los archivos seleccionados en la barra lateral",
      },
      diffs: {
        label: "Abrir un diff",
        description:
          "Abrir junto a tu trabajo los diffs del explorador y de las conversaciones de agentes",
      },
      chatFiles: {
        label: "Abrir un archivo desde un chat de agente",
        description: "Abrir enlaces y archivos de llamadas de herramientas junto a la conversación",
      },
      diffFiles: {
        label: "Abrir un archivo desde Cambios",
        description: "Abrir al lado los archivos de origen seleccionados en un diff",
      },
      subagents: {
        label: "Abrir un subagente",
        description: "Abrir los subagentes junto a su agente padre",
      },
      pullRequests: {
        label: "Abrir un pull request desde Cambios",
        description: "Abrir los detalles del pull request junto a Cambios",
      },
    },
  },
} as const;

export const settingsLayoutFr = {
  openInSidePane: {
    title: "Ouvrir l’emplacement",
    destinations: {
      main: "Panneau principal",
      side: "Sur le côté",
      explorer: "Barre latérale de l’explorateur",
    },
    sources: {
      explorerFiles: {
        label: "Sélectionner un fichier dans l’explorateur",
        description: "Ouvrir à côté de votre travail les fichiers sélectionnés dans l’explorateur",
      },
      diffs: {
        label: "Ouvrir un diff",
        description:
          "Ouvrir les diffs de l’explorateur et des conversations d’agents à côté de votre travail",
      },
      chatFiles: {
        label: "Ouvrir un fichier depuis un chat d’agent",
        description:
          "Ouvrir les liens de fichiers et les fichiers d’appels d’outils à côté de la conversation",
      },
      diffFiles: {
        label: "Ouvrir un fichier depuis Modifications",
        description: "Ouvrir à côté les fichiers source sélectionnés dans un diff",
      },
      subagents: {
        label: "Ouvrir un sous-agent",
        description: "Ouvrir les sous-agents à côté de leur agent parent",
      },
      pullRequests: {
        label: "Ouvrir une pull request depuis Modifications",
        description: "Ouvrir les détails de la pull request à côté des modifications",
      },
    },
  },
} as const;

export const settingsLayoutJa = {
  openInSidePane: {
    title: "場所を開く",
    destinations: {
      main: "メインパネル",
      side: "サイド",
      explorer: "エクスプローラーのサイドバー",
    },
    sources: {
      explorerFiles: {
        label: "エクスプローラーでファイルを選択",
        description: "エクスプローラーのサイドバーで選択したファイルを作業の横に開く",
      },
      diffs: {
        label: "差分を開く",
        description: "エクスプローラーやエージェント会話の差分を作業の横に開く",
      },
      chatFiles: {
        label: "エージェントチャットからファイルを開く",
        description: "ファイルリンクやツール呼び出しのファイルを会話の横に開く",
      },
      diffFiles: {
        label: "変更からファイルを開く",
        description: "差分で選択したソースファイルを差分の横に開く",
      },
      subagents: {
        label: "サブエージェントを開く",
        description: "サブエージェントを親エージェントの横に開く",
      },
      pullRequests: {
        label: "変更からプルリクエストを開く",
        description: "プルリクエストの詳細を変更の横に開く",
      },
    },
  },
} as const;

export const settingsLayoutKo = {
  openInSidePane: {
    title: "위치 열기",
    destinations: { main: "기본 패널", side: "측면", explorer: "탐색기 사이드바" },
    sources: {
      explorerFiles: {
        label: "탐색기에서 파일 선택",
        description: "탐색기 사이드바에서 선택한 파일을 작업 옆에 열기",
      },
      diffs: { label: "diff 열기", description: "탐색기와 에이전트 대화의 diff를 작업 옆에 열기" },
      chatFiles: {
        label: "에이전트 채팅에서 파일 열기",
        description: "파일 링크와 도구 호출 파일을 대화 옆에 열기",
      },
      diffFiles: {
        label: "변경 사항에서 파일 열기",
        description: "diff에서 선택한 소스 파일을 옆에 열기",
      },
      subagents: {
        label: "하위 에이전트 열기",
        description: "하위 에이전트를 상위 에이전트 옆에 열기",
      },
      pullRequests: {
        label: "변경 사항에서 풀 리퀘스트 열기",
        description: "풀 리퀘스트 세부 정보를 변경 사항 옆에 열기",
      },
    },
  },
} as const;

export const settingsLayoutPtBR = {
  openInSidePane: {
    title: "Abrir local",
    destinations: {
      main: "Painel principal",
      side: "Ao lado",
      explorer: "Barra lateral do explorador",
    },
    sources: {
      explorerFiles: {
        label: "Selecionar um arquivo no explorador",
        description: "Abrir ao lado do trabalho os arquivos selecionados na barra lateral",
      },
      diffs: {
        label: "Abrir um diff",
        description: "Abrir diffs do explorador e de conversas de agentes ao lado do trabalho",
      },
      chatFiles: {
        label: "Abrir um arquivo de um chat de agente",
        description:
          "Abrir links de arquivos e arquivos de chamadas de ferramentas ao lado da conversa",
      },
      diffFiles: {
        label: "Abrir um arquivo de Alterações",
        description: "Abrir ao lado os arquivos de origem selecionados em um diff",
      },
      subagents: {
        label: "Abrir um subagente",
        description: "Abrir subagentes ao lado do agente pai",
      },
      pullRequests: {
        label: "Abrir um pull request de Alterações",
        description: "Abrir detalhes do pull request ao lado de Alterações",
      },
    },
  },
} as const;

export const settingsLayoutRu = {
  openInSidePane: {
    title: "Открывать расположение",
    destinations: { main: "Главная панель", side: "Сбоку", explorer: "Боковая панель проводника" },
    sources: {
      explorerFiles: {
        label: "Выбор файла в проводнике",
        description: "Открывать выбранные в проводнике файлы рядом с рабочей областью",
      },
      diffs: {
        label: "Открытие diff",
        description: "Открывать diff из проводника и бесед агентов рядом с рабочей областью",
      },
      chatFiles: {
        label: "Открытие файла из чата агента",
        description: "Открывать ссылки на файлы и файлы вызовов инструментов рядом с беседой",
      },
      diffFiles: {
        label: "Открытие файла из изменений",
        description: "Открывать исходные файлы, выбранные в diff, рядом с ним",
      },
      subagents: {
        label: "Открытие субагента",
        description: "Открывать субагентов рядом с родительским агентом",
      },
      pullRequests: {
        label: "Открытие pull request из изменений",
        description: "Открывать сведения о pull request рядом с изменениями",
      },
    },
  },
} as const;
