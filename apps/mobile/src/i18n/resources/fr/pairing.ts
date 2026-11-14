export const pairing = {
  connectionMethods: {
    title: "Ajouter une connexion",
    direct: {
      title: "Connexion directe",
      description: "Réseau local ou VPN.",
    },
    remoteSsh: {
      title: "SSH distant",
      description: "Connectez-vous avec le client SSH de bureau.",
    },
    scanQr: {
      title: "Scanner le codeQR",
      description: "Connexion relais cryptée.",
    },
    pasteLink: {
      title: "Coller le lien d'association",
      description: "Connexion relais cryptée.",
    },
  },
  direct: {
    title: "Connexion directe",
    helper: "Saisissez l'adresse d'un serveur Ait.",
    fields: {
      host: "Host",
      port: "Port",
      password: "Mot de passe",
      optional: "Facultatif",
      useSsl: "Utiliser SSL",
      connectionUri: "URI de connexion",
    },
    advanced: {
      label: "Avancé",
      show: "Afficher avancé",
      hide: "Masquer avancé",
    },
    passwordVisibility: {
      show: "Afficher le mot de passe",
      hide: "Masquer le mot de passe",
    },
    actions: {
      cancel: "Annuler",
      connect: "Connecter",
      connecting: "De liaison...",
    },
    errors: {
      hostRequired: "Hostest requis",
      invalidPort: "Le port doit être compris entre 1 et 65535",
      invalidConnection: "Connexion invalide",
      failedTitle: "La connexion a échoué",
      failedToConnect: "Nous n'avons pas réussi à nous connecter à{{endpoint}}.",
      noAdditionalDetails: "{{detail}}(aucun détail supplémentaire fourni)",
      timedOut: "La connexion a expiré. Vérifiez l'hôte/portet votre réseau.",
      refused: "Connexion rejetée. Le serveur fonctionne-t-il à cette adresse?",
      hostNotFound: "Hostintrouvable. Vérifiez le nom d'hôte et réessayez.",
      hostUnreachable: "Hostest inaccessible. Vérifiez votre réseau et votre pare-feu.",
      tlsError:
        "Erreur TLS. Les connexions directes utilisent SSL uniquement lorsqu'un terminateur TLS se trouve devant le démon.",
      unableToConnect:
        "Impossible de se connecter. Vérifiez l'hôte/portet que le démon est accessible.",
      details: "Détails:{{detail}}",
    },
  },
  remoteSsh: {
    title: "SSH distant",
    helper: "Connectez-vous à un daemon Ait sur l’hôte distant.",
    fields: {
      target: "Hôte SSH",
    },
    actions: {
      cancel: "Annuler",
      connect: "Connecter",
      connecting: "Connexion...",
    },
    errors: {
      tokenRequired: "Saisissez le jeton du serveur Ait distant (32 à 256 caractères).",
      targetRequired: "L’hôte SSH est requis",
      invalidTarget: "Saisissez un hôte ssh:// valide",
      failedToConnect: "Connexion SSH impossible. {{detail}}",
    },
  },
  link: {
    title: "Coller le lien d'association",
    helper: "Collez le lien d'appairage depuis votre serveur.",
    label: "Lien d'appariement",
    errors: {
      required: "Collez un lien d'appairage (.../#offer=...)",
      missingOffer: "Le lien doit inclure#offer=...",
      emptyOffer: "La charge utile de l'offre est vide",
      invalid: "Lien d'association invalide",
      unableToPair: "Impossible de coupler l'hôte",
    },
    alert: {
      failedTitle: "Échec du couplage",
    },
    actions: {
      cancel: "Annuler",
      pair: "Paire",
      pairing: "L'appariement...",
    },
  },
  scan: {
    title: "ScannerQR",
    webUnavailableTitle: "Non disponible sur le Web",
    webUnavailableBody:
      "L'analyseQRn'est pas prise en charge dans la version Web. Utilisez plutôt \"Coller le lien\".",
    backToSettings: "Retour aux paramètres",
    cameraPermissionTitle: "Autorisation de la caméra",
    cameraPermissionBody:
      "Autorisez l'accès à la caméra pour scanner le code d'appairageQRà partir de votre démon.",
    grantPermission: "Accorder l'autorisation",
    pairing: "L'appariement...",
    unableToPair: "Impossible de coupler l'hôte",
    errorTitle: "Erreur",
  },
  device: {
    loadingOffer: "Chargement de l'offre d'association...",
    failedToLoadOffer: "Échec du chargement de l'offre d'association.",
    relayDisabled: "Le relais n'est pas activé. Activer le relais pour coupler un appareil.",
    enableTitle: "Activer le relais ?",
    enableDescription:
      "Le relais permet à cet appareil de se connecter depuis n’importe où. Le trafic de couplage est chiffré de bout en bout.",
    relayDocs: "Fonctionnement du relais",
    relayDocsAccessibility: "Lire comment fonctionne le relais Ait",
    enableRelay: "Activer le relais",
    enablingRelay: "Activation...",
    notNow: "Pas maintenant",
    directConnectionHint:
      "Sans relais, connectez-vous directement via TCP, Tailscale ou un autre VPN. Aucun code QR n’est créé.",
    updateRequired: "Mettez à jour l’hôte pour activer le relais depuis Ait Desktop.",
    unavailable: "Offre de jumelage indisponible.",
    hint: "Scannez ce codeQRavec Ait sur votre téléphone ou copiez le lien ci-dessous.",
    securityWarning:
      "Traitez ce lien d’association comme un mot de passe. Toute personne qui le possède peut accéder à ce daemon.",
    qrUnavailable: "CodeQRindisponible.",
    qrAccessibility: "Code QR de couplage",
    retry: "Réessayer",
    copy: "Copie",
    copied: "Copié",
  },
};
