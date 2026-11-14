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
};
