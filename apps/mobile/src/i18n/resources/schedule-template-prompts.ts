export const scheduleTemplatePromptsAr = {
  "nightly-test-heal": {
    prompt: `تحقق من صحة اختبارات مساحة العمل وأصلح عيبًا واحدًا محددًا بوضوح عندما تدعم الأدلة ذلك.

قبل تشغيل أي شيء، اقرأ تعليمات المستودع ونصوص الاختبار. التزم بنطاق الاختبارات المطلوب، بما في ذلك أي قيود على تشغيل المجموعة كاملة. افحص شجرة العمل؛ إذا احتوت على تغييرات غير ملتزم بها، سجّل ذلك وأنهِ هذه الجولة.

نفّذ الخطوات التالية:
1. شغّل الفحوصات المسموح بها. سجّل كل أمر وحالة خروجه ومخرجات الفشل اللازمة لتحديد المشكلة.
2. إذا ثبت وجود عيب في الكود، أجرِ إصلاحًا مركزًا في فرع منفصل. حافظ على معنى التأكيدات الحالية واترك الكود غير المرتبط كما هو.
3. أعد تشغيل الفحوصات المتأثرة وافحص الفرق النهائي. ميّز بين أعطال البيئة أو الخدمات الخارجية وعيوب الكود.

أعد سجلًا موجزًا للجولة: النطاق والنتائج والملفات المتغيرة ونتائج التحقق والعناصر غير المحسومة. اتبع قواعد المستودع والتفويضات الحالية للالتزامات والدفع وطلبات السحب.`,
  },
  "pr-gatekeeper": {
    prompt: `أعد تقريرًا عن مخاطر الدمج لهذا المستودع.

راجع طلبات السحب المفتوحة التي حُدّثت خلال آخر 24 ساعة. في يوم الاثنين، أدرج التحديثات منذ يوم الجمعة. اقرأ كل فرق ذي صلة وما يكفي من الكود المحيط لفهم السلوك المتأثر. إذا لم تتوفر بيانات طلبات السحب، حدّد الوصول أو المادة المفقودة.

ابحث عن إخفاقات يمكن ملاحظتها في السلوك والأمان ومعالجة البيانات والتوافق. لكل نتيجة، اذكر رقم طلب السحب والملف والموقع وسيناريو فشل قابلًا لإعادة الإنتاج وأثره وخطوة تحقق مقترحة. رتّب النتائج حسب الأثر، واستبعد التفضيلات الأسلوبية من التقرير.

اختم بقائمة طلبات السحب التي راجعتها وأي فجوات في التغطية. اذكر عندما لا تجد نتائج مدعومة. أبقِ هذه الجولة للقراءة فقط: لا تعدّل الكود ولا تنشر مراجعات أو تعليقات.`,
  },
  "domain-radar": {
    prompt: `أعد إحاطة قصيرة عن «{{topic}}» يستطيع القارئ إنهاءها خلال بضع دقائق.

ابحث في الويب عن مواد نُشرت أو حُدّثت جوهريًا خلال آخر 24 ساعة؛ وفي يوم الاثنين تحقّق من عطلة نهاية الأسبوع أيضًا. فضّل المصادر الأولية القابلة للتتبع مثل إعلانات المشاريع وملاحظات الإصدار والأوراق الأصلية. ادمج التقارير المكررة عن الحدث نفسه.

اختر حتى خمسة مستجدات مرتبطة مباشرة بالموضوع. لكل مستجد، اشرح ما حدث، واذكر تاريخ النشر، وصف أثره العملي على المستخدمين أو المطورين، واربط بالمصدر الأصلي. افصل الحقائق المؤكدة عن تفسيرك، واذكر الأدلة المتعارضة.

اختم بسؤال أو سؤالين أو مستجدات ينبغي متابعتها لاحقًا. إذا كان هناك القليل من الأخبار المفيدة، فاذكر ذلك واجعل الإحاطة قصيرة بدل حشوها بمواد أقدم أو تخمينية.`,
    params: {
      topic: {
        label: "موضوع الإحاطة",
        placeholder: "مثل: إصدارات Postgres، أدوات برمجة الذكاء الاصطناعي، أبحاث البطاريات",
      },
    },
  },
  "weekly-work-draft": {
    prompt: `أنشئ ملخصًا قابلًا للتحرير لعمل الأيام السبعة الماضية في مساحة العمل هذه.

اجمع الأدلة من الالتزامات وطلبات السحب ذات الصلة والوثائق المتغيرة وملاحظات المشروع المتاحة. اجمع التغييرات المرتبطة حسب النتيجة حتى لا تتحول سلسلة من الالتزامات الصغيرة إلى سجل نشاط طويل. لا تستنتج الملكية أو الإكمال أو الخطط من أسماء الفروع وحدها.

نظّم المسودة إلى:
- تم التسليم: النتائج المكتملة وسبب أهميتها.
- ما يزال مفتوحًا: العمل غير المكتمل والعوائق والقيود المعروفة.
- الخطوات التالية المقترحة: إجراءات تدعمها الأدلة الحالية مع تمييزها بوضوح كاقتراحات.
- يحتاج إلى تأكيد: السياق أو القرارات الناقصة التي يملؤها المستخدم.

اربط كل نقطة جوهرية بالتزام أو طلب سحب أو ملف. إذا تعذر الوصول إلى مصدر، اذكر الفجوة. أعد المسودة فقط؛ واترك ملفات مساحة العمل وحالة المستودع دون تغيير.`,
  },
  "docs-freshness": {
    prompt: `راجع وثائق مساحة العمل بحثًا عن تعليمات قد تفشل أو تضلل القارئ اليوم.

ابدأ بملف README وفهرس الوثائق، ثم تتبّع أدلة الإعداد والاستخدام والصيانة المرتبطة. قارن الأوامر والمسارات وحقول الإعداد وأمثلة API المشار إليها بالكود الحالي. استخدم أمر فحص الروابط الموثق في المستودع عندما يكون متاحًا ومسموحًا به.

أعطِ الأولوية للمشكلات القابلة للتحقق: الأهداف المفقودة والأوامر القديمة والتعليمات المتعارضة والأمثلة التي لم تعد تطابق التنفيذ. تعامل مع السجلات التاريخية المؤرخة كتاريخ ما لم تُعرض كتعليمات حالية.

لكل مشكلة، اذكر مسار المستند والعنوان أو السطر، والكود الداعم أو نتيجة الفحص، وإخفاق القارئ المحتمل، وتصحيحًا ملموسًا. رتّب القائمة حسب الأثر واختم بما تعذر التحقق منه. أعد تقرير مراجعة دون تعديل الوثائق.`,
  },
} as const;

