# Avisos, Android y novedades: plan de implementación

> **Para el agente implementador:** usar `executing-plans`. Aplican las restricciones y decisiones D4/D5/D6 de /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-grill/docs/superpowers/plans/2026-09-29-commandos-v1/README.md.

**Objetivo:** entregar avisos atribuibles al pane correcto, recibirlos en Android cuando CommandOS no esté visible y leer tres ediciones diarias de novedades con fuentes.

**Arquitectura:** persistir el evento antes de distribuirlo; cada canal consume una cola de entregas deduplicada. Separar actividad de agentes y novedades editoriales. Los contenidos publicados se cachean y el lector nunca ejecuta investigación al abrirse.

**Tecnologías:** Python/SQLite, hooks de harness, JavaScript, service worker y Web Push sobre el origen HTTPS existente.

## N1. Identidad de eventos y estado observable

**Crear:**

- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/lib/event_store.py
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/lib/turn_state.py
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/tests/test_event_store.py
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/tests/test_turn_state.py

**Modificar:**

- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/hooks/cc-notify.sh
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/adapters/codex-hooks.sh
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/bin/cc-dash
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/tests/test_notify_hook.py
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/lib/app_state.py

**Consume:** identidad W1 y payloads de hooks realmente disponibles. **Produce:** `append_event(connection, event)`, `list_events(after_sequence, limit)` y `reduce_turn(current, event)`. `append_event` acepta la conexión transaccional de P1; no abre otra transacción por dentro.

```json
{
  "eventId": "event-local-uuid", "sourceEventId": null,
  "source": "codex-hook", "harness": "codex",
  "projectKey": "project-a", "sessionKey": "session-a",
  "paneKey": "pane-a", "processKey": "server-pid-start",
  "conversationId": "conversation-a", "turnId": "source-turn-a",
  "requestId": null, "kind": "turn_completed",
  "evidence": "confirmed", "correlation": "source",
  "occurredAtMs": 1000, "receivedAtMs": 1100,
  "title": "Terminó el turno", "excerpt": "Respuesta disponible"
}
```

`kind` incluye prompt_accepted, turn_started, input_requested, permission_requested, turn_completed, turn_cancelled, turn_failed, pane_closed, focus_completed y news_edition. No convertir cualquier waiting en permission_requested. Un UUID local identifica una recepción, no demuestra identidad de turno del proveedor.

- [ ] Capturar fixtures sanitizadas de los payloads existentes y registrar por harness qué identidades suministra. Conservar campos de sesión/turno/request que hoy se pierden. No activar hooks nuevos sobre las sesiones reales como parte de la prueba.
- [ ] Crear tablas `events`, `event_receipts` y `deliveries`. Usar secuencia local monotónica para paginación. La unicidad usa sourceEventId cuando existe, o una clave de ciclo confirmada; no deduplicar dos turnos distintos por proximidad temporal o texto igual.
- [ ] Cuando no exista identidad fuerte, conservar la recepción con correlación local o desconocida. Evitar presentar dos recepciones ambiguas como una única finalización verificada. Actualizar estado solo si el evento corresponde al proceso/conversación vigentes y no retrocede a un turno anterior.
- [ ] Probar este contrato antes de conectar la UI:

```python
def test_completion_cannot_finish_a_newer_turn():
    current = {'turnId': 'new', 'state': 'working', 'processKey': 'p'}
    old = {'kind': 'turn_completed', 'turnId': 'old', 'processKey': 'p',
           'evidence': 'confirmed', 'correlation': 'source'}
    assert reduce_turn(current, old) == current
```

- [ ] Migrar eventos históricos conservándolos como eventos sin destino exacto cuando solo tengan project/status/detail/ts. Su apertura muestra detalle histórico; no adivina un pane actual por nombre de proyecto.
- [ ] Probar duplicados, llegada fuera de orden, dos turnos rápidos, panes del mismo proyecto, cancelación, permiso, reinicio, muerte de proceso y `%N` reutilizado. E1 consume los eventos sin inventar Resuelto.
- [ ] Ejecutar pytest sobre las tres suites anteriores. Guardar commit `feat: retain event identity and reconcile turn lifecycle`.

## N2. Franja, floats, lectura y sonido por dispositivo

**Crear:**

- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/dash/notifications.js
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/dash/ui-sounds.js
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/lib/notification_delivery.py
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/tests/test_notification_delivery.py
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/tests/notification_ui_checks.cjs

**Modificar:**

- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/dash/index.html
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/dash/workspace.css
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/bin/cc-notifyd
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/bin/cc-app

**Consume:** eventos N1 y presencia de cliente `{deviceId, lastInteractionAt, visible, connected, canPlayAudio}`. **Produce:** `route_event(event, clients, policy)` y un recibo de reproducción con unicidad `(eventId, channel)`.

