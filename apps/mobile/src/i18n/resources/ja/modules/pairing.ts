export const pairing = {
  connectionMethods: {
    title: "接続を追加",
    direct: {
      title: "直接接続",
      description: "ローカルネットワークまたはVPN。",
    },
    remoteSsh: {
      title: "リモート SSH",
      description: "デスクトップの SSH クライアント経由で接続します。",
    },
    scanQr: {
      title: "QRコードをスキャン",
      description: "暗号化されたリレー接続。",
    },
    pasteLink: {
      title: "ペアリングリンクを貼り付け",
      description: "暗号化されたリレー接続。",
    },
  },
  direct: {
    title: "直接接続",
    helper: "Aitサーバーのアドレスを入力してください。",
    fields: {
      host: "ホスト",
      port: "ポート",
      password: "パスワード",
      optional: "任意",
      useSsl: "SSLを使用",
      connectionUri: "接続URI",
    },
    advanced: {
      label: "詳細設定",
      show: "詳細設定を表示",
      hide: "詳細設定を非表示",
    },
    passwordVisibility: {
      show: "パスワードを表示",
      hide: "パスワードを非表示",
    },
    actions: {
      cancel: "キャンセル",
      connect: "接続",
      connecting: "接続中...",
    },
    errors: {
      hostRequired: "ホストは必須です",
      invalidPort: "ポートは1から65535の間である必要があります",
      invalidConnection: "無効な接続",
      failedTitle: "接続に失敗しました",
      failedToConnect: "{{endpoint}}への接続に失敗しました。",
      noAdditionalDetails: "{{detail}}（追加の詳細は提供されていません）",
      timedOut: "接続がタイムアウトしました。ホスト/ポートとネットワークを確認してください。",
      refused: "接続が拒否されました。このアドレスでサーバーが実行されていますか？",
      hostNotFound: "ホストが見つかりません。ホスト名を確認してもう一度試してください。",
      hostUnreachable: "ホストに到達できません。ネットワークとファイアウォールを確認してください。",
      tlsError: "TLSエラー。直接接続は、デーモンの前にTLS終端がある場合のみSSLを使用します。",
      unableToConnect: "接続できません。ホスト/ポートとデーモンが到達可能かを確認してください。",
      details: "詳細: {{detail}}",
    },
  },
  remoteSsh: {
    title: "リモート SSH",
    helper: "リモートホストで動作する Ait デーモンに接続します。",
    fields: {
      target: "SSH ホスト",
    },
    actions: {
      cancel: "キャンセル",
      connect: "接続",
      connecting: "接続中...",
    },
    errors: {
      tokenRequired: "リモート Ait サーバートークン（32〜256 文字）を入力してください。",
      targetRequired: "SSH ホストは必須です",
      invalidTarget: "有効な ssh:// ホストを入力してください",
      failedToConnect: "SSH で接続できません。{{detail}}",
    },
  },
  link: {
    title: "ペアリングリンクを貼り付け",
    helper: "サーバーからのペアリングリンクを貼り付けてください。",
    label: "ペアリングリンク",
    errors: {
      required: "ペアリングリンクを貼り付けてください（.../#offer=...）",
      missingOffer: "リンクには#offer=...が必要です",
      emptyOffer: "オファーのペイロードが空です",
      invalid: "無効なペアリングリンク",
      unableToPair: "ホストをペアリングできません",
    },
    alert: {
      failedTitle: "ペアリングに失敗しました",
    },
    actions: {
      cancel: "キャンセル",
      pair: "ペアリング",
      pairing: "ペアリング中...",
    },
  },
  scan: {
    title: "QRをスキャン",
    webUnavailableTitle: "Webでは利用できません",
    webUnavailableBody:
      "QRスキャンはWebビルドではサポートされていません。代わりに「リンクを貼り付け」を使用してください。",
    backToSettings: "設定に戻る",
    cameraPermissionTitle: "カメラの権限",
    cameraPermissionBody:
      "デーモンからのペアリングQRコードをスキャンするためにカメラへのアクセスを許可してください。",
    grantPermission: "権限を許可",
    pairing: "ペアリング中...",
    unableToPair: "ホストをペアリングできません",
    errorTitle: "エラー",
  },
  device: {
    loadingOffer: "ペアリングオファーを読み込み中...",
    failedToLoadOffer: "ペアリングオファーの読み込みに失敗しました。",
    relayDisabled:
      "リレーが有効になっていません。デバイスをペアリングするにはリレーを有効にしてください。",
    enableTitle: "リレーを有効にしますか？",
    enableDescription:
      "リレーを使うと、このデバイスからどこでも接続できます。ペアリング通信はエンドツーエンドで暗号化されます。",
    relayDocs: "リレーの仕組み",
    relayDocsAccessibility: "Ait リレーの仕組みを読む",
    enableRelay: "リレーを有効にする",
    enablingRelay: "有効化中...",
    notNow: "今はしない",
    directConnectionHint:
      "リレーを使わない場合は、TCP、Tailscale、または別の VPN で直接接続してください。QR コードは作成されません。",
    updateRequired: "Ait Desktop からリレーを有効にするにはホストを更新してください。",
    unavailable: "ペアリングオファーが利用できません。",
    hint: "スマートフォンのAitでこのQRコードをスキャンするか、以下のリンクをコピーしてください。",
    securityWarning:
      "このペアリングリンクはパスワードと同様に扱ってください。リンクを知っている人は誰でもこのデーモンにアクセスできます。",
    qrUnavailable: "QRコードが利用できません。",
    qrAccessibility: "ペアリング QR コード",
    retry: "再試行",
    copy: "コピー",
    copied: "コピーしました",
  },
};