export const scheduleTemplatePromptsEs = {
  "nightly-test-heal": {
    prompt: `Comprueba la salud de las pruebas de este espacio de trabajo y corrige un defecto claramente identificado cuando las pruebas lo respalden.

Antes de ejecutar nada, lee las instrucciones del repositorio y los scripts de pruebas. Sigue el alcance requerido, incluidos los límites para ejecutar toda la suite. Comprueba el árbol de trabajo; si contiene cambios sin confirmar, regístralo y termina esta ejecución.

Sigue estos pasos:
1. Ejecuta las comprobaciones permitidas. Registra cada comando, su código de salida y la salida de error necesaria para localizar el problema.
2. Si puedes demostrar que existe un defecto de código, haz una corrección concreta en una rama separada. Mantén el sentido de las aserciones existentes y deja intacto el código no relacionado.
3. Vuelve a ejecutar las comprobaciones afectadas y revisa el diff final. Distingue los fallos del entorno o de servicios externos de los defectos del código.

Devuelve un registro breve: alcance, hallazgos, archivos modificados, resultados de verificación y asuntos pendientes. Sigue las reglas del repositorio y la autorización existente para commits, pushes y pull requests.`,
  },
  "pr-gatekeeper": {
    prompt: `Prepara un informe de riesgos de merge para este repositorio.

Revisa los pull requests abiertos actualizados en las últimas 24 horas. Los lunes, incluye las actualizaciones desde el viernes. Lee cada diff relevante y suficiente código alrededor para entender el comportamiento afectado. Si no hay datos de PR, identifica el acceso o material que falta.

Busca fallos observables de comportamiento, seguridad, gestión de datos y compatibilidad. Para cada hallazgo, indica el número de PR, archivo y ubicación, un escenario reproducible, su impacto y un paso de verificación sugerido. Ordena los hallazgos por impacto y excluye preferencias de estilo.

Termina con la lista de PR revisados y cualquier hueco de cobertura. Indica cuando no haya hallazgos respaldados. Mantén esta ejecución en solo lectura: no edites código ni publiques revisiones o comentarios.`,
  },
  "domain-radar": {
    prompt: `Prepara un informe breve sobre «{{topic}}» que el lector pueda terminar en unos minutos.

Busca en la web material publicado o actualizado de forma sustancial en las últimas 24 horas; los lunes, revisa también el fin de semana. Prefiere fuentes primarias trazables, como anuncios de proyectos, notas de versión y artículos originales. Combina los informes duplicados del mismo evento.

Selecciona hasta cinco novedades directamente relevantes para el tema. Para cada una, explica qué ocurrió, indica la fecha de publicación, describe su efecto práctico para usuarios o desarrolladores y enlaza la fuente original. Separa los hechos establecidos de tu interpretación y señala las pruebas contradictorias.

Termina con una o dos preguntas o novedades que convenga seguir. Si hay pocas noticias útiles, dilo y mantén el informe corto en vez de rellenarlo con material antiguo o especulativo.`,
    params: {
      topic: {
        label: "Tema del informe",
        placeholder:
          "p. ej., versiones de Postgres, herramientas de programación con IA, investigación de baterías",
      },
    },
  },
  "weekly-work-draft": {
    prompt: `Crea un resumen editable de los siete últimos días de trabajo en este espacio de trabajo.

Recopila evidencias de commits, pull requests relevantes, documentación modificada y notas de proyecto disponibles. Agrupa los cambios relacionados por resultado para que una serie de commits pequeños no se convierta en un registro interminable. No deduzcas responsables, finalización ni planes solo por los nombres de las ramas.

Organiza el borrador así:
- Entregado: resultados completados y por qué importan.
- Sigue abierto: trabajo inacabado, bloqueos y limitaciones conocidas.
- Próximos pasos sugeridos: acciones respaldadas por la evidencia actual, marcadas claramente como sugerencias.
- Necesita confirmación: contexto o decisiones que el usuario debe completar.

Enlaza cada punto sustancial con un commit, PR o archivo. Si no puedes acceder a una fuente, indica la laguna. Devuelve solo el borrador; no cambies los archivos ni el estado del repositorio.`,
  },
  "docs-freshness": {
    prompt: `Revisa la documentación del espacio de trabajo en busca de instrucciones que hoy pudieran fallar o confundir al lector.

Empieza por el README y el índice de documentación; después, sigue las guías enlazadas de configuración, uso y mantenimiento. Compara los comandos, rutas, campos de configuración y ejemplos de API con el código actual. Usa el comando documentado de comprobación de enlaces cuando esté disponible y permitido.

Prioriza problemas verificables: destinos inexistentes, comandos obsoletos, instrucciones contradictorias y ejemplos que ya no coincidan con la implementación. Trata los registros históricos fechados como historia salvo que se presenten como instrucciones actuales.

Para cada problema, indica la ruta y el título o línea del documento, el código o resultado que lo respalda, el posible fallo del lector y una corrección concreta. Ordena la lista por impacto y termina con lo que no hayas podido verificar. Produce un informe sin editar los documentos.`,
  },
} as const;

