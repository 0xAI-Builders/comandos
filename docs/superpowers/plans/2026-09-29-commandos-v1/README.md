# CommandOS 1.0: plan de implementación de lo acordado

> **Para el agente implementador:** usar `executing-plans` para ejecutar por tareas y registrar pruebas. La delegación es opcional si Jesús la autoriza. Las casillas pendientes describen trabajo futuro; no son resultados de esta sesión.

**Objetivo:** implementar los acuerdos cerrados del grilling, conservar el trabajo del usuario y entregar bloques verificables en escritorio y Android.

**Arquitectura:** mantener Python, SQLite, tmux, GTK/VTE y el cliente web existentes. Extraer módulos acotados para layout, eventos y temporizador. Ambos clientes consumen la misma autoridad para estado compartido y conservan su foco local.

**Tecnologías:** Python 3, SQLite, tmux, GTK/VTE, JavaScript, xterm/ttyd, service worker, pruebas Python/Node y Chrome remoto en la Mac mini.

## Alcance y estado de la entrega

Este paquete es un plan para otro agente. No implementa el release ni cambia sesiones, servicios, preferencias, credenciales o conversaciones reales. La palabra 1.0 identifica el objetivo de producto de Jesús; no autoriza rebajar la versión publicada ni sobrescribir tags existentes.

El usuario decidió implementar primero lo acordado y continuar el diseño restante en una segunda fase. La primera fase se implementó en `implementation/comandos-v1` y está en `main` (`b5dc4ed`), con activación R2/R3 pendiente de Jesús. La segunda fase se grilló el 2026-09-29 y su plan es el subplan 06: la barra izquierda pasa a ser un catálogo de comandos por CLI con cadenas y terminales rápidas, el chat de CommandOS se retira (esto revoca E4) y la cabecera pasa a la variante Ordenada. Telegram sigue retirado.

Los planes distinguen tres cosas: requisitos aprobados, decisiones técnicas propuestas y preferencias humanas sin resolver. Las últimas bloquean solo su activación concreta. No se inventan respuestas ni se omite el requisito del release.

## Restricciones globales

- Conservar procesos, conversaciones, carpetas, borradores y backups del usuario. Hacer pruebas destructivas únicamente con datos, sockets y sesiones aislados.
- El workspace restaura automáticamente su último estado válido. No introducir otra pantalla de recuperación histórica ni borrar las copias existentes.
- Compartir distribución entre dispositivos y mantener foco independiente. Una acción siempre conserva su pane de destino, incluso tras cambiar de tab.
- Mantener paridad funcional de escritorio y remoto; adaptar espacio, teclado y gestos. No tratar Android como visor.
- Automatización de navegador exclusivamente con MCP `chrome-bg` en `macmini`. Usar /home/someguy/.local/bin/cc-browser-expose para exponer puertos. No iniciar Chrome, Chromium, Playwright, Puppeteer ni Xvfb en Linux.
- Las suites Playwright se ejecutan en la Mac solo después de verificar su runtime allí. Una suite con ruta de ejecutable fija no prueba que ese runtime esté instalado.
- Capturas remotas pertenecen a la Mac; transferirlas explícitamente y dar su ruta absoluta en el equipo receptor.
- No enviar mensajes por Telegram, correo ni chat. Las notificaciones de prueba reales requieren que Jesús active la prueba desde su dispositivo.
- No ejecutar comandos ni cambiar configuraciones automáticamente por recomendaciones. Abrir un resumen no inicia investigación.
- No usar etiquetas de modelo, procesos vivos o silencio de terminal como prueba de trabajo, finalización, permisos o consumo.
- No mergear todo el branch de prototipos a producción. Extraer componentes aprobados; excluir datos ficticios, controles de laboratorio y rutas de simulación.
- No relanzar la app de trabajo del usuario para probar. La revisión humana del candidato precede a su activación en la instalación habitual.

## Dónde está cada cosa

