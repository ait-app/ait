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
};
