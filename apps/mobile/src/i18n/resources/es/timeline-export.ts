export const timelineExport = {
  title: "Exportar sesión",
  description:
    "Solo historial guardado. La conversación incluye mensajes del usuario, del asistente y transcripciones de voz. La cronología también incluye tarjetas guardadas. Máximo 16 MiB.",
  conversation: "Solo conversación",
  timeline: "Toda la cronología guardada",
  download: "Exportar y descargar",
  cancel: "Cancelar exportación",
  unavailable: "Exportación no disponible. Conéctate a un host compatible.",
  tooLarge: "La exportación supera el límite de 16 MiB. No se ha truncado nada.",
  browserHandoff: "Enviado al navegador: revisa sus descargas para terminar de guardar.",
};

export const nativeHtmlExport = {
  title: "Exportación HTML nativa (OMP)",
  description:
    "Oh My Pi renderiza esta sesión con su propio formateador. El archivo puede contener scripts activos y solo se guarda como descarga; nunca se abre dentro de Ait. Máximo 16 MiB.",
  download: "Exportar HTML nativo",
  risk: "El formato nativo contiene scripts y puede cargar un visor externo (stencil.so) al abrirse; ese visor puede leer el historial de sesión incrustado en el archivo. Solo descarga: Ait nunca lo previsualiza ni lo abre. No es la exportación HTML genérica inerte.",
  unavailable: "Exportación HTML nativa no disponible. Conéctate a un host OMP compatible.",
};