| Recurso existente | Ruta absoluta |
| --- | --- |
| Checkout de referencia, rama main | /home/someguy/codebase/0xJesus/ComandOS |
| Worktree de diseño y este paquete | /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-grill |
| Historial de decisiones, leer los veredictos posteriores | /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-grill/dash/prototypes/README-v1-grill.md |
| Mockup fase 1, solo referencia visual | /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-grill/dash/prototypes/prototype-v1-grill.html |
| Mockup fase 2, composición aprobada sin parámetros | /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-grill/dash/prototypes/prototype-v2-barra.html |
| Fuentes de diseño investigadas | /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-grill/design/references.md |
| Planificación de la barra pendiente | /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-grill/design/sidebar-controls-plan.md |
| Handoff de la segunda fase, fuera del repo | /tmp/comandos-v1-fase-2-handoff.md |

Base inspeccionada de main: `c0c68cf76c74aebf5adae4b885f5e5ceca818427`. Evidencia de diseño previa a este paquete: `42e1ac7806fc4e9f8d20712c5be24ed41c592f54`. Verificar ambos contra Git al empezar; no asumir que siguen siendo HEAD.

## Preparación de ejecución

La siguiente ruta es **propuesta para crear**, no un checkout existente en esta entrega: /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation. Todas las rutas de cambios y pruebas en los subplanes apuntan a ese futuro checkout. Si ya existe, inspeccionarlo y continuar solo si corresponde a esta ejecución; no borrarlo ni reutilizar otro trabajo a ciegas.

- [ ] Leer este índice y el subplan del bloque que se va a implementar.
- [ ] Inspeccionar cambios locales y otros worktrees. El checkout principal tiene archivos ajenos sin seguimiento; conservarlos.
- [ ] Crear el checkout aislado desde main, sin tocar la app viva:

```sh
git -C /home/someguy/codebase/0xJesus/ComandOS status --short
git -C /home/someguy/codebase/0xJesus/ComandOS worktree list
git -C /home/someguy/codebase/0xJesus/ComandOS worktree add -b implementation/comandos-v1 /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation main
```

- [ ] Preparar un entorno de pruebas aislado. El Python de sistema inspeccionado no tiene pytest. No instalar paquetes en su entorno global.

```sh
python3 -m venv /tmp/comandos-v1-qa
/tmp/comandos-v1-qa/bin/python -m pip install pytest
/tmp/comandos-v1-qa/bin/python -m pip freeze > /tmp/comandos-v1-qa-requirements.txt
```

El entorno y su archivo de dependencias son rutas propuestas que se crean al ejecutar esos comandos. Instalar dependencias adicionales de una prueba solo dentro del entorno, registrando sus versiones. Ejecutar las suites desde el checkout de implementación para que las cargas de código por cwd apunten al candidato.

- [ ] Crear el registro de aceptación propuesto en /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/docs/verification/commandos-v1.md. Registrar commit, comando, resultado, entorno, limitación y veredicto humano por bloque.

## Orden de implementación

| Bloque | Plan absoluto | Depende de | Entregable |
| --- | --- | --- | --- |
| W | /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-grill/docs/superpowers/plans/2026-09-29-commandos-v1/01-workspace.md | Preparación | Restauración, docking, cierre y foco entre clientes. |
| E | /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-grill/docs/superpowers/plans/2026-09-29-commandos-v1/02-estados-terminal-extensiones.md | W para persistencia; N1 para actividad | Estados, shell rápida, selección de extensiones y compatibilidad del chat. |
| P | /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-grill/docs/superpowers/plans/2026-09-29-commandos-v1/03-pomodoro.md | N1 para eventos de finalización | Temporizador fiable, regla, arte y métricas. |
| N | /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-grill/docs/superpowers/plans/2026-09-29-commandos-v1/04-avisos-push-noticias.md | W para destino exacto | Eventos, avisos, lector, resúmenes y push Android. |
| R | /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-grill/docs/superpowers/plans/2026-09-29-commandos-v1/05-integracion-release.md | Bloques anteriores | Candidato probado con Jesús y guía de activación/reversión. |
| F (fase 2) | /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-grill/docs/superpowers/plans/2026-09-29-commandos-v1/06-barra-comandos-cabecera.md | main con la fase 1 (`b5dc4ed`); E2 para las terminales rápidas | Barra de comandos por CLI, tecleo sin Enter, cadenas, retiro del chat, cabecera Ordenada. Checkout propio `.worktrees/comandos-v1-fase2`. |

Orden recomendado de tareas: W1 a W4, N1, E1 a E4, P1, N2, P2 a P4, N3 a N5 y R1 a R3. N2 crea el adaptador de audio que P2 reutiliza. P4 se activa al cerrar D2. R no permite declarar terminado un bloque requerido que siga desactivado por falta de una decisión.

