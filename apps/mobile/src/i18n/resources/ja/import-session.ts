export const importSession = {
  chooseHostTitle: "ホストからインポート",
  title: "セッションをインポート",
  searchPlaceholder: "セッションを検索...",
  scope: {
    host: "{{host}} のセッション",
    workspace: "このワークスペース",
  },
  filters: {
    all: "すべて",
  },
  status: {
    connectHost: "セッションをインポートするにはホストに接続してください",
    updateHost: "セッションをインポートするにはホストを更新してください。",
    noProviders: "インポート可能なプロバイダーが有効になっていません。",
    loading: "最近のセッションを読み込み中...",
    failedProvider: "{{provider}} のセッションを読み込めませんでした",
    failedImport: "選択したセッションをインポートできませんでした。",
    workingDirectoryUnavailable:
      "セッションの作業ディレクトリがこのホストにありません: {{path}}。ディレクトリを復元して、もう一度インポートしてください。",
  },
  actions: {
    refresh: "セッションを更新",
    showAll: "すべて表示",
    loadMore: "さらに読み込む",
  },
  preview: {
    untitledSession: "無題のセッション",
    noPrompt: "プロンプトのプレビューなし",
  },
  empty: {
    noRecent: "インポートする最近のセッションがありません。",
    noMatches: "検索に一致するセッションがありません。",
    alreadyImported: "最近のセッションはすでにすべてインポートされています。",
    noProviderSessions: "{{provider}}のセッションが見つかりません。",
  },
  row: {
    importing: "インポート中...",
  },
};