- [ ] Extraer franja inferior y aviso flotante del mockup aprobado. Agrupar actividad por proyecto; no adjudicar noticias generales a un proyecto inventado. La llegada conserva foco, borrador y selección de texto.
- [ ] Implementar abrir origen por paneKey y conversación. Si desapareció, mostrar el evento y la indisponibilidad. No crear una terminal sustituta. Leer, cerrar float y resolver una solicitud son operaciones diferentes.
- [ ] Mantener respuesta a permisos en la terminal existente. Esta fase no introduce botones genéricos de autorizar desde el aviso. Si se propone esa acción después, necesita requestId vigente y contrato específico del harness.
- [ ] Registrar interacción humana explícita y presencia visible. El último cliente utilizado reclama audio mediante recibo único; no usar la última respuesta del polling como interacción. No reproducir dos cues por el mismo evento entre popup nativo, dashboard y teléfono.
- [ ] Ofrecer Visual y Visual + sonido por tipo, volumen, mute y preview de videojuego. Crear `uiSounds.play(cue, {eventId, volume})`, `uiSounds.preview(cue)` y `uiSounds.stop()` a partir del adaptador de /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-grill/dash/prototypes/prototype-ui-sounds.js. P2 añade sus cues a ese mismo adaptador. No superponer audio ni crear bucles de aviso; los loops de iconos no activan sonidos repetidos.
- [ ] Resolver D5 con ejemplos de turno terminado, permiso, error, ráfaga y Pomodoro. Registrar duración del float, agrupación, modos iniciales y ausencia. Antes de ese veredicto, usar política de ensayo solo en la demo; no aplicar nuevos defaults a preferencias reales.
- [ ] Unificar los productores: hooks, noticias, cuotas y Pomodoro pasan por la misma clasificación/entrega. Un anuncio de modelo deja de viajar como waiting. El daemon no mantiene una política paralela de silencio independiente.
- [ ] Probar reclamo simultáneo de sonido, cliente oculto, reconexión, permiso de audio denegado, float descartado e historial aún disponible. Ejecutar pytest y Node de los archivos nuevos y revisar la matriz responsive en Chrome remoto.
- [ ] Guardar commit `feat: deliver project notices through a shared notification policy`.

## N3. Retirar Telegram de CommandOS

**Modificar:**

- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/install.sh
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/hooks/cc-notify.sh
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/hooks/cc-notify.conf.example
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/bin/cc-dash
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/dash/index.html
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/README.md

**Retirar de la distribución:**

- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/bin/cc-telegram
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/systemd/cc-telegram.service
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/hooks/telegram.env.example

**Crear:** /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/tests/test_telegram_retirement.py.

**Consume:** instalación existente. **Produce:** instalación que no registra ni arranca el bot, y migración idempotente limitada a la unidad de CommandOS.

- [ ] Quitar controles, envíos de eventos/cuotas, handlers, copia de plantilla de credenciales y autoarranque del bot. Revisar llamadas por símbolo antes de borrar módulos; no retirar el operador/chat, que se conserva.
- [ ] Preparar migración de instalación que detiene/deshabilita únicamente `cc-telegram.service` de este producto y elimina sus enlaces instalados. No tocar conversaciones personales, servicios ajenos ni borrar secretos/historial guardados.
- [ ] Adaptar pruebas de Telegram a la ausencia del canal y a la migración. Ejecutar contra HOME temporal y systemctl falso; comprobar que una unidad ajena no aparece en los comandos emitidos.
- [ ] Activar esa migración solo durante la instalación del candidato aprobado en R3. No parar el servicio real mientras se escribe o prueba este bloque.
- [ ] Guardar commit `refactor: retire the CommandOS Telegram integration`.

## N4. Push Android con contenido breve

**Crear:**

- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/lib/web_push.py
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/requirements-push.txt
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/tests/test_web_push.py

**Modificar:**

- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/dash/sw.js
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/dash/manifest.webmanifest
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/dash/index.html
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/bin/cc-dash

**Consume:** política N2, eventos N1 y usuario autenticado. **Produce:** endpoints propuestos `/push/key`, `/push/subscription`, `/push/test` y `send_due_push(now_ms)`. Suscripción y test son acciones explícitas del usuario.

