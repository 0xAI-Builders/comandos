# Comparación del sidebar

El escritorio y el remoto comparten secciones, orden, estilos según el ancho del panel, métricas de hoy y controles de chat. Las capturas usan el mismo contenido de prueba en un sidebar de 380 px; el borde del remoto ocupa 1 px. Los teléfonos se verifican a 390 y 320 px.

| Vista | Antes | Después | Chat oculto |
|---|---|---|---|
| Referencia de escritorio | [Captura](sidebar-comparison/before-desktop.png) | [Captura](sidebar-comparison/after-desktop.png) | [Captura](sidebar-comparison/after-desktop-chat-hidden.png) |
| Remoto, columna de 380 px | [Captura](sidebar-comparison/before-remote.png) | [Captura](sidebar-comparison/after-remote.png) | [Captura](sidebar-comparison/after-remote-chat-hidden.png) |
| Teléfono de 390 px | [Captura](sidebar-comparison/before-phone390.png) | [Captura](sidebar-comparison/after-phone390.png) | [Captura](sidebar-comparison/after-phone390-chat-hidden.png) |
| Teléfono de 320 px | [Captura](sidebar-comparison/before-phone320.png) | [Captura](sidebar-comparison/after-phone320.png) | [Captura](sidebar-comparison/after-phone320-chat-hidden.png) |

La referencia de escritorio renderiza el HTML real con un puente GTK simulado en Chrome headless. No es una captura del motor WebKit nativo. Todas las API y terminales son fixtures aislados. La comparación no modifica sesiones, llama a comandos GTK ni activa ventanas del usuario.

La [captura inicial del remoto](sidebar-comparison/before-remote.png) muestra tarjetas en dos columnas recortadas y acciones apiladas dentro de un panel estrecho. Las reglas dependían del ancho de la ventana completa. Además, el [DOM inicial](sidebar-comparison/before-dom.json) muestra tres filas globales en remoto y ninguna en escritorio; las métricas solo aparecían en el escritorio.

Los [estilos compartidos](../dash/workspace.css) consultan el ancho del propio sidebar. El [render](../dash/index.html) muestra los splits de la sesión activa y mantiene accesibles Nueva sesión, Todas las sesiones, Perfiles y Abrir chat incluso sin sesiones. El inventario global queda disponible mediante Todas las sesiones. En remoto, cambiar de pestaña actualiza el destino del chat; el foco del escritorio no sustituye esa selección local. Elegir un split actualiza la tarjeta y el destino sin cambiar la terminal abierta.

El chat conserva su control de altura y la preferencia de visibilidad. Solo arrastrar el separador guarda una altura; las medidas transitorias del arranque no se guardan. En el [DOM final](sidebar-comparison/after-dom.json), escritorio y remoto tienen 915 px de contenido y 622 px de chat con la configuración inicial. Al ocultarlo, las mismas métricas y los splits ocupan el espacio disponible. En teléfonos, el panel y la terminal conservan su navegación mediante pestañas.

La [prueba de navegador](../tests/e2e_sidebar_parity.js) verifica las secciones, los splits activos, las métricas, el cambio de sesión y pane, la selección remota independiente, la apertura de analytics, el separador, la persistencia del chat, los controles sin sesiones y la ausencia de desbordamiento horizontal o errores JavaScript. El [lanzador pytest](../tests/test_sidebar_parity.py) omite esa prueba si faltan Chrome o Playwright.

Validación ejecutada en este checkout:

```sh
NODE_PATH=/tmp/comandos-terminal-test-deps/node_modules pytest -q tests/test_dashboard_layout.py tests/test_sidebar_parity.py tests/test_session_config_ui.py
NODE_PATH=/tmp/comandos-terminal-test-deps/node_modules node tests/e2e_session_workspace.js
```

Resultado: 13 pruebas pytest aprobadas y flujo de workspace aprobado. Las capturas verifican layout con datos de prueba; la integración nativa de WebKit y los gestos en un teléfono físico quedan fuera de esta prueba.

Comprobación adicional en el cliente GTK instalado: se confirmó el puente WebKit real, las secciones compartidas, la preferencia de chat oculto y el botón para recuperarlo. El motor anuncia soporte para container queries. Se detectó que una recarga podía conservar el CSS anterior; las referencias de workspace ahora llevan una versión basada en su contenido. Esta comprobación inspecciona el DOM nativo; las capturas anteriores siguen siendo fixtures de Chrome.

Tras cargar los assets versionados, WebKit confirmó `container-type: inline-size`, las secciones `centro-wrap`, `sessions-wrap` y `sidebar-insights`, el chat oculto con el botón de apertura visible y un intervalo de consulta de dos segundos. Los 28 paneles conservaron sus identidades de proceso durante la activación.
