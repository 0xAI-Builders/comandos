# Integración y aceptación: plan de implementación

> **Para el agente implementador:** usar `executing-plans`, `verification-before-completion` y `requesting-code-review` al cerrar los bloques. Las restricciones globales están en /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-grill/docs/superpowers/plans/2026-09-29-commandos-v1/README.md.

**Objetivo:** entregar un candidato instalable cuyos acuerdos estén probados, con aceptación humana y reversión preparada.

**Arquitectura:** integrar los bloques sobre la app existente, con migraciones idempotentes y pruebas aisladas. Registrar por separado pruebas de modelos puros, backend, terminal real de ensayo, navegador remoto y dispositivos humanos.

**Tecnologías:** Git, pytest, Node, tmux con sockets privados, Chrome de la Mac mini y el Android del usuario.

## R1. Regresión, carga y convivencia de componentes

**Modificar según los resultados:**

- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/tests/e2e_remote_workspace.cjs
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/tests/e2e_sidebar_parity.js
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/tests/e2e_session_workspace.js
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/tests/test_dashboard_layout.py
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/tests/test_usage_performance.py

**Crear:**

- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/tests/test_v1_integration.py
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/docs/verification/commandos-v1.md

**Consume:** W/E/P/N terminados y su estado compartido. **Produce:** matriz de resultados con commit y pruebas reproducibles.

- [ ] En el checkout de implementación, ejecutar las suites de cada bloque y estas regresiones. Los comandos son futuros; esta entrega de planificación no las ha ejecutado:

```sh
cd /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation
/tmp/comandos-v1-qa/bin/python -m pytest -q /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/tests
bash /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/tests/test_js_parses.sh
git diff --check
```

Antes de ejecutar toda la carpeta de pruebas, inspeccionar fixtures y excluir cualquier test que contacte el servidor habitual o use credenciales reales. Sustituirlo por un equivalente aislado y registrar qué quedó excluido. Las suites de navegador no se ejecutan desde este comando en Linux.

- [ ] Conservar la protección ya existente de resize de columnas, selección, enlaces, historial y teclado remoto. Probar que docking, barra y lector no cambian conexiones o pane activo al actualizar datos.
- [ ] Comprobar resaltado ANSI en VTE y xterm antes/después de reanudar, redimensionar y mostrar el lector. No resolver un problema visual limpiando el buffer o perdiendo scrollback.
- [ ] Abrir/cerrar repetidamente extensiones, lector, Pomodoro y chat; verificar desmontaje de listeners, observers, timers, audio y terminales. Las vistas ocultas no crean sondeos duplicados ni cargan historiales completos.
- [ ] Medir proceso del dashboard y clientes por separado durante reposo, cambio de tabs y ráfaga de eventos. Registrar tiempo hasta contenido útil, solicitudes simultáneas, memoria inicial/tras estabilizar y tamaño de respuestas. Comparar con el mismo fixture en main; no atribuir la memoria de todos los agentes a la UI.
- [ ] Probar payload de 20 tabs con dos panes por tab usando la fixture existente, sin crear 40 agentes reales. La adaptación responsive no debe multiplicar terminales por cada render.
- [ ] Revalidar sincronización de modelos con /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/tests/test_model_watch_discovery.py y el catálogo de /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/lib/model_catalog.py. Preservar cuentas y configuración activa; descubrir un modelo no autoriza cambiarlo.
- [ ] Guardar commit `test: cover workspace notification and focus integration` cuando existan cambios de pruebas o correcciones.

## Navegador remoto

MCP `chrome-bg` es el runtime autorizado para inspección interactiva. Exponer el servidor candidato mediante /home/someguy/.local/bin/cc-browser-expose y abrir la URL reenviada en la Mac mini. Si el servicio falla, informar el bloqueo; no iniciar otro navegador local.

Las suites Playwright del repo son un runtime aparte. Verificar en la Mac `node`, módulo de Playwright y ejecutable de navegador; la ruta fija dentro de la suite no demuestra su existencia. Corregir el runner para recibir una ruta verificada si hace falta. Copiar únicamente código y fixtures necesarios, sin credenciales, directorios de usuario, worktrees anidados ni tokens de servicios.

Destino remoto propuesto para crear en la Mac mini: /tmp/comandos-v1-browser-qa. No existe como parte de esta entrega. Si ya contiene trabajo ajeno, usar otra ruta registrada. Copiar capturas de vuelta a /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/docs/verification/shots, directorio propuesto para crear.

Comprobar 1440×1000, 820×1180, 390×844, 320×568 y 667×375, además de zoom, teclado móvil y orientación. Revisar screenshots y consola; dimensiones sin overflow no prueban que los controles sean fáciles de usar.

## R2. Recorrido humano por bloques

**Modificar:** /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/docs/verification/commandos-v1.md.

**Consume:** candidato con datos de ensayo y los prototipos elegidos. **Produce:** veredicto de Jesús por escenario, sin aprobar áreas pendientes por asociación.

