const sharedSharing = {
  sharedWithMeCount: "나와 공유됨({{count}})",
  incomingHostHint: "활성화된 호스트와 공유된 프로젝트 및 워크스페이스가 목록에 표시됩니다.",
  showHostInHosts: "호스트에 {{host}} 표시",
  sharedByMeCount: "내가 공유함({{count}})",
  action: "그룹과 공유",
  manage: "관리",
  refresh: "새로 고침",
  stop: "공유 중지",
  active: "활성",
  loading: "공유 리소스를 불러오는 중…",
  hostSection: "공유",
  hostTitle: "이 노드 공유",
  hostHint: "그룹에 이 호스트의 모든 프로젝트와 워크스페이스에 대한 액세스 권한을 부여합니다.",
  shareTitle: "{{name}} 공유",
  resourceTitle: "리소스 공유",
  intro: "공유 소유자가 온라인이면 그룹 구성원이 AIT에서 이 리소스를 열 수 있습니다.",
  projectHostHint: "선택한 호스트에서 이 프로젝트와 워크스페이스를 공유합니다.",
  hostUnavailable: "이 호스트를 사용할 수 없습니다. 연결하여 공유를 관리하세요.",
  scope: { node: "노드", project: "프로젝트", workspace: "워크스페이스" },
  scopeHint: {
    node: "이 노드의 모든 항목",
    project: "이 프로젝트와 워크스페이스",
    workspace: "이 워크스페이스만",
  },
  status: {
    shared: "공유됨",
    rejected: "공유되지 않음 · {{code}}",
    waiting: "서비스 대기 중",
    groupAdmin: "이 그룹을 관리합니다",
    groupMember: "그룹 구성원",
  },
  noGroupsTitle: "사용 가능한 그룹 없음",
  noGroupsHint:
    "호스트 설정 → 연결에서 이 노드를 온라인 서비스에 연결한 다음 노드 소유자가 그룹에 속해 있는지 확인하세요.",
  nodeSettingsOnly:
    "이 설정에는 전체 노드 공유가 필요합니다. 이 공유는 해당 프로젝트 또는 워크스페이스에만 액세스 권한을 부여합니다.",
  ownerOnly:
    "노드 소유자만 공유를 관리할 수 있습니다. 나와 공유된 리소스는 다시 공유할 수 없습니다.",
  unsupported: "이 노드는 공유 관리를 지원하지 않습니다. 업데이트한 뒤 다시 시도하세요.",
  loadError: "공유 설정을 불러오지 못했습니다.",
  updateError: "공유를 업데이트하지 못했습니다.",
  stopError: "공유를 중지하지 못했습니다.",
  emptyIncomingTitle: "아직 나와 공유된 항목이 없습니다",
  emptyIncomingHint: "그룹 구성원이 공유한 리소스가 여기에 표시됩니다.",
  emptyOutgoingTitle: "공유 중인 항목이 없습니다",
  emptyOutgoingHint:
    "워크스페이스나 프로젝트 메뉴를 사용하거나 호스트 설정을 열어 전체 노드를 공유하세요.",
} as const;

export const sharing = {
  ...sharedSharing,
  title: "공유",
  listError: "공유 리소스를 불러올 수 없습니다.",
};
