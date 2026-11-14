export const scheduleTemplatePrompts = {
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