- [ ] Preparar un candidato con estado/sockets temporales. Si la app nativa aún fija rutas globales, aislarlas en el harness de pruebas antes de abrirla. No usar sesiones personales para ensayar caídas o recuperación.
- [ ] Guiar a Jesús en este recorrido y registrar pasa/falla, comentario y evidencia:

| Escenario | Acción humana | Resultado que debe comprobar |
| --- | --- | --- |
| Retomar trabajo | Ordenar tabs, escribir un borrador, mover scroll y reabrir. | Misma distribución, conversación y texto; sin selector de backup. |
| Distribuir | Arrastrar una tab al borde interior/exterior, ajustar y desacoplar. | Preview correcto y ningún pane duplicado o reiniciado. |
| Dos dispositivos | Escribir en panes distintos en escritorio y Android. | Distribución común e input/foco independientes. |
| Cerrar | Cancelar cierre de grupo y luego confirmar un grupo de ensayo. | Cancelar conserva todo; confirmar afecta solo lo enumerado. |
| Estados | Marcar sesión y un pane, crear otro y enviar prompt desde Resuelto. | Sin herencia, favorito independiente, reapertura del pane correcto. |
| Terminal rápida | Pulsar Terminal dos veces y abrir Nueva sesión. | Carpetas fechadas distintas y formulario solo en Nueva sesión. |
| Extensiones | Buscar, quitar todos y añadir una categoría. | El batch global incluye elementos fuera del filtro; no confunde uso/carga. |
| Pomodoro | Iniciar, pausar, extender, cambiar de dispositivo y cancelar/completar. | Tiempo compartido y un registro por bloque; estilo no altera el reloj. |
| Avisos | Recibir eventos de ensayo mientras escribe. | No cambia foco; abrir llega al origen; el sonido respeta la política elegida. |
| Android | Activar prueba y bloquear la pantalla. | Aviso breve y click al origen; registrar cualquier restricción real. |
| Novedades | Leer Markdown, mostrar terminal y volver al artículo. | Terminal a la izquierda, ancla y borrador conservados, fuentes navegables. |
| Chat conservado | Abrir/ocultar, cambiar de tab y volver a un borrador de ensayo. | Chat, historial y destino permanecen disponibles. |

- [ ] Resolver D1 a D6 en la demostración correspondiente y anotar cada respuesta literal con alcance. Un rechazo modifica ese bloque; no reabre decisiones distintas ya aprobadas.
- [ ] Reparar fallos y repetir solo escenarios afectados y dependencias directas. No declarar pruebas de Android físico a partir de emulación ni una escucha a partir de mocks de audio.

## R3. Migración, revisión y activación

**Modificar:**

- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/install.sh
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/README.md

**Crear:**

- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/docs/verification/commandos-v1-activation.md
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/tests/test_v1_migrations.py

**Consume:** candidato probado y migraciones W/P/N. **Produce:** instalación reproducible con reversión de app y conservación de datos.

- [ ] Enumerar cada cambio de almacenamiento y servicio: DB de estado, snapshots, foco antiguo, eventos históricos, estilos, suscripciones y retirada de Telegram. Crear migraciones con versión, backup previo y reejecución inocua.
- [ ] Probar interrupción en cada frontera de escritura y dos ejecuciones del instalador sobre un HOME temporal. Una app anterior debe poder mantener datos nuevos intactos; si no los entiende, no sobrescribirlos durante rollback.
- [ ] Preparar respaldo del estado antes de instalar el candidato. Registrar commit/versión previa, rutas concretas de respaldo, unidades que cambian y comandos de reversión. No poner tokens ni contenido de conversaciones en el informe.
- [ ] Revisar el diff completo contra main y ejecutar la revisión de código. Confirmar chat presente, Telegram retirado y ausencia de fixture UI, noticias ficticias, claves, archivos personales o bibliotecas sin licencia.
- [ ] Presentar a Jesús el candidato y el registro de R2 antes de la activación. La autorización de implementación no equivale a aceptar un resultado defectuoso; usar el alcance de autorización vigente sin repetir confirmaciones ya concedidas.
- [ ] Activar solo los servicios necesarios y sin terminar agentes ajenos. La migración de Telegram opera únicamente la unidad de CommandOS. Cambios de claves push requieren conservar o renovar suscripciones explícitamente, no invalidarlas por cada despliegue.
- [ ] Después de activar, hacer comprobaciones acotadas de salud, restauración normal y apertura remota. Si falla, revertir la app conforme al documento y conservar datos nuevos/snapshots; no reiniciar tmux para arreglar la interfaz.

## Criterio de cierre

El registro debe distinguir implementado, comprobado automáticamente, comprobado en dispositivos y aceptado por Jesús. Un requisito aprobado que siga desactivado por D2/D4/D5/D6 impide declarar ese bloque terminado. El release no está completo mientras restauración, foco independiente o entrega Android requerida carezcan de evidencia.

El rediseño de barra, controles de IA, Analytics general, servidores y otras áreas del handoff no se incorpora por conveniencia durante esta implementación. Registrar nuevos hallazgos allí y mantener el candidato centrado en los acuerdos de esta fase.
