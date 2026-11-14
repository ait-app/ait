export const pairing = {
  connectionMethods: {
    title: "연결 추가",
    direct: {
      title: "직접 연결",
      description: "로컬 네트워크 또는 VPN.",
    },
    remoteSsh: {
      title: "원격 SSH",
      description: "데스크톱 SSH 클라이언트를 통해 연결합니다.",
    },
    scanQr: {
      title: "QR 코드 스캔",
      description: "암호화된 릴레이 연결.",
    },
    pasteLink: {
      title: "페어링 링크 붙여넣기",
      description: "암호화된 릴레이 연결.",
    },
  },
  direct: {
    title: "직접 연결",
    helper: "Ait 서버의 주소를 입력하세요.",
    fields: {
      host: "호스트",
      port: "포트",
      password: "비밀번호",
      optional: "선택 사항",
      useSsl: "SSL 사용",
      connectionUri: "연결 URI",
    },
    advanced: {
      label: "고급",
      show: "고급 표시",
      hide: "고급 숨기기",
    },
    passwordVisibility: {
      show: "비밀번호 표시",
      hide: "비밀번호 숨기기",
    },
    actions: {
      cancel: "취소",
      connect: "연결",
      connecting: "연결 중...",
    },
    errors: {
      hostRequired: "호스트를 입력하세요",
      invalidPort: "포트는 1에서 65535 사이여야 합니다",
      invalidConnection: "잘못된 연결",
      failedTitle: "연결 실패",
      failedToConnect: "{{endpoint}}에 연결하지 못했습니다.",
      noAdditionalDetails: "{{detail}} (추가 세부 정보 없음)",
      timedOut: "연결 시간이 초과되었습니다. 호스트/포트와 네트워크를 확인하세요.",
      refused: "연결이 거부되었습니다. 이 주소에서 서버가 실행 중입니까?",
      hostNotFound: "호스트를 찾을 수 없습니다. 호스트 이름을 확인하고 다시 시도하세요.",
      hostUnreachable: "호스트에 연결할 수 없습니다. 네트워크와 방화벽을 확인하세요.",
      tlsError: "TLS 오류. 직접 연결은 데몬 앞에 TLS 종단 장치가 있을 때만 SSL을 사용합니다.",
      unableToConnect: "연결할 수 없습니다. 호스트/포트와 데몬에 접근 가능한지 확인하세요.",
      details: "세부 정보: {{detail}}",
    },
  },
  remoteSsh: {
    title: "원격 SSH",
    helper: "원격 호스트에서 실행 중인 Ait 데몬에 연결합니다.",
    fields: {
      target: "SSH 호스트",
    },
    actions: {
      cancel: "취소",
      connect: "연결",
      connecting: "연결 중...",
    },
    errors: {
      tokenRequired: "원격 Ait 서버 토큰(32–256자)을 입력하세요.",
      targetRequired: "SSH 호스트가 필요합니다",
      invalidTarget: "유효한 ssh:// 호스트를 입력하세요",
      failedToConnect: "SSH로 연결할 수 없습니다. {{detail}}",
    },
  },
  link: {
    title: "페어링 링크 붙여넣기",
    helper: "서버에서 받은 페어링 링크를 붙여넣으세요.",
    label: "페어링 링크",
    errors: {
      required: "페어링 링크를 붙여넣으세요 (.../#offer=...)",
      missingOffer: "링크에 #offer=...가 포함되어야 합니다",
      emptyOffer: "Offer 페이로드가 비어 있습니다",
      invalid: "잘못된 페어링 링크",
      unableToPair: "호스트를 페어링할 수 없습니다",
    },
    alert: {
      failedTitle: "페어링 실패",
    },
    actions: {
      cancel: "취소",
      pair: "페어링",
      pairing: "페어링 중...",
    },
  },
  scan: {
    title: "QR 스캔",
    webUnavailableTitle: "웹에서 사용할 수 없습니다",
    webUnavailableBody:
      '웹 빌드에서는 QR 스캔이 지원되지 않습니다. 대신 "링크 붙여넣기"를 사용하세요.',
    backToSettings: "설정으로 돌아가기",
    cameraPermissionTitle: "카메라 권한",
    cameraPermissionBody: "데몬에서 페어링 QR 코드를 스캔하려면 카메라 접근을 허용하세요.",
    grantPermission: "권한 허용",
    pairing: "페어링 중...",
    unableToPair: "호스트를 페어링할 수 없습니다",
    errorTitle: "오류",
  },
  device: {
    loadingOffer: "페어링 정보 불러오는 중...",
    failedToLoadOffer: "페어링 정보를 불러오지 못했습니다.",
    relayDisabled: "릴레이가 활성화되어 있지 않습니다. 기기를 페어링하려면 릴레이를 활성화하세요.",
    enableTitle: "릴레이를 활성화하시겠습니까?",
    enableDescription:
      "릴레이를 사용하면 이 장치를 어디에서나 연결할 수 있습니다. 페어링 트래픽은 종단 간 암호화됩니다.",
    relayDocs: "릴레이 작동 방식",
    relayDocsAccessibility: "Ait 릴레이 작동 방식 읽기",
    enableRelay: "릴레이 활성화",
    enablingRelay: "활성화 중...",
    notNow: "지금은 아님",
    directConnectionHint:
      "릴레이 없이 TCP, Tailscale 또는 다른 VPN을 통해 직접 연결하세요. QR 코드가 생성되지 않습니다.",
    updateRequired: "Ait 데스크톱에서 릴레이를 활성화하려면 호스트를 업데이트하세요.",
    unavailable: "페어링 정보를 사용할 수 없습니다.",
    hint: "휴대폰의 Ait로 이 QR 코드를 스캔하거나 아래 링크를 복사하세요.",
    securityWarning:
      "이 페어링 링크는 비밀번호처럼 취급하세요. 링크를 가진 사람은 누구나 이 데몬에 접근할 수 있습니다.",
    qrUnavailable: "QR 코드를 사용할 수 없습니다.",
    qrAccessibility: "페어링 QR 코드",
    retry: "다시 시도",
    copy: "복사",
    copied: "복사됨",
  },
};
