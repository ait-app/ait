export const scheduleTemplatePrompts = {
  "nightly-test-heal": {
    prompt: `Verifique a saúde dos testes deste workspace e corrija um defeito claramente identificado quando as evidências sustentarem isso.

Antes de executar qualquer coisa, leia as instruções do repositório e os scripts de teste. Siga o escopo exigido, incluindo os limites para executar toda a suíte. Verifique a árvore de trabalho; se houver alterações não commitadas, registre isso e encerre esta execução.

Siga estas etapas:
1. Execute as verificações permitidas. Registre cada comando, seu código de saída e a saída de erro necessária para localizar o problema.
2. Se conseguir confirmar um defeito no código, faça uma correção focada em uma branch separada. Preserve o significado das asserções existentes e deixe o código não relacionado intacto.
3. Execute novamente as verificações afetadas e examine o diff final. Diferencie falhas do ambiente ou de serviços externos de defeitos no código.

Retorne um registro curto: escopo, descobertas, arquivos alterados, resultados da verificação e itens não resolvidos. Siga as regras do repositório e as autorizações existentes para commits, pushes e pull requests.`,
  },
  "pr-gatekeeper": {
    prompt: `Prepare um relatório de risco de merge para este repositório.

Revise os pull requests abertos atualizados nas últimas 24 horas. Às segundas-feiras, inclua as atualizações desde sexta-feira. Leia cada diff relevante e código suficiente ao redor para entender o comportamento afetado. Se os dados dos PRs não estiverem disponíveis, identifique o acesso ou material ausente.

Procure falhas observáveis de comportamento, segurança, tratamento de dados e compatibilidade. Para cada descoberta, informe o número do PR, arquivo e local, um cenário reproduzível, seu impacto e uma etapa de verificação sugerida. Ordene as descobertas por impacto e não inclua preferências de estilo.

Finalize com a lista de PRs revisados e quaisquer lacunas de cobertura. Diga quando nenhuma descoberta apoiada por evidências for identificada. Mantenha esta execução somente leitura: não edite o código nem publique revisões ou comentários.`,
  },
  "domain-radar": {
    prompt: `Prepare um briefing curto sobre “{{topic}}” que o leitor consiga terminar em poucos minutos.

Pesquise na web material publicado ou atualizado de forma relevante nas últimas 24 horas; às segundas-feiras, verifique também o fim de semana. Prefira material primário rastreável, como anúncios de projetos, notas de versão e artigos originais. Combine relatos duplicados do mesmo evento.

Selecione até cinco novidades diretamente relevantes para o tema. Para cada uma, explique o que aconteceu, informe a data de publicação, descreva o efeito prático para usuários ou desenvolvedores e inclua o link da fonte original. Separe fatos estabelecidos da sua interpretação e destaque evidências conflitantes.

Termine com uma ou duas perguntas ou novidades para acompanhar. Se houver pouca notícia útil, diga isso e mantenha o briefing curto em vez de preenchê-lo com material antigo ou especulativo.`,
    params: {
      topic: {
        label: "Tema do briefing",
        placeholder:
          "ex.: lançamentos do Postgres, ferramentas de programação com IA, pesquisa de baterias",
      },
    },
  },
  "weekly-work-draft": {
    prompt: `Crie um resumo editável dos últimos sete dias de trabalho neste workspace.

Colete evidências de commits, pull requests relevantes, documentação alterada e notas de projeto disponíveis. Agrupe mudanças relacionadas por resultado para que uma série de commits pequenos não vire um longo registro de atividades. Não deduza responsáveis, conclusão ou planos apenas pelos nomes das branches.

Organize o rascunho assim:
- Entregue: resultados concluídos e por que são importantes.
- Ainda aberto: trabalho inacabado, bloqueios e limitações conhecidas.
- Próximos passos sugeridos: ações apoiadas pelas evidências atuais, marcadas claramente como sugestões.
- Precisa de confirmação: contexto ou decisões que o usuário deve completar.

Vincule cada ponto relevante a um commit, PR ou arquivo. Se uma fonte não puder ser acessada, informe a lacuna. Retorne apenas o rascunho; deixe os arquivos e o estado do repositório intactos.`,
  },
  "docs-freshness": {
    prompt: `Revise a documentação do workspace em busca de instruções que possam falhar ou induzir o leitor ao erro hoje.

Comece pelo README e pelo índice da documentação; depois, percorra os guias vinculados de configuração, uso e manutenção. Compare comandos, caminhos, campos de configuração e exemplos de API citados com o código atual. Use o comando documentado de verificação de links do repositório quando estiver disponível e for permitido.

Priorize problemas verificáveis: destinos ausentes, comandos obsoletos, instruções conflitantes e exemplos que já não correspondam à implementação. Trate registros históricos datados como história, a menos que ainda sejam apresentados como instruções atuais.

Para cada problema, informe o caminho e o título ou linha do documento, o código ou resultado de verificação que o sustenta, a provável falha do leitor e uma correção concreta. Ordene a lista por impacto e termine com o que não pôde verificar. Produza um relatório sem editar os documentos.`,
  },
} as const;
