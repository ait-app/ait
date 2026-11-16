const sharedSharing = {
  sharedWithMeCount: "Compartilhado comigo ({{count}})",
  incomingHostHint:
    "Hosts ativados e seus projetos e workspaces compartilhados aparecem nas suas listas.",
  showHostInHosts: "Mostrar {{host}} em Hosts",
  sharedByMeCount: "Compartilhado por mim ({{count}})",
  action: "Compartilhar com grupos",
  manage: "Gerenciar",
  refresh: "Atualizar",
  stop: "Parar compartilhamento",
  active: "Ativo",
  loading: "Carregando recursos compartilhados…",
  hostSection: "Compartilhamento",
  hostTitle: "Compartilhar este nó",
  hostHint: "Dê a um grupo acesso a todos os projetos e workspaces neste host.",
  shareTitle: "Compartilhar {{name}}",
  resourceTitle: "Compartilhar recurso",
  intro: "Membros do grupo podem abrir este recurso no AIT enquanto o proprietário estiver online.",
  projectHostHint: "Compartilhe este projeto e seus workspaces no host selecionado.",
  hostUnavailable: "Este host está indisponível. Conecte-se para gerenciar o compartilhamento.",
  scope: { node: "Nó", project: "Projeto", workspace: "Workspace" },
  scopeHint: {
    node: "Tudo neste nó",
    project: "Este projeto e seus workspaces",
    workspace: "Somente este workspace",
  },
  status: {
    shared: "Compartilhado",
    rejected: "Não compartilhado · {{code}}",
    waiting: "Aguardando o serviço",
    groupAdmin: "Você gerencia este grupo",
    groupMember: "Membro do grupo",
  },
  noGroupsTitle: "Nenhum grupo disponível",
  noGroupsHint:
    "Conecte este nó ao serviço online em Configurações do host → Conexões e verifique se o proprietário pertence a um grupo.",
  nodeSettingsOnly:
    "Estas configurações exigem o compartilhamento do nó inteiro. Este compartilhamento dá acesso apenas ao projeto ou workspace.",
  ownerOnly:
    "Somente o proprietário do nó pode gerenciar o compartilhamento. Recursos compartilhados com você não podem ser compartilhados novamente.",
  unsupported:
    "Este nó não oferece gerenciamento de compartilhamento. Atualize-o e tente novamente.",
  loadError: "Não foi possível carregar as configurações de compartilhamento.",
  updateError: "Não foi possível atualizar o compartilhamento.",
  stopError: "Não foi possível parar o compartilhamento.",
  emptyIncomingTitle: "Nada foi compartilhado com você ainda",
  emptyIncomingHint: "Recursos compartilhados por membros dos seus grupos aparecerão aqui.",
  emptyOutgoingTitle: "Você não está compartilhando nada",
  emptyOutgoingHint:
    "Use o menu de um workspace ou projeto, ou abra as configurações do host para compartilhar um nó inteiro.",
} as const;

export const sharing = {
  ...sharedSharing,
  title: "Compartilhados",
  listError: "Não foi possível carregar os recursos compartilhados.",
};