Cada tarea sigue un ciclo pequeño: reproducir o escribir la prueba de contrato, comprobar su fallo, implementar, ejecutar pruebas relacionadas y guardar un commit revisable. No escribir pruebas que solo busquen nombres CSS; probar persistencia, destino, límites y efectos observables.

## Almacenamiento compartido propuesto

W1 crea /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/lib/app_state.py con `connect(path)` y `migrate(connection)`. La ruta de ejecución propuesta es /home/someguy/.local/state/comandos/app-state.sqlite3; no es un archivo creado por esta planificación. La instalación resuelve la ubicación mediante XDG_STATE_HOME y permite inyectarla. Todas las pruebas usan una ruta temporal.

Workspace, marcas, eventos, entregas, temporizador y progreso usan esa base con migraciones versionadas, claves foráneas y transacciones explícitas. La conexión se abre con control manual de transacciones; `append_event(connection, event)` nunca confirma por su cuenta. Así P1 puede confirmar bloque, registro y evento de finalización juntos. Cada proceso abre su conexión; no se comparte una conexión SQLite entre hilos sin coordinación. Los informes de uso existentes siguen siendo una proyección o lectura, sin una segunda autoridad para finalizar Pomodoros.

N1 crea las tablas de eventos antes de que P1 las use. E1 y los demás módulos añaden sus tablas mediante la misma secuencia de migraciones. Guardar una copia antes de migrar y no modificar las bases reales durante las pruebas.

## Cobertura de los acuerdos

| Requisito aprobado | Tarea | Referencia del veredicto |
| --- | --- | --- |
| Restaurar último estado, reaprovechar procesos vivos, no resucitar cierres | W1, W4 | Historial: automatic restoration of the latest state. |
| Drag/drop de tabs completas, anidado, preview, separadores, desacoplar | W2 | Historial: flexible tab docking. |
| Número seguido de tabs; sin controles direccionales ni botón Mosaico | W2 | Historial: count followed by tabs; tab states and quick terminals. |
| Cerrar pane termina ese pane; cierre de grupo confirma alcance | W3 | Historial: closing a split; group-close confirmation. |
| Distribución compartida y foco independiente | W1, W3 | Historial: one shared arrangement; independent active tab and pane. |
| Favoritos y estados por sesión/pane sin herencia | E1 | Historial: no organizational-state inheritance. |
| Prompt aceptado quita Resuelto; iconos no emoji en loop | E1 | Historial: new prompt clears Resuelto; continuously animated state icons. |
| Terminal y Nueva sesión separadas; carpeta fechada por shell | E2 | Historial: a dated directory for each quick terminal. |
| Seleccionar/quitar todo, categorías reales, selección/carga/uso distintos | E3 | Pedido directo y corrección sin integrar del worktree de extensiones. |
| Conservar chat propio; MCPs/skills en pane | E4 | Corrección "SI DEJALA", **revocada el 2026-09-29**: el chat se sustituye por terminales rápidas (subplan 06, S2/S4). MCPs/skills siguen en el pane. |
| Comandos por CLI instalado, con explicación y versión verificada; yolo primero; todo plegable | 06 · C1, C2, S1 | Historial: "Aprobado: acordeón por CLI", "Corrección: todo plegable y arranque YOLO primero". |
| Un clic escribe letra por letra sin Enter | 06 · T1, S1 | Historial: respuesta Q1 de fase 2. |
| Fila de dos líneas con chips | 06 · S1 | Historial: "Aprobado: fila de dos líneas". |
| Cadenas guardadas plegables y tarjeta corriendo con Siguiente | 06 · K1, K2, S1, S3 | Historial: "Aprobado: cadena corriendo como tarjeta", "Cadenas con el mismo acordeón por CLI". |
| Cabecera Ordenada sin fila SSH permanente; Servidores intacto | 06 · H1 | Historial: "Aprobado: barra superior ordenada", "Veredicto: Servidores se conserva como hoy". |
| Pomodoro operativo, regla de tiempo, sonidos de videojuego | P1, P2 | Historial: Regla de tiempo y video-game sound direction. |
| Analytics de Pomodoro y gamificación | P3, P4 | Historial: Pomodoro analytics and personal gamification. |
| Seis estilos, Alquimia inicial, sprites reales y loops | P2 | Historial: selectable styles; expressive sprite direction. |
| Franja inferior, floats y grupos por proyecto | N2 | Historial: bottom strip; project grouping. |
| Sonido configurable y cliente de última interacción | N2 | Historial: most recent interaction; configurable notification sound. |
| Telegram fuera por completo | N3 | Historial: remove Telegram entirely. |
| Android recibe push al no ver la app; aviso breve con preview | N4 | Historial: phone push; Android; brief notice and preview. |
| Noticias IA/modelos/MCPs/skills/bounties en Markdown, tres resúmenes | N5 | Historial: news summaries; three daily editions. |
| Edición B, terminal opcional siempre a la izquierda, lectura móvil | N5 | Historial: approved edition with terminal on the left. |
| Desktop/remoto equivalentes, rapidez y revisión humana | R1 a R3 | Restricciones persistentes y petición de release probado con Jesús. |

