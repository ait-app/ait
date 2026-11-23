export const timelineExport = {
  title: "Exporter la session",
  description:
    "Historique enregistré uniquement. La conversation inclut les messages utilisateur, assistant et les transcriptions vocales. La chronologie inclut aussi les cartes enregistrées. Maximum 16 Mio.",
  conversation: "Conversation uniquement",
  timeline: "Toute la chronologie enregistrée",
  download: "Exporter et télécharger",
  cancel: "Annuler l’export",
  unavailable: "Export indisponible. Connectez-vous à un hôte compatible.",
  tooLarge: "L’export dépasse la limite de 16 Mio. Aucun contenu n’a été tronqué.",
  browserHandoff:
    "Transmis au navigateur — consultez ses téléchargements pour terminer l’enregistrement.",
};

export const nativeHtmlExport = {
  title: "Export HTML natif (OMP)",
  description:
    "Oh My Pi rend cette session avec son propre formateur. Le fichier peut contenir des scripts actifs et n’est enregistré qu’en téléchargement — il n’est jamais ouvert dans Ait. Maximum 16 Mio.",
  download: "Exporter le HTML natif",
  risk: "Le format natif contient des scripts et peut charger une visionneuse externe (stencil.so) à l’ouverture ; celle-ci peut lire l’historique de session intégré au fichier. Téléchargement uniquement — Ait ne le prévisualise ni ne l’ouvre jamais. Différent de l’export HTML générique inerte.",
  unavailable: "Export HTML natif indisponible. Connectez-vous à un hôte OMP compatible.",
};
