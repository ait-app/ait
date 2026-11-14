export const desktop = {
  windowControls: {
    minimize: "Réduire la fenêtre",
    maximize: "Agrandir la fenêtre",
    restore: "Restaurer la fenêtre",
    close: "Fermer la fenêtre",
  },
  quitting: {
    title: "Quitter Ait...",
    detail: "Arrêt du démon local.",
  },
  daemon: {
    listen: {
      host: "Adresse d’écoute",
      port: "Port",
      savedHint: "Redémarrez le daemon pour appliquer les modifications.",
      override:
        "AIT_SERVER_LISTEN remplace ce réglage : {{address}}. Supprimez la variable d’environnement pour utiliser l’adresse enregistrée.",
      invalid:
        "Saisissez une adresse IPv4 ou IPv6 (ou localhost) et un port compris entre 0 et 65535.",
    },
    lifecycle: {
      owned: "Lancé par cette session de bureau",
      attached: "Connecté à un daemon existant",
      ownedMessage: "Ce daemon a été lancé par cette session de bureau.",
      attachedMessage: "Ce daemon n’a pas été lancé par cette session de bureau.",
      stopTitle: "Arrêter le daemon local ?",
      stopMessage:
        "{{ownership}}\nRépertoire personnel : {{home}}\nPID du superviseur : {{pid}}\nLes tâches des agents en cours seront interrompues.",
      stop: "Arrêter le daemon",
      stopping: "Arrêt en cours…",
      stopFailed: "Impossible d’arrêter le daemon",
      pauseAttached:
        "Suspendre la gestion automatique du daemon ? Le daemon connecté continuera de fonctionner.",
      pause: "Suspendre la gestion",
      workerUpdated: "Worker mis à jour vers {{version}}",
      supervisorRefresh:
        "Le superviseur en cours conserve son code d’origine. Le programme qui l’a lancé doit l’arrêter puis le redémarrer pour le mettre à jour.",
    },
    title: "Daemon",
    status: {
      title: "Statut",
      builtInOnly: "Seul le démon de bureau intégré est affiché ici",
      running: "en cours d'exécution",
      notRunning: "ne fonctionne pas",
      pid: "PID{{pid}}",
    },
    management: {
      title: "Gérer le démon intégré",
      hint: "Laissez Ait démarrer et arrêter le démon intégré",
      pauseTitle: "Suspendre le démon intégré",
      pauseMessage:
        "Cela arrêtera immédiatement le démon intégré. Les agents en cours d'exécution et les terminaux connectés au démon intégré seront arrêtés.",
      pauseAndStop: "Pause et arrêt",
      registrationFailed:
        "Built-in daemon started, but Ait could not save the localhost connection. Toggle daemon management off and on again, or add localhost manually.",
      pausedStopFailed:
        "La gestion du démon intégré a été suspendue, mais Ait n'a ​​pas pu arrêter le démon.",
      updateFailed: "Impossible de mettre à jour la gestion des démons intégrés.",
    },
    keepRunning: {
      title: "Laisser le démon fonctionner après avoir quitté",
      hint: "Les agents et terminaux continuent de fonctionner après avoir quitté Ait ; le prochain lancement s'y reconnecte.",
    },
    logs: {
      title: "Fichier journal",
      modalTitle: "JournauxDaemon",
      unavailable: "Chemin du journal indisponible",
      empty: "(le fichier journal est vide)",
      copied: "Chemin du journal copié.",
      copyFailed: "Impossible de copier le chemin du journal.",
      open: "Journaux ouverts",
      copyPath: "Copier le chemin",
    },
    fullStatus: {
      title: "Statut complet",
      modalTitle: "StatutDaemon",
      view: "Afficher l'état",
      copied: "Statut copié dans le presse-papiers.",
      fetchFailed: "Échec de la récupération de l'état du démon:{{message}}",
    },
    versionMismatch:
      "Les versions de l'application et du démon ne correspondent pas. Mettez à jour les deux vers la même version pour une meilleure expérience.",
    loadFailed: "Impossible de charger l'état du démon de bureau.",
  },
  updates: {
    status: {
      checking: "Vérification des mises à jour de l'application...",
      installing: "Installation de la mise à jour de l'application...",
      upToDate: "L'application est à jour.",
      upToDateWithLastChecked: "Up to date. Last checked at {{time}}.",
      pending: "Nous vous informerons lorsque la mise à jour sera prête.",
      pendingWithLastChecked:
        "Nous vous informerons lorsque la mise à jour sera prête. Dernière vérification à {{time}}.",
      pendingWithVersion: "Mise à jour trouvée : {{version}}. Téléchargement...",
      pendingWithVersionAndLastChecked:
        "Mise à jour trouvée : {{version}}. Téléchargement... Dernière vérification à {{time}}.",
      availableWithVersion: "Mise à jour prête:{{version}}",
      availableWithVersionAndLastChecked:
        "Mise à jour prête:{{version}}. Dernière vérification à {{time}}.",
      available: "Une mise à jour de l'application est prête à être installée.",
      availableWithLastChecked:
        "Une mise à jour de l'application est prête à être installée. Dernière vérification à {{time}}.",
      installed: "Mise à jour de l'application installée. Redémarrage requis.",
      failed: "Échec de la mise à jour de l'application.",
      idle: "L'état de la mise à jour n'a pas encore été vérifié.",
    },
    installError: "Impossible d'installer la mise à jour de l'application de bureau.",
    callout: {
      installingTitle: "Installation de la mise à jour",
      failedTitle: "La mise à jour a échoué",
      availableTitle: "Mise à jour disponible",
      genericError: "Quelque chose s'est mal passé.",
      whatsNew: "Quoi de neuf",
      installingAction: "Installation...",
      installAndRestart: "Installer et redémarrer",
      installingDescription: "Installation et redémarrage...",
      versionReady: "{{version}}est prêt à être installé.",
      newVersionReady: "Une nouvelle version est prête à être installée.",
      restartWarning:
        "La mise à niveau de l'application arrêtera l'exécution des agents et fermera les sessions de terminal.",
    },
  },
  settings: {
    loadFailed: "Impossible de charger les paramètres du bureau.",
    saveFailed: "Impossible d'enregistrer les paramètres du bureau.",
  },
  rosetta: {
    title: "Téléchargez la versionApple Silicon",
    runningIntel: "Vous exécutez la versionIntelde Ait sousRosettasurApple Silicon.",
    highCpu:
      "Cela entraîne une utilisation élevée du processeur. Téléchargez la versionApple Siliconpour le réparer.",
    download: "Télécharger",
  },
  permissions: {
    notifications: {
      allowed: "Les notifications sont autorisées par le système d'exploitation.",
      denied: "Les notifications sont refusées dans les paramètres système.",
      notGranted: "Les notifications n'ont pas encore été accordées.",
      webOnly:
        "L’état des notifications sur le bureau est uniquement disponible sur l’exécution Web.",
      supported: "Les notifications de bureau sont prises en charge.",
      unsupported: "Les notifications de bureau ne sont pas prises en charge sur cette plateforme.",
      apiUnavailable: "L'API de notification Web n'est pas disponible dans cet environnement.",
      requestsWebOnly:
        "Les demandes de notification sur le bureau sont uniquement disponibles sur l'exécution Web.",
      requestUnavailable: "L'API de notification Web requestPermission() n'est pas disponible.",
      requestFailed: "Échec de la demande d'autorisation de notification:{{message}}",
      unexpectedState: "État d'autorisation de notification inattendu:{{state}}",
    },
    microphone: {
      webOnly:
        "L’état du microphone de bureau est uniquement disponible dans l’environnement d’exécution Web.",
      navigatorUnavailable: "Le navigateur n'est pas disponible dans cet environnement.",
      granted: "L’accès au microphone est accordé.",
      denied: "L'accès au microphone est refusé dans les paramètres système.",
      notGranted: "L'autorisation du microphone n'a pas encore été accordée.",
      unexpectedState: "État d'autorisation inattendu du microphone:{{state}}",
      statusApiUnavailable:
        "L’API d’état du microphone n’est pas disponible dans ce runtime. Utilisez Request pour vérifier l’accès.",
      queryFailed: "Échec de l'interrogation de l'état du microphone:{{message}}",
      captureUnavailable: "La capture du microphone n'est pas disponible dans cet environnement.",
      permissionApiUnavailable:
        "L'API d'état des autorisations n'est pas disponible. Utilisez Request pour vérifier l’accès.",
      requestsWebOnly:
        "Les demandes de microphone de bureau sont uniquement disponibles sur le runtime Web.",
      captureApiUnavailable:
        "L'API de capture de microphone n'est pas disponible dans cet environnement.",
      requestDenied: "L'autorisation du microphone a été refusée par l'utilisateur ou le système.",
      noDevice: "Aucun microphone n'a été trouvé.",
      requestFailed: "Échec de la demande d'autorisation du microphone:{{message}}",
    },
    empty: {
      notifications: "L'état de la notification n'a pas encore été vérifié.",
      microphone: "L'état du microphone n'a pas encore été vérifié.",
    },
    testNotification: {
      title: "Test de notification Ait",
      body: "Si vous pouvez voir cela, les notifications sur le bureau fonctionnent.",
      notDelivered:
        "La notification n'a pas été délivrée. Vérifiez Paramètres système > Notifications.",
      failed: "Échec de l'envoi de la notification.",
    },
  },
};
