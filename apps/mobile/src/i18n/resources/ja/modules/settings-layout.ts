export const settingsLayout = {
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
