export const desktop = {
  windowControls: {
    minimize: "창 최소화",
    maximize: "창 최대화",
    restore: "창 복원",
    close: "창 닫기",
  },
  quitting: {
    title: "Ait 종료 중...",
    detail: "로컬 데몬을 중지하는 중입니다.",
  },
  daemon: {
    listen: {
      host: "수신 주소",
      port: "포트",
      savedHint: "변경 사항을 적용하려면 데몬을 다시 시작하세요.",
      override:
        "AIT_SERVER_LISTEN이 이 설정을 재정의합니다: {{address}}. 저장된 주소를 사용하려면 환경 변수를 제거하세요.",
      invalid: "IPv4 또는 IPv6 주소(localhost 포함)와 0~65535 사이의 포트를 입력하세요.",
    },
    lifecycle: {
      owned: "이 데스크톱 세션에서 시작됨",
      attached: "기존 데몬에 연결됨",
      ownedMessage: "이 데몬은 이 데스크톱 세션에서 시작되었습니다.",
      attachedMessage: "이 데몬은 이 데스크톱 세션에서 시작되지 않았습니다.",
      stopTitle: "로컬 데몬을 중지할까요?",
      stopMessage:
        "{{ownership}}\n홈: {{home}}\n감독 프로세스 PID: {{pid}}\n실행 중인 에이전트 작업이 중단됩니다.",
      stop: "데몬 중지",
      stopping: "중지 중…",
      stopFailed: "데몬을 중지할 수 없음",
      pauseAttached: "데몬 자동 관리를 일시 중지할까요? 연결된 데몬은 계속 실행됩니다.",
      pause: "관리 일시 중지",
      workerUpdated: "워커가 {{version}} 버전으로 업데이트됨",
      supervisorRefresh:
        "실행 중인 감독 프로세스는 원래 코드를 유지합니다. 업데이트하려면 이를 시작한 프로그램에서 중지한 뒤 다시 시작해야 합니다.",
    },
    title: "데몬",
    status: {
      title: "상태",
      builtInOnly: "여기에는 내장 데스크톱 데몬만 표시됩니다",
      running: "실행 중",
      notRunning: "실행 중이 아님",
      pid: "PID {{pid}}",
    },
    management: {
      title: "내장 데몬 관리",
      hint: "Ait가 내장 데몬을 시작하고 중지하도록 허용합니다",
      pauseTitle: "내장 데몬 일시 중지",
      pauseMessage:
        "내장 데몬이 즉시 중지됩니다. 내장 데몬에 연결된 실행 중인 에이전트와 터미널이 중지됩니다.",
      pauseAndStop: "일시 중지 후 중지",
      registrationFailed:
        "내장 데몬이 시작되었지만 Ait가 localhost 연결을 저장하지 못했습니다. 데몬 관리를 껐다가 다시 켜거나 localhost를 수동으로 추가하세요.",
      pausedStopFailed: "내장 데몬 관리가 일시 중지되었지만 Ait가 데몬을 중지하지 못했습니다.",
      updateFailed: "내장 데몬 관리를 업데이트할 수 없습니다.",
    },
    keepRunning: {
      title: "종료 후에도 데몬 계속 실행",
      hint: "Ait를 종료해도 에이전트와 터미널이 계속 실행되며, 다음 실행 시 다시 연결합니다.",
    },
    logs: {
      title: "로그 파일",
      modalTitle: "데몬 로그",
      unavailable: "로그 경로를 사용할 수 없습니다",
      empty: "(로그 파일이 비어 있습니다)",
      copied: "로그 경로가 복사되었습니다.",
      copyFailed: "로그 경로를 복사할 수 없습니다.",
      open: "로그 열기",
      copyPath: "경로 복사",
    },
    fullStatus: {
      title: "전체 상태",
      modalTitle: "데몬 상태",
      view: "상태 보기",
      copied: "상태가 클립보드에 복사되었습니다.",
      fetchFailed: "데몬 상태를 가져오지 못했습니다: {{message}}",
    },
    versionMismatch:
      "앱과 데몬의 버전이 일치하지 않습니다. 최상의 경험을 위해 둘 다 동일한 버전으로 업데이트하세요.",
    loadFailed: "데스크톱 데몬 상태를 불러올 수 없습니다.",
  },
  updates: {
    status: {
      checking: "앱 업데이트 확인 중...",
      installing: "앱 업데이트 설치 중...",
      upToDate: "앱이 최신 상태입니다.",
      upToDateWithLastChecked: "최신 상태입니다. 마지막 확인: {{time}}.",
      pending: "업데이트가 준비되면 알려드리겠습니다.",
      pendingWithLastChecked:
        "업데이트가 준비되면 알려드리겠습니다. {{time}}에서 마지막으로 확인했습니다.",
      pendingWithVersion: "업데이트 발견: {{version}}. 다운로드 중...",
      pendingWithVersionAndLastChecked:
        "업데이트 발견: {{version}}. 다운로드 중... {{time}}에서 마지막으로 확인했습니다.",
      availableWithVersion: "업데이트 준비됨: {{version}}",
      availableWithVersionAndLastChecked:
        "업데이트 준비됨: {{version}}. {{time}}에서 마지막으로 확인했습니다.",
      available: "설치할 앱 업데이트가 준비되었습니다.",
      availableWithLastChecked:
        "앱 업데이트를 설치할 준비가 되었습니다. {{time}}에서 마지막으로 확인했습니다.",
      installed: "앱 업데이트가 설치되었습니다. 재시작이 필요합니다.",
      failed: "앱을 업데이트하지 못했습니다.",
      idle: "업데이트 상태를 아직 확인하지 않았습니다.",
    },
    installError: "데스크톱 앱 업데이트를 설치할 수 없습니다.",
    callout: {
      installingTitle: "업데이트 설치 중",
      failedTitle: "업데이트 실패",
      availableTitle: "업데이트 사용 가능",
      genericError: "문제가 발생했습니다.",
      whatsNew: "새로운 기능",
      installingAction: "설치 중...",
      installAndRestart: "설치 후 재시작",
      installingDescription: "설치 후 재시작 중...",
      versionReady: "{{version}} 설치가 준비되었습니다.",
      newVersionReady: "새 버전 설치가 준비되었습니다.",
      restartWarning: "앱을 업그레이드하면 실행 중인 에이전트가 중지되고 터미널 세션이 닫힙니다.",
    },
  },
  settings: {
    loadFailed: "데스크톱 설정을 불러올 수 없습니다.",
    saveFailed: "데스크톱 설정을 저장할 수 없습니다.",
  },
  rosetta: {
    title: "Apple Silicon 빌드 다운로드",
    runningIntel: "Apple Silicon에서 Rosetta로 Ait의 Intel 빌드를 실행하고 있습니다.",
    highCpu:
      "이로 인해 CPU 사용량이 높아집니다. 이를 해결하려면 Apple Silicon 빌드를 다운로드하세요.",
    download: "다운로드",
  },
  permissions: {
    notifications: {
      allowed: "OS에서 알림이 허용되어 있습니다.",
      denied: "시스템 설정에서 알림이 거부되어 있습니다.",
      notGranted: "알림이 아직 허용되지 않았습니다.",
      webOnly: "데스크톱 알림 상태는 웹 런타임에서만 사용할 수 있습니다.",
      supported: "데스크톱 알림이 지원됩니다.",
      unsupported: "이 플랫폼에서는 데스크톱 알림이 지원되지 않습니다.",
      apiUnavailable: "이 환경에서는 웹 알림 API를 사용할 수 없습니다.",
      requestsWebOnly: "데스크톱 알림 요청은 웹 런타임에서만 사용할 수 있습니다.",
      requestUnavailable: "웹 알림 API의 requestPermission()을 사용할 수 없습니다.",
      requestFailed: "알림 권한을 요청하지 못했습니다: {{message}}",
      unexpectedState: "예기치 않은 알림 권한 상태: {{state}}",
    },
    microphone: {
      webOnly: "데스크톱 마이크 상태는 웹 런타임에서만 사용할 수 있습니다.",
      navigatorUnavailable: "이 환경에서는 Navigator를 사용할 수 없습니다.",
      granted: "마이크 접근이 허용되었습니다.",
      denied: "시스템 설정에서 마이크 접근이 거부되어 있습니다.",
      notGranted: "마이크 권한이 아직 허용되지 않았습니다.",
      unexpectedState: "예기치 않은 마이크 권한 상태: {{state}}",
      statusApiUnavailable:
        "이 런타임에서는 마이크 상태 API를 사용할 수 없습니다. 접근을 확인하려면 요청을 사용하세요.",
      queryFailed: "마이크 상태를 조회하지 못했습니다: {{message}}",
      captureUnavailable: "이 환경에서는 마이크 캡처를 사용할 수 없습니다.",
      permissionApiUnavailable:
        "권한 상태 API를 사용할 수 없습니다. 접근을 확인하려면 요청을 사용하세요.",
      requestsWebOnly: "데스크톱 마이크 요청은 웹 런타임에서만 사용할 수 있습니다.",
      captureApiUnavailable: "이 환경에서는 마이크 캡처 API를 사용할 수 없습니다.",
      requestDenied: "사용자 또는 시스템에 의해 마이크 권한이 거부되었습니다.",
      noDevice: "마이크 장치를 찾을 수 없습니다.",
      requestFailed: "마이크 권한을 요청하지 못했습니다: {{message}}",
    },
    empty: {
      notifications: "알림 상태를 아직 확인하지 않았습니다.",
      microphone: "마이크 상태를 아직 확인하지 않았습니다.",
    },
    testNotification: {
      title: "Ait 알림 테스트",
      body: "이 메시지가 보이면 데스크톱 알림이 작동하는 것입니다.",
      notDelivered: "알림이 전달되지 않았습니다. 시스템 설정 > 알림을 확인하세요.",
      failed: "알림을 보내지 못했습니다.",
    },
  },
};
