const sharedSharing = {
  sharedWithMeCount: "共有されたリソース（{{count}}）",
  incomingHostHint: "有効なホストと共有されたプロジェクトやワークスペースが一覧に表示されます。",
  showHostInHosts: "{{host}} をホスト一覧に表示",
  sharedByMeCount: "自分が共有（{{count}}）",
  action: "グループと共有",
  manage: "管理",
  refresh: "更新",
  stop: "共有を停止",
  active: "有効",
  loading: "共有リソースを読み込み中…",
  hostSection: "共有",
  hostTitle: "このノードを共有",
  hostHint:
    "グループにこのホスト上のすべてのプロジェクトとワークスペースへのアクセスを許可します。",
  shareTitle: "{{name}} を共有",
  resourceTitle: "リソースを共有",
  intro: "共有者がオンラインの間、グループメンバーは AIT でこのリソースを開けます。",
  projectHostHint: "選択したホスト上のこのプロジェクトとワークスペースを共有します。",
  hostUnavailable: "このホストは利用できません。接続して共有を管理してください。",
  scope: { node: "ノード", project: "プロジェクト", workspace: "ワークスペース" },
  scopeHint: {
    node: "このノード上のすべて",
    project: "このプロジェクトとワークスペース",
    workspace: "このワークスペースのみ",
  },
  status: {
    shared: "共有済み",
    rejected: "未共有 · {{code}}",
    waiting: "サービスを待機中",
    groupAdmin: "このグループを管理しています",
    groupMember: "グループメンバー",
  },
  noGroupsTitle: "利用可能なグループがありません",
  noGroupsHint:
    "ホスト設定 → 接続からこのノードをオンラインサービスに接続し、ノード所有者がグループに所属していることを確認してください。",
  nodeSettingsOnly:
    "この設定にはノード全体の共有が必要です。この共有でアクセスできるのはプロジェクトまたはワークスペースだけです。",
  ownerOnly:
    "共有を管理できるのはノード所有者だけです。共有されたリソースを再共有することはできません。",
  unsupported: "このノードは共有管理に対応していません。更新して再試行してください。",
  loadError: "共有設定を読み込めません。",
  updateError: "共有を更新できません。",
  stopError: "共有を停止できません。",
  emptyIncomingTitle: "まだ共有されたものはありません",
  emptyIncomingHint: "グループメンバーが共有したリソースがここに表示されます。",
  emptyOutgoingTitle: "何も共有していません",
  emptyOutgoingHint:
    "ワークスペースやプロジェクトのメニューを使うか、ホスト設定を開いてノード全体を共有してください。",
} as const;

export const sharing = {
  ...sharedSharing,
  title: "共有",
  listError: "共有リソースを読み込めませんでした。",
};