export const scheduleTemplatePromptsFr = {
  "nightly-test-heal": {
    prompt: `Vérifiez l’état des tests de cet espace de travail et corrigez un défaut clairement identifié lorsque les éléments disponibles le justifient.

Avant toute commande, lisez les instructions du dépôt et les scripts de test. Respectez le périmètre requis, notamment les restrictions sur l’exécution de toute la suite. Vérifiez l’arbre de travail ; s’il contient des modifications non validées, notez-le et arrêtez cette exécution.

Procédez ainsi :
1. Exécutez les vérifications autorisées. Notez chaque commande, son code de sortie et la sortie d’erreur nécessaire pour localiser le problème.
2. Si vous établissez qu’il s’agit d’un défaut du code, faites une correction ciblée sur une branche séparée. Préservez le sens des assertions existantes et laissez le code sans rapport inchangé.
3. Relancez les vérifications concernées et examinez le diff final. Distinguez les problèmes d’environnement ou de services externes des défauts du code.

Retournez un compte rendu court : périmètre, résultats, fichiers modifiés, vérifications et points non résolus. Suivez les règles du dépôt et les autorisations existantes pour les commits, pushes et pull requests.`,
  },
  "pr-gatekeeper": {
    prompt: `Préparez un rapport sur les risques de fusion pour ce dépôt.

Examinez les pull requests ouvertes mises à jour au cours des dernières 24 heures. Le lundi, incluez les mises à jour depuis vendredi. Lisez chaque diff pertinent et suffisamment de code autour pour comprendre le comportement concerné. Si les données des PR sont indisponibles, indiquez l’accès ou le contenu manquant.

Recherchez les échecs observables de comportement, de sécurité, de traitement des données et de compatibilité. Pour chaque constat, donnez le numéro de PR, le fichier et l’emplacement, un scénario reproductible, son impact et une étape de vérification proposée. Classez les constats par impact et excluez les préférences stylistiques.

Terminez par la liste des PR examinées et les éventuelles lacunes de couverture. Dites lorsqu’aucun constat étayé n’a été trouvé. Gardez cette exécution en lecture seule : ne modifiez pas le code et ne publiez ni revue ni commentaire.`,
  },
  "domain-radar": {
    prompt: `Préparez une courte synthèse sur « {{topic}} » qu’un lecteur peut parcourir en quelques minutes.

Recherchez sur le web les contenus publiés ou substantiellement mis à jour au cours des dernières 24 heures ; le lundi, consultez aussi le week-end. Privilégiez les sources primaires traçables, comme les annonces de projets, les notes de version et les articles originaux. Regroupez les comptes rendus dupliqués d’un même événement.

Sélectionnez jusqu’à cinq nouveautés directement liées au sujet. Pour chacune, expliquez ce qui s’est passé, donnez la date de publication, décrivez l’effet pratique pour les utilisateurs ou les développeurs et liez la source originale. Séparez les faits établis de votre interprétation et signalez les éléments contradictoires.

Terminez par une ou deux questions ou évolutions à suivre. S’il y a peu d’actualités utiles, dites-le et restez bref au lieu de remplir la synthèse avec des informations anciennes ou spéculatives.`,
    params: {
      topic: {
        label: "Sujet de la synthèse",
        placeholder:
          "ex. versions de Postgres, outils de programmation IA, recherche sur les batteries",
      },
    },
  },
  "weekly-work-draft": {
    prompt: `Créez un bilan modifiable des sept derniers jours de travail dans cet espace.

Rassemblez les éléments des commits, des pull requests pertinentes, de la documentation modifiée et des notes de projet disponibles. Regroupez les changements liés par résultat afin qu’une série de petits commits ne devienne pas un long journal d’activité. N’inférez ni responsable, ni achèvement, ni plan à partir des seuls noms de branches.

Structurez le brouillon ainsi :
- Livré : résultats terminés et leur importance.
- Encore ouvert : travail inachevé, blocages et limites connues.
- Prochaines étapes proposées : actions étayées par les éléments actuels, clairement marquées comme suggestions.
- À confirmer : contexte ou décisions que l’utilisateur doit compléter.

Liez chaque point substantiel à un commit, une PR ou un fichier. Si une source est inaccessible, signalez la lacune. Retournez uniquement le brouillon ; ne modifiez ni les fichiers ni l’état du dépôt.`,
  },
  "docs-freshness": {
    prompt: `Examinez la documentation de l’espace de travail pour trouver les instructions qui pourraient aujourd’hui échouer ou induire le lecteur en erreur.

Commencez par le README et l’index de documentation, puis suivez les guides liés de configuration, d’utilisation et de maintenance. Comparez les commandes, chemins, champs de configuration et exemples d’API cités avec le code actuel. Utilisez la commande de vérification des liens documentée par le dépôt lorsqu’elle est disponible et autorisée.

Priorisez les problèmes vérifiables : cibles manquantes, commandes obsolètes, instructions contradictoires et exemples qui ne correspondent plus à l’implémentation. Considérez les documents historiques datés comme de l’historique, sauf s’ils sont encore présentés comme des instructions actuelles.

Pour chaque problème, indiquez le chemin et le titre ou la ligne du document, le code ou résultat de vérification à l’appui, l’échec probable du lecteur et une correction concrète. Classez la liste par impact et terminez par ce que vous n’avez pas pu vérifier. Produisez un rapport sans modifier les documents.`,
  },
} as const;

