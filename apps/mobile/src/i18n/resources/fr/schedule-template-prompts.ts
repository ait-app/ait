export const scheduleTemplatePrompts = {
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
