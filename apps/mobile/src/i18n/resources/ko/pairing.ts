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
  },
  direct: {
    title: "직접 연결",
    helper: "서버 주소와 액세스 토큰을 입력하세요.",
    fields: {
      host: "호스트",
      port: "포트",
      password: "서버 토큰",
      optional: "필수",
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
    helper: "SSH를 통해 원격 Ait 서버에 연결합니다(기본 서버 포트: 7316).",
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
};
