export const bots = {
  modeCapWarning:
    "エージェントがコマンドを実行できるモードでは、このコンピューターの Ait 認証情報を読み取り、Bot の Ait 制限を回避することもできます。エージェントに無人で任せてよい作業にのみ許可してください。",
  commandReachWarning:
    "Bot は、あなたが起動したものを含め、このワークスペース内のすべてのエージェントにプロンプトを送信し、設定を変更できます。これらのエージェントは各自の権限を保持するため、Bot のプロンプトは Bot 自身の制限を超える機能に及ぶ可能性があります。",
  terminalsWarning:
    "ターミナルはこのコンピューター上でコマンドを実行するため、その Ait 認証情報を読み取り、Bot の Ait 制限を回避できます。エージェントに無人で任せてよい作業にのみ有効にしてください。",
  noteInstructionsInInput:
    "このプロバイダーは更新可能なシステムプロンプトに対応していないため、Bot の組み込み指示とメモリは会話の入力に付加されます。",
  noteNoReadOnlyMode:
    "このプロバイダーには読み取り専用モードがないため、Bot はデフォルトモードで実行されます。承認が必要な書き込みは自動的に拒否されますが、プロバイダーが承認なしで許可する操作はブロックされず、このコンピューターの Ait 認証情報の読み取りや Bot の Ait 制限の回避も含まれます。",
  noteToolsNeedApproval:
    "このプロバイダーは Ait ツールを事前承認できないため、承認が必要な Ait ツール呼び出しは自動的に拒否されます。Bot は会話のみ可能で、エージェントの起動や管理ができない場合があります。",
  noteNoAitTools: "このプロバイダーは Ait ツール接続に対応していないため、Bot は会話のみ可能です。",
  permissions: "権限",
  permissionsHint: "権限の変更は Bot の次のターンから反映されます。",
  groupObserve: "監視（常に有効）：エージェント、アクティビティ、プロバイダーの表示",
  groupAgents: "エージェント：エージェントの作成、プロンプト送信、設定変更",
  groupStop: "停止：エージェントのキャンセルと停止",
  groupSchedules: "スケジュール：スケジュールの作成と管理",
  groupScripts: "スクリプト：ワークスペースのスクリプトの実行",
  groupTerminals: "ターミナル：ターミナルの作成と入力",
  permissionRiskHint: "コマンドを実行できます。",
  modeCap: "デフォルトのエージェントモード",
  modeNotAllowed: "許可しない",
  modeCapHint:
    "Bot が各プロバイダーで起動するエージェントのモードを選択します。Bot はこのモードしか使用できません。プロバイダーを使わせない場合は「許可しない」を選択してください。",
  providerUnavailable: "{{provider}}（利用不可）",
  readOnlyHint:
    "Bot はプロバイダーが提供する最も厳格なモードで実行され（Claude ではデフォルトモードで、あなた自身の許可ルールは引き続き適用されます）、変更は Bot が起動するエージェントに任せます。",
  title: "Bot",
  chat: "チャット",
  settings: "設定",
  provider: "プロバイダー",
  model: "モデル",
  mode: "権限モード",
  thinking: "推論",
  defaultOption: "プロバイダーのデフォルト",
  save: "保存",
  setupTitle: "ワークスペース Bot を設定",
  setupHint: "Bot はこのワークスペースを管理し、このタブを閉じても動作し続けます。",
  setup: "Bot を設定",
  clearMemory: "保存済みのメモリを消去",
  notConfigured:
    "このワークスペースにはまだ Bot がありません。設定できるのはホストの所有者のみです。",
  disabled: "Bot を使うには、設定 → 情報 で実験的機能を有効にしてください。",
  unsupported: "Bot を使うには、このホストを更新してください。",
  disconnected: "Bot を使うには、このホストに接続してください。",
  message: "メッセージ",
  cancel: "ターンをキャンセル",
  emptyChat: "メッセージを送信して会話を始めましょう。",
  conversation: "会話",
  conversationOwner: "Ait（あなた）",
  conversationOtherChannel: "その他のチャネル",
  conversationReadOnly:
    "閲覧のみ：この会話は {{channel}} で行われています。ここでメッセージを送れるのは Ait の会話だけです。",
  session: "Bot セッション",
  reset: "新しい会話",
  resetConfirm:
    "新しい会話を始めますか？既存のメッセージは保持され、最新の Bot 設定が適用されます。",
  configurationHint:
    "モデルの変更は新しい会話に適用されます。ここで使うには新しい会話を始めてください。",
  memory: "メモリ",
  memoryHint: "PR タイトルの言語や返信スタイルなど、Bot が返信のたびに読み込む設定です。",
  saveMemory: "メモリを保存",
  memoryConflict: "メモリが別の場所で変更されました。再読み込みしてから保存してください。",
  resumeToEdit: "変更するには Bot を再開してください。",
  reload: "再読み込み",
  pause: "一時停止",
  remove: "Bot を削除",
  removeConfirm:
    "この Bot を削除しますか？会話履歴は表示されなくなり、メモリは保持され、Telegram と Feishu の接続は切断されます。",
  paused: "一時停止中",
  resume: "再開",
  directoryMissing:
    "ワークスペースのディレクトリが見つからないため、Bot は一時停止中です。ディレクトリを復元してから再開してください。",
  requestFailed: "リクエストに失敗しました",
  tooLong: "メッセージは 16 KiB 以内にしてください。",
  memoryTooLong: "メモリは 32 KiB 以内にしてください。",
  channels: "チャンネル",
  addChannel: "チャンネルを追加",
  channelType: "チャンネルの種類",
  telegram: "Telegram",
  feishu: "Feishu",
  lark: "Lark",
  tokenRef: "Bot トークンの参照",
  secretRef: "アプリシークレットの参照",
  appId: "アプリ ID",
  allowedSenders: "許可する送信者（カンマまたは改行区切り）",
  allowGroups: "グループチャットを許可",
  channelRefHint:
    "認証情報は、デーモンを起動するプロセスの環境変数から取得されます。env:AIT_SERVER_CREDENTIAL_TELEGRAM のような変数参照のみを入力し、許可する送信者を少なくとも 1 人指定してください。",
  channelEnabled: "有効",
  channelConnected: "接続済み",
  channelIdle: "待機中",
  feishuScanHint:
    "Feishu アプリで QR コードをスキャンします。Feishu がボットを作成して Ait が接続します。変更するまでメッセージを送れるのはあなただけです。",
  feishuScanStart: "QR コードを表示",
  feishuScanWaiting:
    "Feishu アプリでこのコードをスキャンし、新しいボットを承認してください。完了するまでこのページを開いたままにしてください。",
  feishuScanOpen: "Feishu で開く",
  feishuScanQrLabel: "Feishu 設定用 QR コード",
  feishuScanRetry: "再試行",
  feishuScanDenied: "Feishu でリクエストが拒否されました。",
  feishuScanExpired: "この QR コードは期限切れです。もう一度開始してください。",
  feishuScanLark:
    "この QR コードは Feishu 用です。Lark アカウントでは Lark を選択し、アプリの認証情報を手動で入力してください。",
  feishuScanUnavailable:
    "Feishu の設定を完了できませんでした。再試行するか、認証情報を手動で入力してください。",
  feishuManualEntry: "認証情報を手動で入力",
  feishuScanInstead: "QR コードをスキャンする",
  feishuAppSecret: "App Secret",
  feishuAppSecretHint:
    "App Secret を貼り付けます。Ait は {{platform}} で検証し、このホストに非公開で保存します。env:AIT_SERVER_CREDENTIAL_* 参照も使えます。",
  feishuManualStep1: "{{developerUrl}} でカスタムアプリを作成し、ボット機能を有効にします。",
  feishuManualStep2:
    "権限管理で im:message:send_as_bot, im:message.p2p_msg:readonly, im:message.group_at_msg:readonly を追加します。",
  feishuManualStep3:
    "App ID と App Secret をここにコピーし、自分の {{platform}} Open ID（ou_...）を許可された送信者に追加して、チャンネルを追加します。",
  feishuManualStep4:
    "次に {{platform}} の「イベントとコールバック」で長時間接続を選び、im.message.receive_v1 を購読します。チャンネルが接続されるまで {{platform}} はこの設定を保存できません。",
  feishuManualStep5: "アプリのバージョンを公開すると、ボットにメッセージを送れるようになります。",
  telegramGuideStep1:
    "このコードをスキャンするかボタンを押して、Telegram で BotFather を開きます。",
  telegramGuideStep2: "/newbot を送信し、名前と bot で終わるユーザー名を決めます。",
  telegramGuideStep3: "BotFather から届いたトークンをコピーして、下に貼り付けます。",
  telegramBotFatherOpen: "BotFather を開く",
  telegramBotFatherQrLabel: "BotFather を開く QR コード",
  telegramToken: "ボットトークン",
  telegramTokenHint: "Ait は Telegram でトークンを検証し、このホストに非公開で保存します。",
  telegramContinue: "続行",
  telegramPairWaiting:
    "このコードをスキャンするかリンクを開き、@{{bot}} とのチャットで Start をタップすると、許可された送信者になります。完了するまでこのページを開いたままにしてください。",
  telegramPairOpen: "Telegram で開く",
  telegramPairQrLabel: "新しいボットを開く QR コード",
  telegramInvalidToken:
    "Telegram がこのトークンを拒否しました。BotFather からもう一度コピーしてください。",
  telegramAlreadyConnected:
    "このボットはこのホストのチャンネルに既に接続されています。先にそのチャンネルを削除するか、新しいボットを作成してください。",
  telegramWebhookConflict:
    "このボットには webhook が設定されています。deleteWebhook で削除してから再試行してください。",
  telegramPollingConflict:
    "別のプログラムがこのボットのメッセージを受信しています。停止してから再試行してください。",
  telegramExpired: "このリンクは期限切れです。続行して新しいリンクを取得してください。",
  telegramUnavailable:
    "Telegram に接続できませんでした。このホストの接続を確認して再試行してください。",
  telegramUseEnvironment: "環境変数を使用",
  telegramUseGuided: "ガイド付き設定を使用",
  weixin: "WeChat",
  qq: "QQ",
  weixinScanHint:
    "WeChat で QR コードをスキャンしてボットを接続します。プライベートチャットであなただけがメッセージを送れます。",
  weixinScanWaiting:
    "WeChat でこのコードをスキャンし、スマートフォンで確認してください。完了するまでこのページを開いたままにしてください。",
  weixinScanOpen: "WeChat で開く",
  weixinScanQrLabel: "WeChat 設定用 QR コード",
  weixinScanUnavailable: "WeChat の設定を完了できませんでした。もう一度お試しください。",
  qqScanHint:
    "QQ で QR コードをスキャンします。QQ がボットを作成して Ait が接続します。変更するまであなただけがメッセージを送れます。",
  qqScanWaiting:
    "QQ でこのコードをスキャンし、新しいボットを承認してください。完了するまでこのページを開いたままにしてください。",
  qqScanOpen: "QQ で開く",
  qqScanQrLabel: "QQ 設定用 QR コード",
  qqScanUnavailable:
    "QQ の設定を完了できませんでした。もう一度試すか、認証情報を手動で入力してください。",
  scanAlreadyConnected:
    "このボットはこのホストのチャンネルに既に接続されています。そのチャンネルを削除してからもう一度お試しください。",
  qqManualStep1: "q.qq.com でボットを作成し、App ID と App Secret をここにコピーします。",
  qqManualStep2:
    "チャンネルを追加したら、その行でペアリングコードを取得し、QQ でボットに送信してください。これで許可された送信者になります。",
  qqAppSecretHint: "Ait は App Secret を QQ で確認し、このホストに非公開で保存します。",
  qqPairButton: "ペアリングコードを取得",
  qqPairHint:
    "コード: {{code}}。プライベートチャットでボットに送るか、QQ グループでボットを @ メンションして送信してください。1 回のみ有効で、10 分後に期限切れになります。",
  imessage: "iMessage",
  webhook: "Webhook",
  webhookHint:
    "Ait Hub がこの Bot 用の HTTPS アドレスとシークレットを作成します。HTTP リクエストを送れるシステムなら、これを使って Bot にメッセージを送り、返信を受け取れます。",
  webhookCreate: "Webhook を作成",
  webhookUrl: "アドレス",
  webhookSecret: "シークレット",
  webhookSecretOnce:
    "シークレットは今回だけ表示されます。今すぐコピーしてください。紛失した場合は新しく作成できます。",
  webhookExample: "例",
  webhookExampleHint:
    "呼び出すとステータスアドレスが返されます。同じシークレットでリクエストするか（?wait=30 を付けると返信を待てます）、呼び出し時に callback_url を指定すると返信がそのアドレスに送られます。同じ thread の呼び出しは同じ会話を続けます。",
  webhookCopy: "コピー",
  webhookCopied: "コピーしました",
  webhookDone: "完了",
  webhookRotate: "新しいシークレットを作成",
  webhookRotateConfirm:
    "現在のシークレットはすぐに使えなくなります。使用中の呼び出し元は新しいシークレットに切り替える必要があります。",
  webhookCenterOffline:
    "このホストは Ait Hub に接続されていません。サインインして Hub 接続をオンにしてから、もう一度お試しください。",
  webhookCenterUnsupported: "この Ait Hub はまだ Webhook に対応していません。",
  webhookAutomationUnavailable: "この Ait Hub では自動化が有効になっていません。",
  webhookLimitReached:
    "アカウントの Webhook 数が上限に達しました。使っていないものを削除してから、もう一度お試しください。",
  webhookCenterError:
    "Ait Hub で Webhook を設定できませんでした。しばらくしてからもう一度お試しください。",
  imessageGuideStep1: "app.beta.photon.codes で Photon Beta プロジェクトを作成します。",
  imessageGuideStep2:
    "そのプロジェクトで project:read, project:write, platforms:read、platforms:write の権限を持つ API キーを作成します。",
  imessageGuideStep3:
    "下に Project ID と API キー、自分の電話番号または iMessage のメールアドレスを入力します。",
  imessagePhotonOpen: "Photon を開く",
  imessagePrivacyNotice:
    "このボットとのメッセージは Photon のサービスを経由します。ダイレクトメッセージのみ対応しており、グループチャットには対応していません。",
  imessageProjectId: "Project ID",
  imessageApiKey: "API キー",
  imessageApiKeyHint: "Ait は Photon でキーを検証し、このホストに非公開で保存します。",
  imessageHandle: "自分の電話番号または iMessage のメールアドレス",
  imessageHandleHint:
    "iMessage の送信に使うアドレスです。電話番号は +14155550123 のような国際形式で入力してください。",
  imessageContinue: "続行",
  imessagePairPhoneNumber: "送信先",
  imessagePairCode: "コード",
  imessagePairWaiting:
    "必ず {{handle}} から、SMS ではなく iMessage でこのコードを送信してください。QR コードをスキャンするかリンクを開くと、コードが自動で入力されます。完了するまでこのページを開いたままにしてください。",
  imessagePairExpires: "コードの有効期限は {{minutes}} 分です。",
  imessagePairOpen: "メッセージで開く",
  imessagePairQrLabel: "コードを入力した状態でメッセージを開く QR コード",
  imessageInvalidKey:
    "Photon がこの Project ID または API キーを拒否しました。両方を確認し、キーに 4 つの権限がすべてあることを確かめてください。",
  imessageInvalidHandle:
    "+14155550123 のような国際形式の電話番号、または iMessage のメールアドレスを入力してください。",
  imessageDedicatedLine:
    "この Photon プロジェクトは専用回線を使用しており、対応していません。共有回線のプロジェクトを使用してください。",
  imessageAlreadyConnected:
    "この Photon プロジェクトはこのホストのチャンネルに既に接続されています。先にそのチャンネルを削除するか、別のプロジェクトを使用してください。",
  imessageExpired: "このコードは期限切れです。続行して新しいコードを取得してください。",
  imessageUnavailable:
    "Photon に接続できませんでした。このホストの接続を確認して再試行してください。",
  deleteChannel: "チャンネルを削除",
  deleteChannelConfirm: "このチャンネルを削除しますか？外部との接続は切断されます。",
  notifications: {
    title: "通知",
    wokeBy: "ワークスペースの更新",
    silent: "報告することはありません。",
    wakeScope: "Bot を起こす通知",
    wakeScopeOwn: "自分の作業のみ（既定）",
    wakeScopeAll: "このワークスペースのすべて",
    wakeScopeHint:
      "自分の作業とは、Bot が開始したエージェントとスケジュールです。すべてを選ぶと、このワークスペースのほかのエージェント、スケジュール、ターミナルのイベントでも Bot のターンが始まり、モデルの呼び出しが増えます。",
    recent: "最近の通知",
    empty: "まだ通知はありません。",
    state: {
      logged: "起動対象外",
      pending: "Bot の起動待ち",
      woke: "Bot を起こしました",
      other: "その他の状態",
    },
    skipped: {
      over_budget: "未配信：1 時間あたりの起動上限に達しました",
      inbox_full: "未配信：待機中の通知が多すぎます",
      paused: "未配信：Bot は一時停止中でした",
      observed: "再通知なし：Bot はその会話でこの Agent を確認済みでした",
      blocked: "未配信：その会話は中断されており、リセットが必要です",
      reported: "再通知なし：エージェントがこの結果をすでにタスクで報告済みです",
      other: "Bot を起こしませんでした",
    },
    kind: {
      agent_finished: "エージェントが完了",
      agent_failed: "エージェントが失敗",
      agent_permission: "エージェントが許可を待っています",
      schedule_succeeded: "スケジュール実行が成功",
      schedule_failed: "スケジュール実行が失敗",
      terminal_attention: "ターミナルが注意を求めています",
      ticket_blocked: "エージェントがタスクで行き詰まっています",
      ticket_done: "エージェントがタスクの担当分を完了",
      ticket_failed: "エージェントがタスクの担当分を完了できませんでした",
      other: "ワークスペースの更新",
    },
  },
  tickets: {
    title: "タスク",
    pillOpen: "進行中 {{count}}",
    pillWaiting: "対応待ち {{count}}",
    pillLabel: "Bot のタスク：進行中 {{open}}、対応待ち {{waiting}}",
    empty: "進行中のタスクはありません。Bot がエージェントに作業を任せるとタスクが表示されます。",
    showClosed: "最近閉じたタスクを表示",
    hideClosed: "閉じたタスクを隠す",
    closedHeading: "最近閉じたタスク",
    noClosed: "閉じたタスクはありません。",
    agentCount: "エージェント：{{count}}",
    agentFallback: "エージェント {{id}}",
    agents: "エージェント",
    noAgents: "まだエージェントは割り当てられていません。",
    request: "依頼内容",
    history: "履歴",
    noReports: "まだ報告はありません。",
    outcome: "結果",
    noOutcome: "結果は記録されていません。",
    outcomePlaceholder: "結果（任意）",
    markDone: "完了にする",
    cancel: "タスクを取り消す",
    cancelConfirm:
      "このタスクを取り消しますか？エージェントは実行を続けるため、必要なら個別に停止してください。タスクは後で再開できます。",
    keep: "タスクを残す",
    reopen: "再開",
    openLimit: "進行中のタスクが多すぎます。完了したタスクを閉じてから再開してください。",
    conflict: "このタスクは更新されました。現在の状態を確認してからもう一度お試しください。",
    linked: "タスク：{{title}}",
    untitled: "無題のタスク",
    movedTo: "{{status}}に変更",
    status: {
      open: "進行中",
      waiting: "対応待ち",
      done: "完了",
      failed: "失敗",
      cancelled: "取り消し済み",
      other: "その他の状態",
    },
    waiting: {
      permission: "エージェントが許可を待っています",
      blocked: "エージェントが行き詰まり、入力を待っています",
      other: "対応が必要です",
    },
    source: {
      agent: "エージェント",
      bot: "Bot",
      user: "あなた",
      system: "Ait",
      other: "その他",
    },
    report: {
      progress: "進捗",
      blocked: "行き詰まり",
      done: "担当分を完了",
      failed: "担当分を断念",
      agent_finished: "エージェントがターンを完了",
      agent_failed: "エージェントが失敗",
      agent_permission: "エージェントが許可を待っています",
      note: "メモ",
      status: "状態を変更",
      other: "更新",
    },
  },
  status: {
    queued: "待機中",
    sending: "開始中…",
    running: "処理中…",
    needs_approval: "承認が必要",
    completed: "完了",
    failed: "失敗",
    uncertain: "中断されました。続けるには新しい会話を始めてください",
    cancelled: "キャンセル済み",
  },
};