- [ ] Verificar requisitos contra la investigación existente en /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-grill/design/mobile-push-research.md. Revalidar fuentes oficiales al fijar una librería. Usar una implementación Web Push mantenida, fijar versión/hash y probar su transporte con un doble; no escribir cifrado propio.
- [ ] Generar claves VAPID en instalación con permisos privados. Publicar solo la clave pública. Validar suscripción, longitudes y endpoint HTTPS de un servicio push permitido; rechazar destinos privados o con credenciales para que la API no sea un proxy arbitrario.
- [ ] Registrar service worker y permiso tras un gesto. No exigir instalación de PWA como requisito universal de Android. Manejar denegación y revocación sin loops de petición.
- [ ] Persistir entregas por evento/dispositivo. 404/410 retiran la suscripción caducada; 429/5xx siguen Retry-After/backoff y TTL. Timeout incierto no crea un evento nuevo. El historial muestra el fallo de entrega sin fingir recepción.
- [ ] La notificación debe poder mostrarse con el payload recibido sin consultar primero el host de Tailscale, que podría estar inaccesible al teléfono. Su toque abre el origen autenticado con `?event=<id>`; no incluir tokens en URL ni elegir pane por título. Acceder al contenido privado sigue requiriendo conexión/autenticación.
- [ ] Añadir handlers `push` y `notificationclick` conservando el fetch/offline existente. Usar tag estable por evento para reintentos, cuerpo breve y preview acorde con D6. El sistema Android decide apariencia y sonido; no prometer audio de videojuego personalizado fuera de la app.
- [ ] Probar cifrado/transporte con fixtures, permisos denegados, suscripción caducada, doble envío, app abierta, segundo plano y click sin pane vivo. Resolver D5/D6 antes de activar envío automático.
- [ ] Con Jesús, pulsar el botón de prueba y verificar el Android real: app visible, otra app activa, pantalla bloqueada y apertura del evento. Registrar navegador/versión, permiso, estado de Tailscale y resultado. Ningún emulador sustituye esta aceptación.
- [ ] Guardar commit `feat: add authenticated Android Web Push delivery` con límites de entrega documentados.

## N5. Tres ediciones con fuentes y lector aprobado

**Crear:**

- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/lib/news_editions.py
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/dash/news-reader.js
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/tests/test_news_editions.py
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/tests/news_reader_checks.cjs

**Modificar:**

- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/lib/news_watch.py
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/bin/cc-dash
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/dash/index.html
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/dash/workspace.css

**Consume:** catálogo/noticias existentes, fuentes recuperadas y política `{timezone, slots[3], budget, sourceScope}` elegida en D4. **Produce:** `schedule_editions(day, policy)`, `build_edition(job, fetch, summarize)` y endpoints propuestos `/news/editions`, `/news/edition?id=...`. La clave única es fecha local más slot; el worker admite como máximo una generación simultánea de rutina.

- [ ] Agregar a cada fuente `publishedAt` opcional, `discoveredAt`, `verifiedAt`, URL y resultado de captura. Un fetch fallido no equivale a fuente sin noticias. Conservar la edición previa durante errores y marcar una edición parcial cuando corresponda.
- [ ] Normalizar URLs y agrupar cobertura del mismo anuncio conservando fuentes. No fusionar actualizaciones distintas solo por similitud de título. Resumen y afirmaciones deben citar la fuente leída; precio, disponibilidad y capacidades no se deducen del nombre de modelo.
- [ ] Construir un trabajo persistente con límite de fuentes, tokens/costo y tiempo. Inyectar `fetch`/`summarize` en pruebas para no gastar ni abrir agentes. En producción fijar presupuesto y proveedor antes de activar el scheduler.
- [ ] Reutilizar criterios de investigación de /home/someguy/.claude/skills/synced/f2f85f47-6328-47ad-affe-4d661dbc2d5f_591de498-3462-4235-8db5-843cc3293a43/deep-research/SKILL.md. Esa skill es un flujo, no un servicio instalado. Reservar investigación ampliada para una petición explícita con presupuesto; no lanzar su fan-out por abrir noticias ni automáticamente tres veces al día.
- [ ] Incluir IA, modelos, MCPs, skills, bounties y hackathons. Para oportunidades: fuente, recompensa, fecha/zona horaria, elegibilidad y entrega, con campos desconocidos explícitos. No presentar descubrimiento como publicación ni participantes como probabilidad de ganar.
- [ ] Implementar Markdown con parser mantenido y saneado, HTML crudo desactivado, enlaces limitados a http/https y código tratado como texto. Fijar versión/hash de dependencias al incorporarlas. Probar scripts, javascript URLs, tablas anchas y enlaces engañosos; el contenido externo es dato, nunca instrucción de herramientas.
- [ ] Extraer edición continua B del mockup, terminal opcional a la izquierda y lector a la derecha. A 560 px o menos conservar ese orden con vistas de ancho completo y Terminal/Novedades o swipe. Mostrar/ocultar conserva lectura y borrador; no reinicia la terminal.
- [ ] Implementar guardado, filtros y tamaño de texto del lector. Guardar no crea recordatorio. Seguir temas no activa un canal adicional sin política elegida. Abrir, filtrar o releer una edición no llama al modelo ni al buscador.
- [ ] Resolver D4: tres horas concretas en America/Mexico_City, presupuesto, fuentes y tratamiento de slots perdidos durante una caída. Crear tres claves por día, registrar ausencia/fallo con honestidad y deduplicar jobs al reiniciar. No inventar 08:00/14:00/20:00 como configuración aceptada.
- [ ] Probar caché, fuente caída, generación fallida, gasto límite, duplicados, medianoche, reinicio, bounties vencidas y conservación de ancla/draft. Ejecutar las suites nuevas y revisión visual remota. Guardar commit `feat: publish sourced AI editions in the approved reader`.

N5 es un requisito de implementación completo, con activación condicionada a D4. No dar por satisfechos los tres resúmenes diarios mostrando solo las noticias ficticias del mockup.
