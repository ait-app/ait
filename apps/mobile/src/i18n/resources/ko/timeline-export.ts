export const timelineExport = {
  title: "세션 내보내기",
  description:
    "저장된 기록만 포함합니다. 대화에는 사용자·어시스턴트 메시지와 음성 전사가 포함됩니다. 타임라인에는 저장된 카드도 포함됩니다. 최대 16 MiB.",
  conversation: "대화만",
  timeline: "저장된 전체 타임라인",
  download: "내보내기 및 다운로드",
  cancel: "내보내기 취소",
  unavailable: "내보내기를 사용할 수 없습니다. 지원하는 호스트에 연결하세요.",
  tooLarge: "내보내기가 16 MiB 제한을 초과합니다. 내용은 잘리지 않았습니다.",
  browserHandoff: "브라우저로 전송했습니다. 브라우저 다운로드 목록에서 저장을 완료하세요.",
};

export const nativeHtmlExport = {
  title: "네이티브 HTML 내보내기 (OMP)",
  description:
    "Oh My Pi가 자체 포매터로 이 세션을 렌더링합니다. 파일에는 활성 스크립트가 포함될 수 있으며 다운로드 전용으로 저장됩니다. Ait 내부에서 열리지 않습니다. 최대 16 MiB.",
  download: "네이티브 HTML 내보내기",
  risk: "네이티브 형식에는 스크립트가 포함되어 있으며 열 때 외부 뷰어(stencil.so)를 로드할 수 있습니다. 뷰어는 파일에 포함된 세션 기록을 읽을 수 있습니다. 다운로드 전용이며 Ait는 미리 보거나 열지 않습니다. 비활성 일반 HTML 내보내기와 다릅니다.",
  unavailable: "네이티브 HTML 내보내기를 사용할 수 없습니다. 지원하는 OMP 호스트에 연결하세요.",
};
