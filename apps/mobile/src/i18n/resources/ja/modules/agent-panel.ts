export const agentPanel = {
  states: {
    notFound: "エージェントが見つかりません",
    failedToLoad: "エージェントの読み込みに失敗しました",
    reconnecting: "ホストに再接続中",
    updating: "メッセージを更新中",
    timelineSyncFailed: "エージェントの履歴を更新できませんでした。",
    timelineSyncRetrying: "再試行しています…",
    archivingTitle: "エージェントをアーカイブ中...",
    archivingSubtitle: "このエージェントをアーカイブするまでお待ちください。",
  },
  unavailable: {
    selectedHost: "選択中のホスト",
    unknownHost:
      "{{serverLabel}}がこのデバイスで設定されていないため、このエージェントを開けません。",
    addHost:
      "続行するには設定でホストを追加するか、設定済みのサーバーでエージェントを開いてください。",
    preparingSession: "{{serverLabel}}セッションを準備中...",
    connecting: "{{serverLabel}}に接続中...",
    showSoon: "このエージェントはすぐに表示されます。",
    showWhenOnline: "ホストがオンラインになったらこのエージェントを表示します。",
    reconnectingTo: "{{serverLabel}}に再接続中...",
    showAgainWhenReachable: "ホストに到達できるようになり次第、このエージェントを再び表示します。",
  },
  archived: {
    callout: "このエージェントはアーカイブされています",
    unarchive: "アーカイブ解除",
  },
};
