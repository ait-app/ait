export const settingsLayout = {
  openInSidePane: {
    title: "위치 열기",
    destinations: { main: "기본 패널", side: "측면", explorer: "탐색기 사이드바" },
    sources: {
      explorerFiles: {
        label: "탐색기에서 파일 선택",
        description: "탐색기 사이드바에서 선택한 파일을 작업 옆에 열기",
      },
      diffs: { label: "diff 열기", description: "탐색기와 에이전트 대화의 diff를 작업 옆에 열기" },
      chatFiles: {
        label: "에이전트 채팅에서 파일 열기",
        description: "파일 링크와 도구 호출 파일을 대화 옆에 열기",
      },
      diffFiles: {
        label: "변경 사항에서 파일 열기",
        description: "diff에서 선택한 소스 파일을 옆에 열기",
      },
      subagents: {
        label: "하위 에이전트 열기",
        description: "하위 에이전트를 상위 에이전트 옆에 열기",
      },
      pullRequests: {
        label: "변경 사항에서 풀 리퀘스트 열기",
        description: "풀 리퀘스트 세부 정보를 변경 사항 옆에 열기",
      },
    },
  },
} as const;