export const scheduleTemplatePromptsPtBR = {
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

export const scheduleTemplatePromptsRu = {
  "nightly-test-heal": {
    prompt: `Проверьте состояние тестов этого рабочего пространства и исправьте один явно выявленный дефект, если это подтверждается фактами.

Перед запуском чего-либо прочитайте инструкции репозитория и скрипты тестов. Соблюдайте требуемый объём проверок, включая ограничения на запуск полного набора. Проверьте рабочее дерево; если в нём есть незакоммиченные изменения, зафиксируйте это и завершите запуск.

Выполните следующие шаги:
1. Запустите разрешённые проверки. Запишите каждую команду, её код завершения и необходимый вывод ошибки для поиска проблемы.
2. Если удаётся подтвердить дефект кода, внесите точечное исправление в отдельной ветке. Сохраните смысл существующих утверждений и не меняйте несвязанный код.
3. Повторно запустите затронутые проверки и изучите итоговый diff. Отличайте проблемы окружения или внешних сервисов от дефектов кода.

Верните краткий отчёт: область проверки, результаты, изменённые файлы, итоги верификации и нерешённые вопросы. Соблюдайте правила репозитория и действующие разрешения для коммитов, push и pull request.`,
  },
  "pr-gatekeeper": {
    prompt: `Подготовьте отчёт о рисках слияния для этого репозитория.

Проверьте открытые pull request, обновлённые за последние 24 часа. По понедельникам включите обновления с пятницы. Изучите каждый релевантный diff и окружающий код в объёме, достаточном для понимания затронутого поведения. Если данные PR недоступны, укажите отсутствующий доступ или материал.

Ищите наблюдаемые проблемы в поведении, безопасности, обработке данных и совместимости. Для каждого вывода укажите номер PR, файл и место, воспроизводимый сценарий сбоя, его влияние и предлагаемую проверку. Ранжируйте выводы по влиянию и не включайте стилистические предпочтения.

Завершите списком проверенных PR и пробелами в покрытии. Скажите, если подтверждённых выводов нет. Оставьте этот запуск только для чтения: не изменяйте код и не публикуйте ревью или комментарии.`,
  },
  "domain-radar": {
    prompt: `Подготовьте краткую сводку по теме «{{topic}}», которую читатель сможет просмотреть за несколько минут.

Ищите в интернете материалы, опубликованные или существенно обновлённые за последние 24 часа; по понедельникам проверьте также выходные. Отдавайте предпочтение отслеживаемым первичным источникам: объявлениям проектов, заметкам о релизах и оригинальным статьям. Объединяйте дублирующие сообщения об одном событии.

Выберите до пяти событий, напрямую связанных с темой. Для каждого объясните, что произошло, укажите дату публикации, опишите практический эффект для пользователей или разработчиков и приведите ссылку на первоисточник. Отделяйте установленные факты от своей интерпретации и отмечайте противоречивые данные.

Завершите одним-двумя вопросами или событиями для дальнейшего наблюдения. Если полезных новостей мало, скажите об этом и сохраните краткость вместо добавления старых или предположительных материалов.`,
    params: {
      topic: {
        label: "Тема сводки",
        placeholder:
          "например: релизы Postgres, инструменты ИИ для программирования, исследования батарей",
      },
    },
  },
  "weekly-work-draft": {
    prompt: `Создайте редактируемый итог работы за последние семь дней в этом рабочем пространстве.

Соберите данные из коммитов, релевантных pull request, изменённой документации и доступных заметок проекта. Объединяйте связанные изменения по результату, чтобы серия небольших коммитов не превратилась в длинный журнал активности. Не делайте выводы о владельцах, завершённости или планах только по именам веток.

Организуйте черновик так:
- Выполнено: завершённые результаты и их значение.
- Остаётся открытым: незавершённая работа, блокеры и известные ограничения.
- Предлагаемые следующие шаги: действия, подтверждённые текущими данными и явно отмеченные как предложения.
- Требует подтверждения: недостающий контекст или решения, которые должен заполнить пользователь.

Свяжите каждый существенный пункт с коммитом, PR или файлом. Если источник недоступен, укажите пробел. Верните только черновик; не меняйте файлы рабочего пространства и состояние репозитория.`,
  },
  "docs-freshness": {
    prompt: `Проверьте документацию рабочего пространства на наличие инструкций, которые сегодня могут не сработать или ввести читателя в заблуждение.

Начните с README и индекса документации, затем проследите связанные руководства по настройке, использованию и обслуживанию. Сравните указанные команды, пути, поля конфигурации и примеры API с текущим кодом. Используйте документированную в репозитории команду проверки ссылок, если она доступна и разрешена.

Отдавайте приоритет проверяемым проблемам: отсутствующим целям, устаревшим командам, противоречивым инструкциям и примерам, которые больше не соответствуют реализации. Считайте датированные исторические записи историей, если они не представлены как действующие инструкции.

Для каждой проблемы укажите путь документа и заголовок или строку, подтверждающий код или результат проверки, вероятную ошибку читателя и конкретное исправление. Отсортируйте список по влиянию и завершите тем, что не удалось проверить. Подготовьте отчёт без изменения документов.`,
  },
} as const;
