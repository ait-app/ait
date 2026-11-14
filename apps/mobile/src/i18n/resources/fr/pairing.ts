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
  },
  direct: {
    title: "Connexion directe",
    helper: "Saisissez l'adresse de votre serveur et le jeton d'accès.",
    fields: {
      host: "Hôte",
      port: "Port",
      password: "Jeton du serveur",
      optional: "Requis",
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
      connecting: "Connexion...",
    },
    errors: {
      hostRequired: "L'hôte est requis",
      invalidPort: "Le port doit être compris entre 1 et 65535",
      invalidConnection: "Connexion invalide",
      failedTitle: "La connexion a échoué",
      failedToConnect: "Nous n'avons pas réussi à nous connecter à {{endpoint}}.",
      noAdditionalDetails: "{{detail}} (aucun détail supplémentaire fourni)",
      timedOut: "La connexion a expiré. Vérifiez l'hôte/le port et votre réseau.",
      refused: "Connexion rejetée. Le serveur fonctionne-t-il à cette adresse ?",
      hostNotFound: "Hôte introuvable. Vérifiez le nom d'hôte et réessayez.",
      hostUnreachable: "L'hôte est inaccessible. Vérifiez votre réseau et votre pare-feu.",
      tlsError:
        "Erreur TLS. Les connexions directes utilisent SSL uniquement lorsqu'un terminateur TLS se trouve devant le démon.",
      unableToConnect:
        "Impossible de se connecter. Vérifiez l'hôte/le port et que le démon est accessible.",
      details: "Détails : {{detail}}",
    },
  },
  remoteSsh: {
    title: "SSH distant",
    helper: "Connectez-vous au serveur Ait distant via SSH (port par défaut du serveur : 7316).",
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
};