Los ejemplos de mapas, XP, minutos objetivo, avisos, nombres de modelos y noticias del mockup no son datos ni preferencias reales. Los proveedores/modelos sincronizados y controles directos ya tienen código en main; conservarlos y probar regresión, no reimplementarlos desde una captura.

## Decisiones pequeñas que faltan dentro de estos bloques

| ID | Decisión pendiente de Jesús | Se puede construir antes | Momento de preguntar |
| --- | --- | --- | --- |
| D1 | Icono neutral tras finalizar y transición de Congelado/Esperando respuesta al llegar actividad nueva. | Estados independientes, evidencia de trabajo y transición aprobada desde Resuelto. No borrar bloqueos por inferencia. | Demostración E1, antes de cerrar esa UX. |
| D2 | Fórmula de XP, niveles, logros y objetivo diario. Los 10 XP/min, 1.000 XP/nivel y 100 min eran propuestas. | Registro exacto de tiempo y cálculo parametrizado probado con una política de ensayo. | Demostración P3, antes de habilitar recompensas reales. |
| D3 | Alcance de la preferencia de arte, global o por proyecto, y automatismo de ciclos/descansos. | Seis estilos y controles; conservar configuración vigente sin cambiar preferencias silenciosamente. | Demostración P2. |
| D4 | Horas de los tres resúmenes y presupuesto/alcance de investigación. 08/14/20 no está aprobado. | Cola, caché, lector, generador con dobles de prueba y agenda de tres slots configurables. | Demostración N5, antes de programar trabajos con gasto. |
| D5 | Sonido predeterminado por evento, duración/agrupación de floats, relación con Pomodoro y criterio de ausencia para push. | Preferencias, deduplicación y rutas probadas con reloj controlado. | Demostración N2/N4, antes de activar la política nueva. |
| D6 | Contenido visible en pantalla bloqueada y fallback si el último cliente no puede sonar. | Suscripción y aviso breve de ensayo; opciones de privacidad. | Prueba física N4. |

Estas decisiones quedan en la cola de revisión, no convierten todo el paquete en trabajo bloqueado. Consultarlas con una propuesta concreta al llegar al bloque; mientras tanto continuar con tareas independientes. No resolverlas por silencio ni usar una espera como aprobación.

## Segunda fase

Grillada y cerrada el 2026-09-29 (historial desde "Fase 2 · propuesta de Jesús y ronda 6" hasta "Cierre del grilling de fase 2"). Se implementa con el subplan 06. La política de cambios de IA dejó de ser una función orquestada: la barra ofrece los comandos nativos de cada CLI y el usuario los ejecuta. Servidores no se rediseña. Sin grillar y fuera de toda implementación: Analytics general/Reparto, entrada remota/reconexión, ajustes y utilidades, contexto de retorno.

No reintroducir Telegram, recuperación histórica manual, el árbol de proyectos ni el chat de CommandOS. El handoff conserva las preguntas y hallazgos, sin crear un segundo conjunto de decisiones.

## Entrega del agente implementador

Por bloque: diff, pruebas, capturas desktop/móvil cuando corresponda, limitaciones y una comprobación humana concreta. El cierre de R requiere evidencia de restauración, foco, notificaciones y Android físico. Si algo no está cubierto, informarlo como pendiente; un mockup aceptado o una prueba simulada no equivale a un release operativo.
