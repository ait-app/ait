export const desktop = {
  windowControls: {
    minimize: "ウィンドウを最小化",
    maximize: "ウィンドウを最大化",
    restore: "ウィンドウを元に戻す",
    close: "ウィンドウを閉じる",
  },
  quitting: {
    title: "Aitを終了中...",
    detail: "ローカルデーモンを停止中。",
  },
  daemon: {
    listen: {
      host: "待ち受けアドレス",
      port: "ポート",
      savedHint: "変更を適用するにはデーモンを再起動してください。",
      override:
        "AIT_SERVER_LISTEN がこの設定を上書きします: {{address}}。保存したアドレスを使うには環境変数を削除してください。",
      invalid:
        "IPv4 または IPv6 アドレス（localhost を含む）と 0～65535 のポートを入力してください。",
    },
    lifecycle: {
      owned: "このデスクトップセッションが起動",
      attached: "既存のデーモンに接続中",
      ownedMessage: "このデーモンは、このデスクトップセッションが起動しました。",
      attachedMessage: "このデーモンは、このデスクトップセッションが起動したものではありません。",
      stopTitle: "ローカルデーモンを停止しますか？",
      stopMessage:
        "{{ownership}}\nホーム：{{home}}\nスーパーバイザー PID：{{pid}}\n実行中のエージェントのタスクは中断されます。",
      stop: "デーモンを停止",
      stopping: "停止中…",
      stopFailed: "デーモンを停止できません",
      pauseAttached: "デーモンの自動管理を一時停止しますか？接続中のデーモンは実行を継続します。",
      pause: "管理を一時停止",
      workerUpdated: "ワーカーを {{version}} に更新しました",
      supervisorRefresh:
        "実行中のスーパーバイザーは元のコードを保持しています。更新するには、起動元のプログラムで停止して再起動する必要があります。",
    },
    title: "デーモン",
    status: {
      title: "ステータス",
      builtInOnly: "組み込みデスクトップデーモンのみここに表示されます",
      running: "実行中",
      notRunning: "実行していません",
      pid: "PID {{pid}}",
    },
    management: {
      title: "組み込みデーモンを管理",
      hint: "Aitが組み込みデーモンを起動・停止できるようにする",
      pauseTitle: "組み込みデーモンを一時停止",
      pauseMessage:
        "これにより組み込みデーモンが即座に停止します。組み込みデーモンに接続されている実行中のエージェントとターミナルが停止されます。",
      pauseAndStop: "一時停止して停止",
      registrationFailed:
        "組み込みデーモンは起動しましたが、Aitがlocalhostの接続を保存できませんでした。デーモン管理をオフにしてから再度オンにするか、localhostを手動で追加してください。",
      pausedStopFailed:
        "組み込みデーモン管理は一時停止されましたが、Aitがデーモンを停止できませんでした。",
      updateFailed: "組み込みデーモン管理を更新できません。",
    },
    keepRunning: {
      title: "終了後もデーモンを実行し続ける",
      hint: "Aitを終了してもエージェントとターミナルは実行し続け、次回起動時に再接続します。",
    },
    logs: {
      title: "ログファイル",
      modalTitle: "デーモンログ",
      unavailable: "ログパスが利用できません",
      empty: "（ログファイルは空です）",
      copied: "ログパスをコピーしました。",
      copyFailed: "ログパスをコピーできません。",
      open: "ログを開く",
      copyPath: "パスをコピー",
    },
    fullStatus: {
      title: "詳細ステータス",
      modalTitle: "デーモンのステータス",
      view: "ステータスを表示",
      copied: "ステータスをクリップボードにコピーしました。",
      fetchFailed: "デーモンのステータスの取得に失敗しました: {{message}}",
    },
    versionMismatch:
      "アプリとデーモンのバージョンが一致しません。最良の体験のために両方を同じバージョンに更新してください。",
    loadFailed: "デスクトップデーモンのステータスを読み込めません。",
  },
  updates: {
    status: {
      checking: "アプリの更新を確認中...",
      installing: "アプリの更新をインストール中...",
      upToDate: "アプリは最新です。",
      upToDateWithLastChecked: "最新の状態です。最終確認: {{time}}。",
      pending: "更新の準備ができたらお知らせします。",
      pendingWithLastChecked: "更新の準備ができたらお知らせします。最終確認: {{time}}。",
      pendingWithVersion: "更新が見つかりました: {{version}}。ダウンロード中...",
      pendingWithVersionAndLastChecked:
        "更新が見つかりました: {{version}}。ダウンロード中... 最終確認: {{time}}。",
      availableWithVersion: "更新の準備ができました: {{version}}",
      availableWithVersionAndLastChecked:
        "更新の準備ができました: {{version}}。最終確認: {{time}}。",
      available: "アプリの更新をインストールできます。",
      availableWithLastChecked: "アプリの更新をインストールできます。最終確認: {{time}}。",
      installed: "アプリの更新がインストールされました。再起動が必要です。",
      failed: "アプリの更新に失敗しました。",
      idle: "更新ステータスはまだ確認されていません。",
    },
    installError: "デスクトップアプリの更新をインストールできません。",
    callout: {
      installingTitle: "更新をインストール中",
      failedTitle: "更新に失敗しました",
      availableTitle: "更新が利用可能",
      genericError: "問題が発生しました。",
      whatsNew: "新機能",
      installingAction: "インストール中...",
      installAndRestart: "インストールして再起動",
      installingDescription: "インストールして再起動中...",
      versionReady: "{{version}}のインストール準備ができました。",
      newVersionReady: "新しいバージョンのインストール準備ができました。",
      restartWarning:
        "アプリを更新すると、実行中のエージェントが停止しターミナルセッションが閉じられます。",
    },
  },
  settings: {
    loadFailed: "デスクトップ設定を読み込めません。",
    saveFailed: "デスクトップ設定を保存できません。",
  },
  rosetta: {
    title: "Apple Siliconビルドをダウンロード",
    runningIntel: "Apple Silicon上のRosettaでAitのIntelビルドを実行しています。",
    highCpu:
      "これにより高いCPU使用率が発生します。修正するにはApple Siliconビルドをダウンロードしてください。",
    download: "ダウンロード",
  },
  permissions: {
    notifications: {
      allowed: "通知はOSによって許可されています。",
      denied: "通知はシステム設定で拒否されています。",
      notGranted: "通知はまだ許可されていません。",
      webOnly: "デスクトップ通知のステータスはWebランタイムでのみ利用できます。",
      supported: "デスクトップ通知はサポートされています。",
      unsupported: "デスクトップ通知はこのプラットフォームではサポートされていません。",
      apiUnavailable: "この環境ではWeb Notification APIは利用できません。",
      requestsWebOnly: "デスクトップ通知のリクエストはWebランタイムでのみ利用できます。",
      requestUnavailable: "Web Notification API の requestPermission() は利用できません。",
      requestFailed: "通知の権限リクエストに失敗しました: {{message}}",
      unexpectedState: "予期しない通知の権限状態: {{state}}",
    },
    microphone: {
      webOnly: "デスクトップマイクのステータスはWebランタイムでのみ利用できます。",
      navigatorUnavailable: "この環境ではNavigatorは利用できません。",
      granted: "マイクへのアクセスが許可されています。",
      denied: "マイクへのアクセスはシステム設定で拒否されています。",
      notGranted: "マイクの権限はまだ許可されていません。",
      unexpectedState: "予期しないマイクの権限状態: {{state}}",
      statusApiUnavailable:
        "マイクステータスAPIはこのランタイムでは利用できません。アクセス確認は［許可を求める］から行ってください。",
      queryFailed: "マイクのステータス確認に失敗しました: {{message}}",
      captureUnavailable: "この環境ではマイクのキャプチャは利用できません。",
      permissionApiUnavailable:
        "権限ステータスAPIは利用できません。アクセス確認は［許可を求める］から行ってください。",
      requestsWebOnly: "デスクトップマイクのリクエストはWebランタイムでのみ利用できます。",
      captureApiUnavailable: "この環境ではマイクキャプチャAPIは利用できません。",
      requestDenied: "マイクの権限はユーザーまたはシステムによって拒否されました。",
      noDevice: "マイクデバイスが見つかりませんでした。",
      requestFailed: "マイクの権限リクエストに失敗しました: {{message}}",
    },
    empty: {
      notifications: "通知のステータスはまだ確認されていません。",
      microphone: "マイクのステータスはまだ確認されていません。",
    },
    testNotification: {
      title: "Ait通知テスト",
      body: "これが見えれば、デスクトップ通知は機能しています。",
      notDelivered: "通知が届きませんでした。システム設定 > 通知を確認してください。",
      failed: "通知の送信に失敗しました。",
    },
  },
  integrations: {
    cli: {
      statusFailed: "CLIのインストール状態を確認できません。",
      installFailed: "Ait CLIをインストールできません。",
    },
  },
};
