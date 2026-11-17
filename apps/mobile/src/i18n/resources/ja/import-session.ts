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
    label: "フィルター: {{provider}}",
    title: "プロバイダーで絞り込む",
  },
  status: {
    connectHost: "セッションをインポートするにはホストに接続してください",
    updateHost: "セッションをインポートするにはホストを更新してください。",
    noProviders: "インポート可能なプロバイダーが有効になっていません。",
    loading: "最近のセッションを読み込み中...",
    failedProvider: "{{provider}} のセッションを読み込めませんでした",
    failedImport: "選択したセッションをインポートできませんでした。",
    openImportedFailed: "セッションはインポートされましたが、プロジェクトを開けませんでした。",
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
  sessionPreview: {
    title: "セッションプレビュー",
    open: "セッション履歴をプレビュー（読み取り専用）",
    activeNotice:
      "このセッションには未完了のターンがあります。プレビューは読み取り専用で、再開しません。",
    loadOlder: "以前のメッセージを読み込む",
    loadingOlder: "以前のメッセージを読み込み中...",
    empty: "このセッションに表示可能な履歴はありません。",
    failed: "セッション履歴を読み取れませんでした。",
    retry: "再試行",
    you: "あなた",
    assistant: "アシスタント",
    compacted: "以前のコンテキストは圧縮されました",
    toolRunning: "実行中",
    toolCompleted: "完了",
    toolFailed: "失敗",
    toolCanceled: "キャンセル済み",
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
