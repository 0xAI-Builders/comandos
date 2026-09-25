# Extensiones por panel

El botón **MCPs · Skills**, junto al logo, modelo y esfuerzo, abre un estante bajo las terminales. El encabezado identifica la sesión y el panel al que se aplica la selección. Abrir el botón de otro panel muestra su propia selección; cerrar el estante conserva las terminales.

Para cambiar la altura del estante, arrastra su borde superior. En el dashboard también responde al teclado: ↑/↓ en pasos de 24 px, Inicio al máximo y Fin al mínimo; con doble clic vuelve a la altura por defecto. Nunca baja de 180 px ni tapa las terminales. La altura se recuerda: en la app de escritorio en `~/.claude/hooks/app-extension-shelf.json` y en el dashboard en el `localStorage` de cada navegador. Si vuelves a pulsar el botón con el estante abierto, la altura no cambia.

Pulsa una burbuja o arrástrala entre **Seleccionadas** y **Disponibles**. Cada edición guarda un borrador de ese panel y conversación. **Aplicar y reanudar** prepara la configuración y un punto de recuperación antes de cerrar el agente. Si está trabajando, **Aplicar al terminar** espera; **Interrumpir y aplicar ahora** solicita el cambio inmediato. La espera se puede cancelar antes de que empiece el reinicio.

Las plantillas guardan una selección con nombre. Se cargan cuando las eliges, entre CLIs compatibles, y muestran las extensiones que faltan o no son editables. No se asignan automáticamente a un proyecto.

## Qué significa el estado

- **Seleccionadas** es el borrador guardado. Los signos `+` y `−` comparan ese borrador con una configuración de proceso verificada.
- **Carga sin verificar** significa que aún no hay evidencia suficiente del proceso actual. Guardar un borrador no confirma una carga.
- La verificación comprueba la configuración del proceso lanzado, su identidad y la conversación que retoma. No confirma que cada servidor MCP haya logrado conectarse a su proveedor.
- Si falla el destino, la operación intenta restaurar la conversación y selección anteriores. Una recuperación pendiente conserva el punto de recuperación y bloquea nuevos cambios en ese panel.
- **Sin dato** significa que no se puede establecer el uso. Solo un historial completo puede establecer **sin uso**. Los datos parciales conservan las llamadas observadas; no se estiman ahorros de tokens.

## Alcance y aislamiento

Se incluyen Claude, Codex, Grok, OpenCode y Antigravity. Las selecciones se aplican mediante opciones nativas o montajes en un espacio de nombres privado del proceso. Los archivos compartidos, las credenciales, el catálogo y los directorios originales de skills no se modifican al editar un panel.

El reinicio requiere una conversación exacta, una cuenta identificada y un panel cuya identidad siga siendo la misma. Nunca se toma la conversación más reciente de una carpeta. OpenCode puede identificar una conversación iniciada sin `--session` mediante el plugin de ComandOS; un proceso anterior a esa actualización puede necesitar iniciarse de nuevo con el plugin actualizado. Las declaraciones que un CLI no permite aislar se muestran como no editables o impiden la preparación antes de detener el origen.

Si el modelo o esfuerzo cambian mientras la operación espera, se cancela antes de cerrar el agente: vuelve a aplicar desde el estado actualizado. OpenCode conserva los permisos por proceso admitidos; las rutas de configuración externas y las extensiones de un overlay que el inventario no pueda verificar impiden el reinicio.

Los artefactos privados de lanzamiento y recuperación se conservan mientras puedan ser necesarios. Contienen configuración sensible: no deben subirse a Git ni compartirse. La API del estante devuelve nombres, selecciones, estados y recuentos; no devuelve comandos, variables de entorno ni contenido de conversaciones.

## Comprobación

Las pruebas de Python cubren selección, revisiones, bloqueo compartido con el coordinador, identidad del proceso, recuperación e inventario. Los procesos de prueba usan configuraciones y sockets tmux aislados; no llaman a modelos reales.

`tests/e2e_pane_extensions.cjs` comprueba el estante con APIs simuladas en el runtime de Chrome del Mac. `tests/e2e_remote_workspace.cjs` comprueba el dashboard completo en cuatro tamaños. No ejecutar las pruebas de navegador en el equipo local: esta instalación reserva esa automatización para el Mac.

Para validar este cambio, usar las pruebas `test_extension_launch.py`, `test_pane_extensions.py`, `test_pane_extensions_api.py`, `test_extension_coordinator.py`, `test_extension_observations.py`, `test_native_extension_metadata.py`, `test_extension_desktop.py` y `test_extension_tmux.py`. Cubren el contrato nuevo y sus conexiones directas. Las pruebas antiguas de perfiles globales no sustituyen esta validación.

La comprobación remota integrada puede limitarse al estante con `node tests/e2e_remote_workspace.cjs --extensions-only`. Usa el HTML actual de ComandOS y verifica el espacio de la terminal, el logo/modelo/esfuerzo, el cierre y el área visible después del zoom. El contrato actual de zoom ajusta la app al área visible; la antigua expectativa de mantener la altura completa dejó de ser válida.

## Procedencia y tamaño de las burbujas

Los grupos plegables usan procedencia registrada: repositorio GitHub del registro de instalación de skills o identificador del plugin. El registro se aplica únicamente al archivo instalado bajo el directorio compartido, resolviendo enlaces simbólicos. Una skill de proyecto con el mismo nombre no hereda ese origen. Si falta procedencia, aparece **Origen no registrado**. Los MCPs sin plugin se agrupan por la configuración que los declara, incluido el catálogo compartido.

El tamaño usa tokens de referencia **cl100k_base**, con tiktoken 0.12.0. En skills cuenta el archivo principal completo, incluido su frontmatter; no suma archivos auxiliares, referencias ni recursos. En MCPs cuenta JSON canónico de nombres, descripciones y esquemas de entrada/salida de las herramientas observadas. No representa consumo por turno, tokens facturados ni contexto efectivamente cargado. El área de la burbuja sigue una escala fija, limitada a diámetros de 76–152 px para mantener legibilidad. **Sin medir** usa 88 px y nunca equivale a cero.

Las listas MCP se miden pasivamente al atravesar el proxy compartido, solo tras completar la paginación. Los servidores stdio que pasan directamente al CLI y los procesos ya abiertos sin esta captura pueden seguir sin medición. No se arranca ni reinicia un MCP para medirlo. Solo se guarda el recuento, las huellas de configuración/contenido y la fecha; la medición caduca al cambiar la configuración o después de 24 horas.

El instalador de extensiones prepara el tokenizador y su caché. El inventario no descarga archivos de codificación; si falta el tokenizador o su caché verificada, muestra **Sin medir**. Cerrar un grupo se conserva durante las actualizaciones de ese panel.
