# Referencias de controles de sesión

El trabajo mejora controles funcionales de ComandOS y su uso desde un teléfono. La implementación conserva la tipografía, los colores y los componentes existentes.

Mobbin no aparece entre las herramientas conectadas de esta sesión. No se consultaron pantallas de Mobbin ni se atribuyen decisiones a referencias de esa plataforma. No se generaron propuestas visuales alternativas: se implementaron los controles sobre la interfaz existente y se verificaron con pruebas aisladas de navegador.

| Fuente consultada | Qué se revisó | Uso |
|---|---|---|
| [Interfaz existente](../dash/index.html) | Tema, selector, tarjetas, chat y formulario de inicio. | Sí: reutilización de estilos y flujos. |
| [Terminal existente](../dash/term.html) | Gestos, entrada, historial y barra de acciones. | Sí: selección nativa y compositor sobre el terminal existente. |
| [SQLite: usos apropiados](https://www.sqlite.org/whentouse.html) | Almacenamiento local de una aplicación. | Sí: decisión de persistencia; no es una referencia visual. |
| [SQLite: WAL](https://www.sqlite.org/wal.html) | Concurrencia de lectura y escritura. | Sí: persistencia local; no es una referencia visual. |

La dirección aplicada es un selector con un solo envío, tarjetas para comparar sesiones y controles táctiles accesibles. Un mosaico que renderice todas las terminales no se usa para el resumen, para evitar trabajo de renderizado innecesario.

Los archivos del trabajo visual son `DESIGN.md`, `proof.html` y `shots/`. Las capturas muestran datos de prueba; no son evidencia de validación en un teléfono físico.


## Pomodoro: pixel art de artistas, 2026-09-28

Jesús rechazó los vectores arcade dibujados para el mockup y pidió assets pixel art abiertos de mejor calidad. La distribución B, Regla de tiempo, sigue elegida. Esta ronda compara cinco conjuntos dentro del mismo mockup, sin reabrir la distribución ni cambiar producción. Las búsquedas y lecturas de las páginas se hicieron con la skill busqueda-web; después se descargaron los archivos gratuitos publicados por los autores. Las imágenes se inspeccionaron directamente y en el prototipo.

| Fuente | Qué se revisó | Uso y motivo |
|---|---|---|
| [Shikashi, pack gratuito](https://shikashipx.itch.io/shikashis-fantasy-icons-pack) | Sprites transparentes de 32 px, reloj de arena, gemas, fogata, plantas, cofre. Página actual CC BY 4.0; notas del ZIP de 2020 conservan referencia CC BY 3.0 de game-icons.net. | Sí. Conjuntos Fantasía y Jardín; reloj y cofre compartidos con Arcade. Se acreditan Matt Firth y game-icons.net. |
| [La Red Games, Gems / Coins Free](https://laredgames.itch.io/gems-coins-free) | Tiras originales de monedas y gemas de 16 px, licencia CC0 indicada por el autor. | Sí. Propuesta inicial Arcade, con animación real de cinco cuadros para monedas y cuatro para gemas. |
| [Quintino Pixels, Free Assorted Icons](https://quintino-pixels.itch.io/assorted-icons) | PNG transparentes de 32 px: reloj, frascos, llave y moneda. CC0 en título y confirmación del autor. | Sí. Conjunto Alquimia, con movimiento CSS añadido a iconos estáticos. |
| [7Soul, compilación en OpenGameArt](https://opengameart.org/content/496-pixel-art-icons-for-medievalfantasy-rpg) | Reloj, medallas, cristal, cofre y fuego. Compilación CC0 que excluye derivados de juegos comerciales. | Sí. Conjunto RPG clásico. Se conservan atribución y referencias de licencia ante las etiquetas históricas diferentes. |
| [7Soul, publicación del autor](https://www.deviantart.com/7soul1/art/420-Pixel-Art-Icons-for-RPG-129892453) | Descripción del autor: Public Domain y eliminación de derivados; metadatos aún CC BY 3.0. | Sí. Corroboración y atribución conservada a Henrique Lazarini. No se presenta la inconsistencia como resuelta. |
| [7Soul, pack actual de pago](https://7soul.itch.io/7souls-rpg-graphics-pack-1-icons) | Producto distinto, más de 1700 iconos. | No. No se compró ni se copió; no confundir con la compilación antigua abierta. |
| [Kenney, Pixel UI Pack](https://kenney.nl/assets/pixel-ui-pack) | Preview y ZIP CC0 de paneles, botones y flechas. | No. Útil para interfaz, pero carece del conjunto de reloj y recompensas que necesita esta ronda. |

El inventario, hashes de archivos y créditos de los packs están en /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-grill/dash/prototypes/assets/pixel-packs. Los PNG originales no se modificaron. CSS selecciona celdas y añade movimiento; La Red conserva sus cuadros originales. La elección visual sigue pendiente del usuario.


### Alquimia: el usuario exige movimiento por fotogramas

Tras elegir Alquimia, Jesús pidió sprites cuya animación cambie realmente el dibujo. La propuesta anterior de Quintino era estática y se movía con CSS; no satisface ese nuevo requisito. La siguiente versión conserva la dirección de alquimia y usa fotogramas originales de karsiori, con sus recipientes inmóviles mientras se animan burbujas, líquido y destellos.

| Fuente | Revisión | Uso |
|---|---|---|
| [karsiori, pociones animadas](https://karsiori.itch.io/pixel-art-potion-pack-animated) | Licencia CC0 explícita, spritesheets PNG, rate sugerido de 10 fps. Se inspeccionó el archivo original y sus cuadros. | Sí: seis sprites de 7 a 24 fotogramas. Una poción de foco reemplaza visualmente el reloj de arena estático; el contador sigue mostrando el tiempo real del mockup. |
| [karsiori, llaves animadas](https://karsiori.itch.io/pixel-art-key-pack-animated) | Página y licencia CC0; 10 tipos animados. | No: alternativa disponible para futuros logros, no se descargó ni mezcló en esta revisión. |
| [karsiori, cofres animados](https://karsiori.itch.io/pixel-art-chest-pack-animated) | Página y licencia CC0; secuencias de apertura y cierre. | No: esta revisión muestra una familia de pociones coherente. |
| [karsiori, faroles animados](https://karsiori.itch.io/free-pixel-art-lantern-pack) | Página y licencia CC0; siete tipos animados. | No: posible recurso posterior, no se descargó. |

La galería ofrece una vista de los fotogramas originales. Es una herramienta de revisión del mockup, no un control de producción. Se debe validar visualmente esta nueva familia con el usuario; aceptar el estilo Alquimia no implica haber aprobado ya los sprites concretos de karsiori.


### Catálogo de estilos de Pomodoro

Jesús pidió conservar varios estilos intercambiables, incluidos los que ya había visto, en vez de elegir uno para excluir los demás. El catálogo mantiene Alquimia, Arcade, Fantasía, RPG clásico y Jardín. Añade Cristales con sprites originales de karsiori. La elección aplica en un toque y conserva el tiempo, los XP y los logros.

| Fuente | Revisión | Uso |
|---|---|---|
| [karsiori, gemas animadas](https://karsiori.itch.io/free-pixel-art-gem-pack) | Página CC0, seis hojas PNG originales inspeccionadas; 10 u 11 cuadros según la gema. | Sí: nuevo estilo Cristales, con seis símbolos por fotogramas. |
| [karsiori, flores](https://karsiori.itch.io/free-pixel-art-flower-pack) | Licencia CC0; ilustraciones estáticas, no confirma animación. | No: no resuelve el requisito de sprites para Jardín. |
| [karsiori, pinos animados](https://karsiori.itch.io/spruce-tree-pack-pixel-art-animated) | Licencia CC0; árboles animados de 52 a 128 px de alto. | No: requeriría rediseñar Jardín y sus tamaños. Se conservan las plantas que el usuario ya vio. |
| [Perfil de karsiori](https://karsiori.itch.io/) | Catálogo de packs del artista y enlaces a gemas, flores y árboles. | Sí: descubrimiento de las fuentes; no se copiaron otras ilustraciones. |

Las tarjetas distinguen seis sprites originales, combinación de sprites/iconos, o iconos estáticos con movimiento CSS. No se presenta el movimiento CSS como animación dibujada. Convertir Fantasía, RPG clásico y Jardín a sprites originales completos sigue pendiente de diseño. La preferencia global de producto se propone para desktop y remoto; el prototipo guarda solo el estilo en el navegador y no sincroniza equipos.


### Visible action, rather than subtle glints, for Pomodoro

Jesús reports that the clock appears still and asks for more expressive original animations. Inspection reproduced the mismatch: Fantasía, Arcade, RPG clásico and Jardín still used static clock art with outer CSS movement; the separate 20 px header hand did animate, but its idle revolution took 24 seconds. Chrome on the remote Mac confirmed changing transforms and reduced motion disabled. This was an artwork/visibility problem in the mockup; no evidence ties it to tmux.

| Source | Verified evidence | Decision |
|---|---|---|
| [Zoedoz, Animated Hourglass](https://opengameart.org/content/animated-hourglass) | CC0; unchanged 15-frame PNG plus Aseprite source establishing 42×42 geometry and 100 ms timing. Frames show sand falling and the glass turning over. | Use as the common clock in all styles and the header. |
| [karsiori, animated chests](https://karsiori.itch.io/pixel-art-chest-pack-animated) | Explicit CC0; free ZIP contains Golden Chest 1, five distinct original frames. | Use in Arcade, Fantasía and RPG clásico; show opening/holding/closing in the review gallery. |
| [ArlanTR, animated campfire](https://opengameart.org/content/campfire-pixel-art-animated) | CC0; four distinct original 32×32 frames. | Replace the static fire in Fantasía and RPG clásico; include in the motion review. |
| [William.Thompsonj and Sharm, LPC Clock Animation](https://opengameart.org/content/lpc-clock-animation) | CC BY 3.0 / GPL 3.0 options; original sheet inspected. Tall pendulum clock; movement concentrated at the base. | Omit: the hourglass communicates motion more clearly at small UI sizes. |
| [Robert Brooks, animated sand timers](https://gamedeveloperstudio.itch.io/animated-sand-timers) | Paid download, £5 minimum, turn sequence and 15 states. | Omit; not an open/free pack and not purchased. |
| [ollieMarsh, animated cauldron](https://makinggamesinc.itch.io/cauldron-pixel-art) | Paid download, $1 minimum, nine frames. | Omit; not purchased and redistribution terms were not established. |

This revises the earlier note saying no animated hourglass was found. The shared hourglass and other new artwork are proposals for review, not a recorded user acceptance. The six styles remain selectable; unrelated static symbols remain clearly labeled.


### Notification placement round

The reference is the current ComandOS sidebar served by the isolated fixture, alongside the already accepted terminal workspace direction. Five alternatives compare placement of the same notice content: side inbox, floating notice, bottom strip, contextual tab/pane indicators and a full notification center. No external visual references or stock illustrations were used in this round. The line icons are inline SVG; the existing optional UISFX controller supplies local one-shot sound previews.

The approved most-recent-device sound policy is represented in fixture state only. Reliable event identity, real delivery and synchronization still require production work. In particular, the production focus queue must preserve the source pane identity if a later notification is expected to return to that exact pane. Notification suppression during Pomodoro remains an unresolved product choice.

## Referencias para complementar las decisiones del grill

La selección masiva de Carbon, las configuraciones reutilizables de Raycast y la bandeja de Linear aportan patrones concretos para revisar en ComandOS. Son propuestas complementarias a las decisiones aceptadas. Jesús pide investigación de referencias antes de plantear más refactorizaciones.

La consulta del 28 de septiembre de 2026 usa la skill 0xai-design-research. Mobbin es el motor solicitado y está configurado, pero su MCP falla durante initialize: connection closed. No se consultaron pantallas de Mobbin ni se accedió mediante scraping. La investigación disponible procede de documentación oficial, imágenes publicadas por los productos y Chrome remoto en la Mac mini.

El alcance es comparar interacciones para desktop y remoto. Se conservan el drag/drop de tabs, la restauración automática, los estados independientes, la regla de tiempo y la franja inferior con flotantes. Una referencia externa no cambia esas decisiones. Las diferencias de gestos y distribución móvil deben conservar las capacidades acordadas.

| Referencia | Evidencia consultada | Uso propuesto y límite |
|---|---|---|
| [Mobbin MCP](https://api.mobbin.com/mcp) | Intento de iniciar la conexión; fallo de handshake. | No utilizado como evidencia visual. Pendiente de conexión funcional. |
| [Linear Inbox](https://linear.app/docs/inbox) | Documentación, página en Chrome remoto y dos imágenes oficiales: bandeja y opciones de presentación. | Parcial: separar atención pendiente, lectura y posponer dentro de nuestra franja. No copiar su navegación lateral. |
| [Linear Notifications](https://linear.app/docs/notifications) | Documentación de canales, indicadores de habilitación y envío condicionado a lectura para resúmenes de correo. | Parcial: hacer explícito el estado de cada canal. No atribuir su política de correo a Telegram ni adoptarla automáticamente. |
| [Linear Mobile](https://linear.app/mobile) | Página oficial leída: acciones táctiles, posponer y horario de notificaciones. | Parcial: acciones alcanzables en móvil. Su enfoque de cliente complementario no satisface por sí solo la paridad completa requerida en ComandOS. |
| [VS Code Custom Layout](https://code.visualstudio.com/docs/configure/custom-layout) | Documentación, sección Editor groups inspeccionada en Chrome y captura oficial de separadores. | Parcial: ampliar temporalmente un grupo y volver a su tamaño, conservando el diseño elegido. Sus gestos de escritorio requieren adaptación táctil; no se proponen menús direccionales. |
| [VS Code Extension Marketplace](https://code.visualstudio.com/docs/configure/extensions/extension-marketplace) | Documentación de publisher, identificadores, filtros y habilitación por workspace. | Parcial: procedencia verificable y alcance visible. Instalado, habilitado, seleccionado y cargado por un proceso deben seguir siendo conceptos distintos. |
| [Raycast Agents](https://manual.raycast.com/ai/agents) | Documentación y dos capturas oficiales: configuración y agente activo en chat. | Parcial: configuraciones con nombre que reúnen modelo, instrucciones y herramientas; selección visible junto al compositor. La fuente no establece recomendaciones deterministas de costo/beneficio. |
| [TickTick Start Focus](https://help.ticktick.com/articles/7055782010496745472) | Documentación leída y página inspeccionada en Chrome; describe móvil, desktop, cambios de tiempo y sincronización. | Parcial: comenzar desde el contexto de trabajo y ajustar tiempo directamente. Se conserva nuestra regla horizontal. No se validó una sesión real del producto. |
| [TickTick Focus Statistics](https://help.ticktick.com/articles/7055781966800486400) | Documentación de tendencias, periodos, distribución, historial y filtros. | Parcial: relacionar totales con registros visibles. Sus estadísticas no prueban atención humana ni definen nuestras reglas de XP. |
| [Carbon Data Table](https://carbondesignsystem.com/components/data-table/usage/) | Documentación y captura del ejemplo de acciones por lote desde Chrome remoto. | Parcial: checkbox con estado parcial, contador de selección y barra de acciones; controles disponibles en touch y reserva de espacio al cargar. No convertir por eso toda la interfaz en tablas. |

La búsqueda adicional de selección masiva en Gmail devolvió conversaciones de soporte comunitario. No se abrieron ni se usaron como evidencia del comportamiento del producto. Se eligió Carbon por documentar explícitamente los estados y acciones del componente.

### Complementos recomendados para revisar

1. **Selección con alcance visible.** Conservar Todos y Ninguno globales, además de cada categoría. Mostrar cuántos elementos se seleccionaron y un estado parcial cuando corresponda. Carbon fundamenta los estados de selección; el alcance global proviene del requisito de Jesús. El costo es reservar espacio para ese resumen, especialmente en móvil.
2. **Configuraciones de IA reutilizables.** Ofrecer acceso directo a combinaciones guardadas y mostrar cuál está aplicada. Antes de aplicar una recomendación, mostrar qué modelo y herramientas cambia y el motivo verificable. Raycast fundamenta la reutilización; las recomendaciones deterministas siguen siendo diseño propio pendiente. El costo es mantener esas combinaciones cuando cambian modelos o herramientas.
3. **Atención con contexto.** Separar avisos pendientes de intervención del historial informativo y distinguir leído, pospuesto y resuelto. Linear fundamenta la distinción entre acciones de bandeja y trabajo. Se conserva la franja con flotantes. La alternativa es una lista cronológica simple, con menos controles pero más esfuerzo para localizar pendientes.

Como complementos secundarios, VS Code aporta ampliación temporal de grupos y TickTick aporta inicio contextual y análisis del tiempo. No se incorporan automáticamente. La restauración de conversaciones, las acciones seguras de Telegram y el costo real en tokens requieren evidencia del sistema de ComandOS; una captura externa no resuelve esos contratos.

### Evidencia y alcance de esta investigación

Se inspeccionaron seis imágenes: bandeja y opciones de Linear, chat y ajustes de Raycast, grupos de VS Code y selección por lote de Carbon. También se guardó una captura de la documentación de Linear. Las capturas del navegador pertenecen a la Mac mini y se copiaron explícitamente al workspace. Son ejemplos oficiales y capturas de documentación, no pruebas de uso autenticado de esas aplicaciones.

Los intentos de descargar imágenes de TickTick y dos imágenes de Carbon directamente desde este equipo fallaron. Carbon sí se pudo capturar desde el navegador remoto. La recomendación de TickTick se apoya en su documentación, sin afirmar revisión visual completa en móvil.

La evidencia local está en /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-grill/design/shots/research-complements. El inventario de fuentes está en /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-grill/design/shots/research-complements/sources.json. Las imágenes se usan para revisión, no como assets del producto.

No se generaron glimpses, branding, una mascota ni HTML nuevo: el alcance pedido es buscar referencias complementarias. No hay cambio de stack, importación de componentes ni modificación de tokens visuales. Las propuestas todavía requieren evaluación dentro del mockup de ComandOS y la revisión humana ya acordada.


## Mobbin recuperado y detalle de la franja de avisos · 29 de septiembre de 2026

Jesús indicó detener la investigación hasta recuperar Mobbin. Se corrigió el curso: las fuentes oficiales anteriores se conservan como evidencia independiente, no como sustituto autorizado de Mobbin.

El cliente MCP integrado seguía fallando al inicializar. El transporte configurado, `/home/someguy/.local/bin/cc-extensions serve mobbin`, respondió al healthcheck y a un cliente MCP acotado que usa la misma autenticación compartida. Se obtuvieron sus tres herramientas y se ejecutaron tres búsquedas reales mediante `search_screens`. No se abrió otro login, se duplicaron credenciales ni se scrapeó Mobbin. La descarga inicial no seguía una redirección HTTP 302; al habilitar redirecciones se descargaron las imágenes originales desde los enlaces devueltos por el MCP. Se inspeccionaron las ocho imágenes. Cada cliente temporal se cerró al terminar. Esto recupera la investigación; no prueba que las herramientas integradas de esta conversación se hayan reconectado.

Consultas: bandeja de notificaciones web, bandeja de avisos iOS y bandeja de Linear web. Modo standard, límites 4, 3 y 1. Los resultados no se describieron a partir de metadatos. Algunos resultados son tangenciales y se descartan abajo.

| Pantalla Mobbin | Qué se vio en la imagen | Uso en esta ronda |
|---|---|---|
| [Klaviyo · helpdesk](https://mobbin.com/screens/8f7ead49-4c06-4459-b340-dbfd6a8f7834) | Lista de conversaciones junto al detalle seleccionado, acciones en la cabecera y estado Open. | Parcial. Comparar lista con vista previa en D; no copiar sus cuatro columnas ni atribuirle una bandeja de permisos. |
| [Basecamp · Latest Activity](https://mobbin.com/screens/b71d21b5-1078-44a4-adb3-9583531a6059) | Actividad por día, hora, proyecto y autor, filtros visibles y acceso inferior a Pings/New for you. | Parcial. Cronología en A y contexto de proyecto en B. El agrupado por proyecto de B es una adaptación propia. |
| [Canny · changelog](https://mobbin.com/screens/2c6b438e-5b00-44f2-aa82-e3a05ed47c67) | Publicaciones extensas, filtros Draft/Scheduled/Published y All/New/Improved/Fixed. | No. Es administración de publicaciones; no fundamenta la atención de avisos. |
| [Tana · Activity](https://mobbin.com/screens/05899178-5ec3-4a5e-831e-703561fd25ec) | Actividad reciente y columna Notifications, puntos de no leído, Mark all read y filtro Unread. | Parcial. Filas compactas y lectura separada de la acción de trabajo. No demuestra prioridades ni permisos. |
| [Mindtrip · Updates](https://mobbin.com/screens/3c451683-6c3d-4c76-be6a-0a129143ba3a) | Contador de no leídos, punto junto a cada aviso nuevo, título, extracto y hora en una lista móvil. | Sí. Contador visible y origen legible en las variantes móviles. No se copian emojis ni imágenes. |
| [LinkedIn · Notifications](https://mobbin.com/screens/2f976b2e-5bd9-4b99-ab34-12988fd7d200) | Filtros All/Jobs/My posts/Mentions sobre una lista, diferencias de fondo y puntos para avisos. | Parcial. Filtros táctiles en la franja; se evita mezclar sugerencias y publicidad con avisos operativos. |
| [Mesh · Home](https://mobbin.com/screens/8db45ea4-7352-4edf-bd49-6f68e0b8c791) | Grupos por día y menú Filter by/Dismiss All Items. | No para la acción global. Ocultar todo podría confundir lectura con resolución. En ComandOS se propone Marcar todos leídos y conservar permisos pendientes. |
| [Linear · issue](https://mobbin.com/screens/cef36326-d8ec-4c6f-acd4-a9f1e1060d33) | Detalle de un issue con propiedades y actividad; la bandeja solicitada no está abierta. | No para esta ronda. No se atribuyen patrones de inbox a esta captura. |

### Cinco propuestas dentro de la decisión ya aceptada

Todas mantienen franja inferior y aviso flotante, acceso al pane de origen, lectura independiente del permiso, ejemplo de pane cerrado, errores, carga, vacío y desconexión. Las acciones permanecen simuladas. No se conecta Telegram ni se cambia su política de entrega.

- A, Por tiempo. Lista compacta de eventos recientes. Facilita rastrear qué pasó; exige buscar los permisos entre los demás avisos.
- B, Por proyecto. Columnas en escritorio y grupos apilados en móvil. Facilita retomar un proyecto; requiere recorrer los grupos para comparar toda la actividad.
- C, Primero tu respuesta. Recomendación para evaluar: permisos pendientes separados de la actividad. La prioridad procede del tipo de evento y del estado pendiente, no de IA ni de una captura externa. En móvil se apilan las dos secciones.
- D, Vista previa. Lista y detalle en la franja. En móvil el detalle reemplaza la lista y ofrece volver. Evita cambiar de pane antes de leer, a cambio de una interacción adicional.
- E, Uno a uno. Un aviso detallado con flechas para recorrer. Hace cómodo revisar cada evento, pero da menos visión global. Es una hipótesis propia, no un patrón atribuido a Mobbin.

No se abre de nuevo la decisión sobre la ubicación de las notificaciones. Esta ronda baja un nivel al orden, densidad y acción de cada aviso. Las variantes siguen pendientes del veredicto humano. No hay mascota, branding, dependencias nuevas ni importación de componentes; se usa el HTML existente y SVG inline. El pedido explícito de grill-design autoriza las cinco variantes interactivas dentro del mismo prototipo, sin otro paso de permiso para producirlas.

Evidencia original: /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-grill/design/shots/mobbin-notifications. Los inventarios son /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-grill/design/shots/mobbin-notifications/web-results.json, /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-grill/design/shots/mobbin-notifications/ios-results.json y /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-grill/design/shots/mobbin-notifications/linear-results.json. Las imágenes se usan como evidencia de revisión, no como assets de la interfaz.

Prototipo actualizado: /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-grill/dash/prototypes/prototype-v1-grill.html. Ruta de revisión: `?round=notification-detail&variant=C`. Las rutas anteriores mantienen la ronda de ubicación y su elección C.


### Veredicto: por proyecto; continuación dentro de cada grupo

Jesús eligió B, Por proyecto: "por proyecto me gusta". La ronda anterior ahora marca B como elegida y abre esa base por defecto. C, Primero tu respuesta, queda como alternativa no seleccionada.

La siguiente comparación conserva el agrupado por proyecto y contrasta lista completa, pilas por pane, pestañas de panes, avisos con detalle y filtros por tipo dentro de cada proyecto. Se reutilizan las ocho capturas de Mobbin ya inspeccionadas en esta investigación. No se hizo otra búsqueda ni se atribuyen a Mobbin las pilas de terminales, la identidad de pane o las reglas de permisos: esas adaptaciones son hipótesis propias de ComandOS pendientes del veredicto. Tana fundamenta la separación visual entre filas y lectura; Klaviyo aporta el contraste lista/detalle; Mindtrip y LinkedIn aportan contadores y filtros visibles en móvil. No se copian assets externos.

Se verificó el prototipo con Chrome remoto en la Mac mini a 1440×900, 820×1180, 390×844, 667×375 y 320×568. La limitación de navegador ocupado de la ronda anterior dejó de bloquear esta verificación. Se corrigió el selector de revisión que tapaba el pie de la app, y en pantallas bajas se usa scroll para evitar que terminal y franja se solapen. B ya está aceptada para agrupar proyectos; B de la nueva ronda, Pilas por pane, solo es una recomendación para evaluar.

Fuente actualizada: /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-grill/dash/prototypes/prototype-v1-grill.html. Registro de decisiones y comprobaciones: /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-grill/dash/prototypes/README-v1-grill.md. Capturas locales: /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-grill/design/shots/notification-projects/comandos-notification-projects-mobile-20260929.png y /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-grill/design/shots/notification-projects/comandos-notification-projects-desktop-20260929.png. Proceden de archivos generados en la Mac mini y transferidos explícitamente.

## Lector de novedades de IA · 29 de septiembre de 2026

La pregunta visual es cómo leer resúmenes dentro de CommandOS conservando acceso a la terminal. Jesús pidió ver el diseño mediante grill-design. Se construyen cinco variantes en la ruta existente, con el mismo contenido ficticio y los mismos controles en escritorio y remoto. La frecuencia de las notificaciones sigue pendiente; esta ronda no la decide.

Motor: MCP Mobbin, mediante el transporte autorizado /home/someguy/.local/bin/cc-extensions. Dos búsquedas reales, web e iOS, en modo standard con tres resultados cada una. Se descargaron e inspeccionaron las seis imágenes originales. La búsqueda y las imágenes respondieron sin errores. No se reutilizaron sus contenidos editoriales como noticias ni sus imágenes como assets del producto.

| Pantalla | Qué se inspeccionó | Uso |
|---|---|---|
| [GetYourGuide](https://mobbin.com/screens/7771ec75-2458-4381-9cbe-f0cb87d4b027) | Columna de artículo con subtítulos, enlaces y un índice lateral separado de las recomendaciones. | Parcial. Ancho de lectura y jerarquía de texto. Se descarta la columna promocional. |
| [Mintlify](https://mobbin.com/screens/b409b4ea-993e-4cb5-ae9e-8aaab0558700) | Navegación de documentos junto a un artículo; conversación separada en otra columna. | Parcial. Lista y detalle en A; contexto de trabajo junto al documento en C. No se copia el editor ni el botón de publicar. |
| [Microsoft Copilot](https://mobbin.com/screens/602ae92b-9bf8-49dd-9e44-7632bab04bf0) | Lectura extensa con índice lateral, subtítulos y referencia junto al texto. | Sí. Índice y recorrido editorial en B; fuentes en el artículo. No se verifica ni reutiliza la noticia de la captura. |
| [Public](https://mobbin.com/screens/d4ff9a4c-6714-4e65-8de5-2efe0640ca77) | Lista móvil con fuente, antigüedad, titular y extracto; categorías horizontales. | Sí. Filas legibles en A y filtros desplazables. Se descartan cotizaciones y miniaturas decorativas. |
| [Perplexity](https://mobbin.com/screens/4873615d-9df3-4992-b758-d6dac2cad573) | Resumen móvil con titulares desplegables, un texto abierto y control de fuentes. | Sí. Expansión en el lugar en D y lectura breve antes del artículo. |
| [LinkedIn](https://mobbin.com/screens/9328b77e-e61b-482e-ba68-35852db74f05) | Navegación de noticias con flechas y puntos de progreso sobre publicaciones relacionadas. | Parcial. Recorrido anterior/siguiente y posición visible en E. Se descartan comentarios, reacciones y contenido social. |

Direcciones: A, Lista + lector, favorece elegir una noticia y leerla con espacio; C, Junto a la terminal, conserva el trabajo visible a costa del ancho; D, Temas desplegables, favorece explorar categorías a costa de más aperturas. A es la recomendación inicial para lectura cómoda desde el celular. Ninguna está elegida por el usuario.

Stack: HTML, CSS y JavaScript del prototipo existente; iconos SVG propios, fuentes del sistema y contenido en memoria. Las cinco variantes sustituyen los glimpses separados porque el usuario pidió expresamente la ronda interactiva dentro de CommandOS. No se introduce mascota, branding ni una dependencia de renderizado. El pequeño parser de Markdown se limita al contenido fijo del ejemplo y no constituye una solución para renderizar noticias externas en producción.

Inventario de Mobbin: /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-grill/design/shots/mobbin-news-reader/results.json. Imágenes originales: /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-grill/design/shots/mobbin-news-reader. Capturas del mockup A, B, C, D y E en escritorio y móvil: /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-grill/design/shots/news-reader. Todas las capturas del mockup se generaron en Chrome de la Mac mini y se transfirieron explícitamente a este equipo.

Prototipo: /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-grill/dash/prototypes/prototype-v1-grill.html. Ruta: https://nodo-01.tail63a117.ts.net:8444/prototypes/prototype-v1-grill.html?round=news-reader&variant=A. El registro de verificación está en /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-grill/dash/prototypes/README-v1-grill.md. Tokens y alcance: /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-grill/design/DESIGN.md. Prueba visual: /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-grill/design/proof.html.
