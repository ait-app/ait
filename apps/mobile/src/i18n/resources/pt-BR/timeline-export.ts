export const timelineExport = {
  title: "Exportar sessão",
  description:
    "Somente o histórico salvo. A conversa inclui mensagens do usuário, do assistente e transcrições de voz. A linha do tempo também inclui cartões salvos. Máximo de 16 MiB.",
  conversation: "Somente conversa",
  timeline: "Toda a linha do tempo salva",
  download: "Exportar e baixar",
  cancel: "Cancelar exportação",
  unavailable: "Exportação indisponível. Conecte-se a um host compatível.",
  tooLarge: "A exportação excede o limite de 16 MiB. Nenhum conteúdo foi truncado.",
  browserHandoff: "Enviado ao navegador — confira os downloads para concluir o salvamento.",
};

export const nativeHtmlExport = {
  title: "Exportação HTML nativa (OMP)",
  description:
    "O Oh My Pi renderiza esta sessão com o formatador dele. O arquivo pode conter scripts ativos e é salvo apenas como download — nunca é aberto dentro do Ait. Máximo de 16 MiB.",
  download: "Exportar HTML nativo",
  risk: "O formato nativo contém scripts e pode carregar um visualizador externo (stencil.so) ao ser aberto; esse visualizador pode ler o histórico da sessão incorporado no arquivo. Somente download — o Ait nunca o pré-visualiza ou abre. Não é a exportação HTML genérica inerte.",
  unavailable: "Exportação HTML nativa indisponível. Conecte-se a um host OMP compatível.",
};
