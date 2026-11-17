export const importSession = {
  chooseHostTitle: "호스트에서 가져오기",
  title: "세션 가져오기",
  searchPlaceholder: "세션 검색...",
  scope: {
    host: "{{host}}의 세션",
    workspace: "이 워크스페이스",
  },
  filters: {
    all: "전체",
    label: "필터: {{provider}}",
    title: "프로바이더로 필터링",
  },
  status: {
    connectHost: "세션을 가져오려면 호스트에 연결하세요",
    updateHost: "세션을 가져오려면 호스트를 업데이트하세요.",
    noProviders: "가져올 수 있는 프로바이더가 활성화되어 있지 않습니다.",
    loading: "최근 세션을 불러오는 중...",
    failedProvider: "{{provider}} 세션을 불러올 수 없습니다",
    failedImport: "선택한 세션을 가져올 수 없습니다.",
    workingDirectoryUnavailable:
      "이 호스트에서 세션의 작업 디렉터리를 사용할 수 없습니다: {{path}}. 디렉터리를 복원한 후 다시 가져오세요.",
  },
  actions: {
    refresh: "세션 새로고침",
    showAll: "전체 표시",
    loadMore: "더 보기",
  },
  preview: {
    untitledSession: "제목 없는 세션",
    noPrompt: "프롬프트 미리보기 없음",
  },
  sessionPreview: {
    title: "세션 미리보기",
    open: "세션 기록 미리보기(읽기 전용)",
    activeNotice:
      "이 세션에는 완료되지 않은 턴이 있습니다. 미리보기는 읽기 전용이며 재개하지 않습니다.",
    loadOlder: "이전 메시지 불러오기",
    loadingOlder: "이전 메시지를 불러오는 중...",
    empty: "이 세션에 표시할 기록이 없습니다.",
    failed: "세션 기록을 읽을 수 없습니다.",
    retry: "다시 시도",
    you: "나",
    assistant: "어시스턴트",
    compacted: "이전 컨텍스트가 압축되었습니다",
    toolRunning: "실행 중",
    toolCompleted: "완료",
    toolFailed: "실패",
    toolCanceled: "취소됨",
  },
  empty: {
    noRecent: "가져올 최근 세션이 없습니다.",
    noMatches: "검색과 일치하는 세션이 없습니다.",
    alreadyImported: "최근 세션이 모두 이미 가져와졌습니다.",
    noProviderSessions: "{{provider}} 세션을 찾을 수 없습니다.",
  },
  row: {
    importing: "가져오는 중...",
  },
};
