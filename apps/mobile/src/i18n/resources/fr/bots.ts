export const bots = {
  modeCapWarning:
    "Les modes qui permettent aux agents d'exécuter des commandes leur permettent aussi de lire les identifiants Ait de cet ordinateur et de contourner les limites Ait du Bot. N'autorisez-les que pour un travail que vous laisseriez un agent faire sans surveillance.",
  commandReachWarning:
    "Le Bot peut envoyer des instructions à chaque agent de cet espace de travail, y compris ceux que vous avez lancés, et modifier leurs réglages. Ces agents conservent leurs propres autorisations : une instruction du Bot peut donc atteindre des capacités au-delà des limites du Bot.",
  terminalsWarning:
    "Les terminaux exécutent des commandes sur cet ordinateur : ils peuvent donc lire ses identifiants Ait et contourner les limites Ait du Bot. Ne les activez que pour un travail que vous laisseriez un agent faire sans surveillance.",
  noteInstructionsInInput:
    "Ce fournisseur ne prend pas en charge un prompt système modifiable ; les instructions intégrées et la mémoire du Bot sont donc ajoutées à l'entrée de la conversation.",
  noteNoReadOnlyMode:
    "Ce fournisseur n'a pas de mode lecture seule ; le Bot s'exécute donc dans son mode par défaut. Les écritures qui nécessitent une approbation sont refusées automatiquement, mais les actions que le fournisseur autorise sans approbation ne sont pas bloquées, y compris la lecture des identifiants Ait de cet ordinateur et le contournement des limites Ait du Bot.",
  noteToolsNeedApproval:
    "Ce fournisseur ne peut pas pré-approuver les outils Ait ; les appels d'outils Ait qui nécessitent une approbation sont donc refusés automatiquement. Le Bot ne pourra peut-être que discuter, sans lancer ni gérer d'agents.",
  noteNoAitTools:
    "Ce fournisseur ne prend pas en charge la connexion aux outils Ait ; le Bot ne peut donc que discuter.",
  permissions: "Autorisations",
  permissionsHint: "Les changements d'autorisations prennent effet au prochain tour du Bot.",
  groupObserve: "Observer (toujours actif) : voir les agents, l'activité et les fournisseurs",
  groupAgents: "Agents : créer des agents, leur envoyer des instructions et les reconfigurer",
  groupStop: "Arrêt : annuler et arrêter des agents",
  groupSchedules: "Planifications : créer et gérer des planifications",
  groupScripts: "Scripts : exécuter les scripts de l'espace de travail",
  groupTerminals: "Terminaux : créer des terminaux et y saisir du texte",
  permissionRiskHint: "Peut exécuter des commandes.",
  modeCap: "Mode d'agent par défaut",
  modeNotAllowed: "Non autorisé",
  modeCapHint:
    "Choisissez le mode des agents que le Bot lance pour chaque fournisseur. Le Bot ne peut utiliser que ce mode ; choisissez Non autorisé pour l'empêcher d'utiliser un fournisseur.",
  providerUnavailable: "{{provider}} (indisponible)",
  readOnlyHint:
    "Le Bot s'exécute dans le mode le plus strict de son fournisseur (pour Claude, le mode par défaut, où vos propres règles d'autorisation s'appliquent toujours) et laisse les modifications aux agents qu'il lance.",
  title: "Bot",
  chat: "Discussion",
  settings: "Réglages",
  provider: "Fournisseur",
  model: "Modèle",
  mode: "Mode d’autorisation",
  thinking: "Raisonnement",
  defaultOption: "Par défaut du fournisseur",
  save: "Enregistrer",
  setupTitle: "Configurer le Bot de l’espace de travail",
  setupHint:
    "Le Bot gère cet espace de travail et continue de fonctionner après la fermeture de cet onglet.",
  setup: "Configurer le Bot",
  clearMemory: "Effacer la mémoire enregistrée",
  notConfigured:
    "Cet espace de travail n’a pas encore de Bot. Seul le propriétaire de l’hôte peut en configurer un.",
  disabled:
    "Activez les fonctionnalités expérimentales dans Réglages → À propos pour utiliser les Bots.",
  unsupported: "Mettez à jour cet hôte pour utiliser les Bots.",
  disconnected: "Connectez-vous à cet hôte pour utiliser les Bots.",
  message: "Message",
  cancel: "Annuler le tour",
  emptyChat: "Envoyez un message pour démarrer une conversation.",
  session: "Session du Bot",
  reset: "Nouvelle conversation",
  resetConfirm:
    "Démarrer une nouvelle conversation ? Les messages existants sont conservés et les derniers réglages du Bot seront appliqués.",
  configurationHint:
    "Les changements de modèle s’appliquent aux nouvelles conversations. Démarrez une nouvelle conversation pour les utiliser ici.",
  memory: "Mémoire",
  memoryHint:
    "Préférences que le Bot lit avant chaque réponse, comme la langue des titres de PR ou le style de réponse.",
  saveMemory: "Enregistrer la mémoire",
  memoryConflict:
    "La mémoire a été modifiée ailleurs. Rechargez-la avant de l’enregistrer à nouveau.",
  resumeToEdit: "Reprenez le Bot pour effectuer des modifications.",
  reload: "Recharger",
  pause: "Mettre en pause",
  remove: "Supprimer le Bot",
  removeConfirm:
    "Supprimer ce Bot ? Son historique de conversation ne sera plus visible, sa mémoire est conservée et les connexions Telegram et Feishu seront déconnectées.",
  paused: "En pause",
  resume: "Reprendre",
  directoryMissing:
    "Le répertoire de l’espace de travail est introuvable, le Bot est donc en pause. Restaurez le répertoire, puis reprenez.",
  requestFailed: "Échec de la requête",
  tooLong: "Les messages ne doivent pas dépasser 16 Kio.",
  memoryTooLong: "La mémoire ne doit pas dépasser 32 Kio.",
  channels: "Canaux",
  addChannel: "Ajouter un canal",
  channelType: "Type de canal",
  telegram: "Telegram",
  feishu: "Feishu",
  lark: "Lark",
  tokenRef: "Référence du jeton du Bot",
  secretRef: "Référence du secret de l’application",
  appId: "ID de l’application",
  allowedSenders: "Expéditeurs autorisés (séparés par des virgules ou des retours à la ligne)",
  allowGroups: "Autoriser les discussions de groupe",
  channelRefHint:
    "Les identifiants proviennent des variables d’environnement du processus qui démarre le daemon. Saisissez uniquement la référence de la variable, comme env:AIT_SERVER_CREDENTIAL_TELEGRAM, et au moins un expéditeur autorisé.",
  channelEnabled: "Activé",
  channelConnected: "Connecté",
  channelIdle: "Inactif",
  feishuScanHint:
    "Scannez un code QR avec l'application Feishu. Feishu crée un bot pour vous et Ait le connecte ; vous seul pouvez lui écrire tant que vous ne changez pas ce réglage.",
  feishuScanStart: "Afficher le code QR",
  feishuScanWaiting:
    "Scannez ce code avec l'application Feishu et approuvez le nouveau bot. Gardez cette page ouverte jusqu'à la fin.",
  feishuScanOpen: "Ouvrir dans Feishu",
  feishuScanQrLabel: "Code QR de configuration Feishu",
  feishuScanRetry: "Réessayer",
  feishuScanDenied: "La demande a été refusée dans Feishu.",
  feishuScanExpired: "Ce code QR a expiré. Recommencez pour en obtenir un nouveau.",
  feishuScanLark:
    "Ce QR code est destiné à Feishu. Pour un compte Lark, choisissez Lark et saisissez les identifiants de l’application manuellement.",
  feishuScanUnavailable:
    "Impossible de terminer la configuration avec Feishu. Réessayez ou saisissez les identifiants manuellement.",
  feishuManualEntry: "Saisir les identifiants manuellement",
  feishuScanInstead: "Scanner un code QR",
  feishuAppSecret: "App Secret",
  feishuAppSecretHint:
    "Collez l'App Secret. Ait le vérifie auprès de {{platform}} et le conserve en privé sur cet hôte. Une référence env:AIT_SERVER_CREDENTIAL_* fonctionne aussi.",
  feishuManualStep1:
    "Créez une application personnalisée sur {{developerUrl}} et activez la capacité Bot.",
  feishuManualStep2:
    "Dans Autorisations, ajoutez im:message:send_as_bot, im:message.p2p_msg:readonly, im:message.group_at_msg:readonly.",
  feishuManualStep3:
    "Copiez ici l'App ID et l'App Secret, ajoutez votre Open ID {{platform}} (ou_...) comme expéditeur autorisé, puis ajoutez le canal.",
  feishuManualStep4:
    "Ensuite, dans Événements et rappels de {{platform}}, choisissez Connexion longue et abonnez-vous à im.message.receive_v1. {{platform}} ne peut l'enregistrer qu'une fois le canal connecté.",
  feishuManualStep5: "Publiez une version de l'application pour qu'on puisse écrire au bot.",
  telegramGuideStep1: "Scannez ce code ou utilisez le bouton pour ouvrir BotFather dans Telegram.",
  telegramGuideStep2:
    "Envoyez /newbot, puis choisissez un nom et un nom d'utilisateur se terminant par bot.",
  telegramGuideStep3: "Copiez le jeton envoyé par BotFather et collez-le ci-dessous.",
  telegramBotFatherOpen: "Ouvrir BotFather",
  telegramBotFatherQrLabel: "Code QR qui ouvre BotFather",
  telegramToken: "Jeton du bot",
  telegramTokenHint:
    "Ait vérifie le jeton auprès de Telegram et le conserve en privé sur cet hôte.",
  telegramContinue: "Continuer",
  telegramPairWaiting:
    "Scannez ce code ou ouvrez le lien, puis appuyez sur Start dans la conversation avec @{{bot}}. Vous deviendrez l'expéditeur autorisé. Gardez cette page ouverte jusqu'à la fin.",
  telegramPairOpen: "Ouvrir dans Telegram",
  telegramPairQrLabel: "Code QR qui ouvre votre nouveau bot",
  telegramInvalidToken: "Telegram a refusé ce jeton. Copiez-le à nouveau depuis BotFather.",
  telegramAlreadyConnected:
    "Ce bot est déjà connecté à un canal sur cet hôte. Supprimez d'abord ce canal ou créez un nouveau bot.",
  telegramWebhookConflict:
    "Ce bot a un webhook configuré. Supprimez-le avec deleteWebhook, puis réessayez.",
  telegramPollingConflict:
    "Un autre programme reçoit les messages de ce bot. Arrêtez-le, puis réessayez.",
  telegramExpired: "Ce lien a expiré. Continuez pour en obtenir un nouveau.",
  telegramUnavailable:
    "Impossible de joindre Telegram. Vérifiez la connexion de cet hôte, puis réessayez.",
  telegramUseEnvironment: "Utiliser une variable d'environnement",
  telegramUseGuided: "Utiliser la configuration guidée",
  weixin: "WeChat",
  qq: "QQ",
  weixinScanHint:
    "Scannez un code QR avec WeChat pour connecter un bot. Vous seul pouvez lui écrire, en discussion privée.",
  weixinScanWaiting:
    "Scannez ce code avec WeChat et confirmez sur votre téléphone. Gardez cette page ouverte jusqu’à la fin.",
  weixinScanOpen: "Ouvrir dans WeChat",
  weixinScanQrLabel: "Code QR de configuration WeChat",
  weixinScanUnavailable: "Impossible de terminer la configuration avec WeChat. Réessayez.",
  qqScanHint:
    "Scannez un code QR avec QQ. QQ crée un bot et Ait le connecte ; vous seul pouvez lui écrire jusqu’à ce que vous changiez cela.",
  qqScanWaiting:
    "Scannez ce code avec QQ et approuvez le nouveau bot. Gardez cette page ouverte jusqu’à la fin.",
  qqScanOpen: "Ouvrir dans QQ",
  qqScanQrLabel: "Code QR de configuration QQ",
  qqScanUnavailable:
    "Impossible de terminer la configuration avec QQ. Réessayez ou saisissez les identifiants manuellement.",
  scanAlreadyConnected:
    "Ce bot est déjà connecté à un canal sur cet hôte. Supprimez ce canal, puis réessayez.",
  qqManualStep1: "Créez un bot sur q.qq.com et copiez ici son App ID et son App Secret.",
  qqManualStep2:
    "Après avoir ajouté le canal, obtenez un code d’appairage sur sa ligne et envoyez-le au bot dans QQ. Vous deviendrez un expéditeur autorisé.",
  qqAppSecretHint:
    "Ait vérifie l’App Secret auprès de QQ et le stocke de manière privée sur cet hôte.",
  qqPairButton: "Obtenir un code d’appairage",
  qqPairHint:
    "Code : {{code}}. Envoyez-le au bot en discussion privée, ou mentionnez le bot avec ce code dans un groupe QQ. Il fonctionne une seule fois et expire dans 10 minutes.",
  imessage: "iMessage",
  webhook: "Webhook",
  webhookHint:
    "Ait Hub crée une adresse HTTPS et un secret pour ce Bot. Tout système capable d’envoyer une requête HTTP peut les utiliser pour écrire au Bot et lire sa réponse.",
  webhookCreate: "Créer un webhook",
  webhookUrl: "Adresse",
  webhookSecret: "Secret",
  webhookSecretOnce:
    "Le secret n’est affiché qu’une seule fois. Copiez-le maintenant ; en cas de perte, créez-en un nouveau.",
  webhookExample: "Exemple",
  webhookExampleHint:
    "L’appel renvoie une adresse de statut. Interrogez-la avec le même secret (ajoutez ?wait=30 pour attendre la réponse), ou indiquez callback_url dans l’appel pour recevoir la réponse à cette adresse. Les appels avec le même thread poursuivent une conversation.",
  webhookCopy: "Copier",
  webhookCopied: "Copié",
  webhookDone: "Terminé",
  webhookRotate: "Créer un nouveau secret",
  webhookRotateConfirm:
    "Le secret actuel cesse immédiatement de fonctionner. Les appelants qui l’utilisent devront passer au nouveau.",
  webhookCenterOffline:
    "Cet hôte n’est pas connecté à Ait Hub. Connectez-vous et activez la connexion au Hub, puis réessayez.",
  webhookCenterUnsupported: "Cet Ait Hub ne prend pas encore en charge les webhooks.",
  webhookAutomationUnavailable: "L’automatisation n’est pas activée sur cet Ait Hub.",
  webhookLimitReached:
    "Votre compte a atteint sa limite de webhooks. Supprimez-en un que vous n’utilisez plus, puis réessayez.",
  webhookCenterError: "Ait Hub n’a pas pu configurer le webhook. Réessayez plus tard.",
  imessageGuideStep1: "Créez un projet Photon Beta sur app.beta.photon.codes.",
  imessageGuideStep2:
    "Dans ce projet, créez une clé d'API avec project:read, project:write, platforms:read et platforms:write.",
  imessageGuideStep3:
    "Saisissez ci-dessous le Project ID, la clé d'API et votre propre numéro de téléphone ou e-mail iMessage.",
  imessagePhotonOpen: "Ouvrir Photon",
  imessagePrivacyNotice:
    "Les messages échangés avec ce bot passent par le service de Photon. Seuls les messages directs sont pris en charge, pas les discussions de groupe.",
  imessageProjectId: "Project ID",
  imessageApiKey: "Clé d'API",
  imessageApiKeyHint: "Ait vérifie la clé auprès de Photon et la conserve en privé sur cet hôte.",
  imessageHandle: "Votre numéro de téléphone ou e-mail iMessage",
  imessageHandleHint:
    "L'adresse depuis laquelle vous envoyez des iMessages. Écrivez les numéros au format international, par exemple +14155550123.",
  imessageContinue: "Continuer",
  imessagePairPhoneNumber: "Envoyer à",
  imessagePairCode: "Code",
  imessagePairWaiting:
    "Envoyez ce code par iMessage, et non par SMS, exactement depuis {{handle}}. Scanner le code QR ou ouvrir le lien le remplit pour vous. Gardez cette page ouverte jusqu'à la fin.",
  imessagePairExpires: "Le code expire dans {{minutes}} min.",
  imessagePairOpen: "Ouvrir dans Messages",
  imessagePairQrLabel: "Code QR qui ouvre Messages avec le code déjà saisi",
  imessageInvalidKey:
    "Photon a refusé ce Project ID ou cette clé d'API. Vérifiez les deux, et que la clé dispose des quatre autorisations.",
  imessageInvalidHandle:
    "Saisissez un numéro au format international, par exemple +14155550123, ou un e-mail iMessage.",
  imessageDedicatedLine:
    "Ce projet Photon utilise des lignes dédiées, qui ne sont pas prises en charge. Utilisez un projet avec des lignes partagées.",
  imessageAlreadyConnected:
    "Ce projet Photon est déjà connecté à un canal sur cet hôte. Supprimez d'abord ce canal ou utilisez un autre projet.",
  imessageExpired: "Ce code a expiré. Continuez pour en obtenir un nouveau.",
  imessageUnavailable:
    "Impossible de joindre Photon. Vérifiez la connexion de cet hôte, puis réessayez.",
  deleteChannel: "Supprimer le canal",
  deleteChannelConfirm: "Supprimer ce canal ? Sa connexion externe sera déconnectée.",
  notifications: {
    title: "Notifications",
    wokeBy: "Activité de l'espace de travail",
    silent: "Rien à signaler.",
    wakeScope: "Réveiller le Bot pour",
    wakeScopeOwn: "Son propre travail (par défaut)",
    wakeScopeAll: "Tout cet espace de travail",
    wakeScopeHint:
      "Son propre travail couvre les agents et les tâches planifiées lancés par le Bot. Avec Tout, n'importe quel autre événement d'agent, de tâche planifiée ou de terminal de cet espace de travail lance aussi un tour du Bot, ce qui consomme plus d'appels au modèle.",
    recent: "Notifications récentes",
    empty: "Aucune notification pour l'instant.",
    state: {
      logged: "Hors du périmètre de réveil",
      pending: "En attente de réveil du Bot",
      woke: "A réveillé le Bot",
      other: "Autre état",
    },
    skipped: {
      over_budget: "Non transmise : limite horaire de réveils atteinte",
      inbox_full: "Non transmise : trop de notifications en attente",
      paused: "Non transmise : le Bot était en pause",
      observed: "Non répétée : le Bot avait déjà consulté cet agent dans cette conversation",
      blocked: "Non transmise : cette conversation a été interrompue et doit être réinitialisée",
      reported: "Non répétée : l'agent avait déjà signalé ce résultat sur sa tâche",
      other: "N'a pas réveillé le Bot",
    },
    kind: {
      agent_finished: "Agent terminé",
      agent_failed: "Échec de l'agent",
      agent_permission: "L'agent attend une autorisation",
      schedule_succeeded: "Exécution planifiée réussie",
      schedule_failed: "Échec de l'exécution planifiée",
      terminal_attention: "Le terminal demande votre attention",
      ticket_blocked: "L'agent est bloqué sur une tâche",
      ticket_done: "L'agent a terminé sa part d'une tâche",
      ticket_failed: "L'agent n'a pas pu terminer sa part d'une tâche",
      other: "Activité de l'espace de travail",
    },
  },
  tickets: {
    title: "Tâches",
    pillOpen: "{{count}} en cours",
    pillWaiting: "En attente de vous : {{count}}",
    pillLabel: "Tâches du Bot : {{open}} en cours, {{waiting}} en attente de vous",
    empty: "Aucune tâche en cours. Une tâche apparaît quand le Bot confie du travail à un agent.",
    showClosed: "Afficher les tâches fermées récemment",
    hideClosed: "Masquer les tâches fermées",
    closedHeading: "Fermées récemment",
    noClosed: "Aucune tâche fermée.",
    agentCount: "Agents : {{count}}",
    agentFallback: "Agent {{id}}",
    agents: "Agents",
    noAgents: "Aucun agent assigné pour l'instant.",
    request: "Demande",
    history: "Historique",
    noReports: "Aucun compte rendu pour l'instant.",
    outcome: "Conclusion",
    noOutcome: "Aucune conclusion enregistrée.",
    outcomePlaceholder: "Conclusion (facultative)",
    markDone: "Marquer comme terminée",
    cancel: "Annuler la tâche",
    cancelConfirm:
      "Annuler cette tâche ? Ses agents continuent de tourner ; arrêtez-les séparément si besoin. Vous pourrez rouvrir la tâche plus tard.",
    keep: "Garder la tâche",
    reopen: "Rouvrir",
    openLimit: "Trop de tâches en cours. Fermez celles qui sont finies avant de rouvrir celle-ci.",
    conflict: "Cette tâche a changé entre-temps. Vérifiez son état et réessayez.",
    linked: "Tâche : {{title}}",
    untitled: "Tâche sans titre",
    movedTo: "désormais {{status}}",
    status: {
      open: "En cours",
      waiting: "En attente de vous",
      done: "Terminée",
      failed: "Échouée",
      cancelled: "Annulée",
      other: "Autre état",
    },
    waiting: {
      permission: "Un agent attend votre autorisation",
      blocked: "Un agent est bloqué et a besoin de vous",
      other: "Demande votre attention",
    },
    source: {
      agent: "Agent",
      bot: "Bot",
      user: "Vous",
      system: "Ait",
      other: "Autre",
    },
    report: {
      progress: "Avancement",
      blocked: "Bloqué",
      done: "A terminé sa part",
      failed: "A abandonné sa part",
      agent_finished: "L'agent a terminé un tour",
      agent_failed: "Échec de l'agent",
      agent_permission: "L'agent attend une autorisation",
      note: "Note",
      status: "Changement d'état",
      other: "Mise à jour",
    },
  },
  status: {
    queued: "En file d’attente",
    sending: "Démarrage…",
    running: "En cours…",
    needs_approval: "Approbation requise",
    completed: "Terminé",
    failed: "Échec",
    uncertain: "Interrompu : démarrez une nouvelle conversation pour continuer",
    cancelled: "Annulé",
  },
};
