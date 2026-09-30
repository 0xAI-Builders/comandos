# Pomodoro: plan de implementación

> **Para el agente implementador:** usar `executing-plans`. Restricciones y decisiones D2/D3 en /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-grill/docs/superpowers/plans/2026-09-29-commandos-v1/README.md.

**Objetivo:** tener un temporizador compartido que funcione con la página cerrada, controles de regla, arte animado y estadísticas fiables.

**Arquitectura:** el servidor posee el bloque y su revisión; los clientes calculan solo la representación del tiempo restante. Finalización, registro y evento N1 se confirman en una misma transacción de la base de estado de la app.

**Tecnologías:** Python/SQLite, JavaScript, reloj inyectado, sprites vendorizados y adaptador UISFX ya ensayado.

## Contrato propuesto

`PomodoroStore(db, clock).command(request)` recibe `requestId`, `expectedRevision` y `action`: start, pause, resume, extend o cancel. `start` añade `mode`, `targetMs` y atribución de proyecto/sessionKey/paneKey. `extend` añade `deltaMs`. Devuelve el bloque confirmado y su revisión. GET no modifica el temporizador.

```json
{
  "blockId": "block-a", "revision": 3, "mode": "focus",
  "status": "running", "targetMs": 1500000,
  "activeMs": 120000, "resumedAtMs": 1000000,
  "deadlineMs": 2380000, "project": "ComandOS",
  "sessionKey": "session-a", "paneKey": "pane-a"
}
```

La atribución se fija al iniciar. Cambiar de pane no cambia el proyecto al que pertenece el bloque. El servidor devuelve `serverNowMs` para corregir diferencia de reloj visual. El uso de un reloj de prueba nunca altera la hora del sistema.

## P1. Una autoridad para inicio, pausa, extensión y finalización

**Crear:**

- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/lib/pomodoro.py
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/dash/pomodoro.js
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/tests/test_pomodoro.py
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/tests/pomodoro_sync_checks.cjs

**Modificar:**

- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/bin/cc-dash
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/dash/index.html
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/bin/cc-notifyd
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/lib/app_state.py

**Consume:** conexión de estado W1 y `append_event(connection, event)` de N1. **Produce:** comandos anteriores, `snapshot()` y `settle_due(now_ms)`; una terminación por blockId.

- [ ] Reproducir en código base el diagnóstico ya preparado. Se espera salida 1 por divergencia de extensión y cancelación; no ejecuta llamadas reales:

```sh
node /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-grill/dash/prototypes/prototype-pomodoro-sync-repro.cjs /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/dash/index.html
```

- [ ] Llevar sus aserciones al módulo nuevo, sin dejar una extracción dependiente del HTML antiguo como único test. Usar dos clientes, reloj fijo y transporte falso. La extensión confirmada y la cancelación deben aparecer en ambos.
- [ ] Implementar cómputo de tiempo activo independiente del número de ticks:

```python
def elapsed_ms(block, now_ms):
    running = block['status'] == 'running'
    delta = max(0, now_ms - block['resumedAtMs']) if running else 0
    return min(block['targetMs'], block['activeMs'] + delta)

def remaining_ms(block, now_ms):
    return max(0, block['targetMs'] - elapsed_ms(block, now_ms))

def test_paused_time_does_not_count():
    block = {'status': 'paused', 'targetMs': 1500000,
             'activeMs': 120000, 'resumedAtMs': None}
    assert elapsed_ms(block, 9000000) == 120000
    assert remaining_ms(block, 9000000) == 1380000
```

- [ ] En cada comando, liquidar primero un vencimiento ya ocurrido. Pausar fija activeMs y borra deadline/resumedAt; reanudar calcula deadline desde lo restante. Extender cambia targetMs y deadline en servidor. Cancelar conserva tiempo activo con status cancelled. Validar duración con los límites existentes del producto; no introducir objetivos diarios nuevos.
- [ ] Guardar requestId/revisión para evitar dobles inicios por reintento. Iniciar un segundo bloque mientras otro está vivo devuelve conflicto explícito; no terminar el anterior de forma silenciosa.
- [ ] Ejecutar un único scheduler del backend que liquida vencimientos al arrancar y durante su vida. Dentro de una transacción: actualizar estado final, insertar registro con blockId único y `append_event`. Los clientes no envían finish porque su intervalo llegó a cero.
- [ ] Reemplazar el estado autoritativo local del HTML y las llamadas a loopback desde el navegador por GET/POST al backend autenticado de la app. Reconciliar también pausa, cancelación y ausencia de bloque. Errores dejan visible el último estado confirmado, con reintento de la misma operación.
- [ ] Mantener lectura/migración de datos antiguos del archivo de foco; no correr dos autoridades simultáneas. La interacción del Pomodoro con supresión de avisos pasa a N2/D5, no a un filtro independiente oculto en el daemon.
- [ ] Ejecutar suites nuevas y pruebas de caída antes/después de commit, dos solicitudes simultáneas, pausa larga, suspensión de página y reinicio del backend. Esperado: un registro y un evento por bloque terminado.

```sh
/tmp/comandos-v1-qa/bin/python -m pytest -q /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/tests/test_pomodoro.py
node /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/tests/pomodoro_sync_checks.cjs
```

- [ ] Guardar commit `fix: make pomodoro state authoritative across clients`.

## P2. Regla de tiempo, estilos y audio opcional

**Modificar:**

- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/dash/pomodoro.js
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/dash/workspace.css
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/bin/cc-app
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/dash/ui-sounds.js

**Crear:**

- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/tests/pomodoro_ui_checks.cjs

**Consume:** P1 y permiso de reproducción asignado por N2. **Produce:** misma operación de regla/pause/resume en escritorio y remoto, sin temporizadores paralelos.

- [ ] Extraer la vista B, Regla de tiempo, del mockup. Conservar teclado, valor editable y rango táctil; arrastrar duración no inicia el temporizador. En ejecución, mostrar la duración resultante de un cambio confirmado.
- [ ] Copiar a un directorio de assets de producción los originales necesarios desde /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-grill/dash/prototypes/assets/pixel-packs. Conservar atribuciones/licencias de /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-grill/dash/prototypes/assets/pixel-packs/CREDITS.md y verificar hashes contra /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-grill/dash/prototypes/assets/pixel-packs/manifest.json. No copiar todo el laboratorio.
- [ ] Mantener Alquimia, Arcade, Fantasía, RPG clásico, Jardín y Cristales. Alquimia es el valor inicial aprobado. El reloj común usa los 15 frames originales de Zoedoz; no volver al reloj estático con movimiento de contenedor. Mantener los sprites de cofre, fuego, pociones y cristales elegidos.
- [ ] Conservar loops visibles, reduced-motion y pausa cuando la vista está oculta. El reloj decorativo no representa arena restante real; el número del temporizador manda. Cambiar estilo no altera bloque, tiempo, XP ni historial.
- [ ] Reutilizar el contrato de audio de /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-grill/dash/prototypes/prototype-ui-sounds.js y /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-grill/dash/prototypes/prototype-pomodoro-audio.js. Conservar licencia MIT de UISFX vendorizado. Cues breves, volumen, mute y preview por gesto humano; ningún loop de sonido ni tick continuo.
- [ ] Resolver D3 con Jesús en la demostración: alcance del estilo y automatismo de ciclos. Hasta entonces, conservar ajustes existentes y no escribir una preferencia global nueva por asumirla aprobada.
- [ ] Comprobar ruler, foco, arrastre, teclado, estilo durante bloque activo y finalización en la matriz responsive de W2. Escuchar los sonidos con Jesús en ambos dispositivos. La prueba automática intercepta audio; no equivale a una prueba audible.
- [ ] Guardar commit `feat: add the approved pomodoro ruler and animated styles`.

## P3. Analytics con tiempo medido

**Modificar:**

- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/bin/cc_usage.py
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/lib/pomodoro.py
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/dash/pomodoro.js
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/tests/test_usage_core.py

**Crear:** /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/tests/test_pomodoro_analytics.py.

**Consume:** registros P1 por blockId. **Produce:** `focus_report(from_ms, to_ms, project=None)` con completados, cancelados, tiempo activo y tiempo planeado separados.

- [ ] Guardar mode, targetMs, activeMs, startedAt, endedAt, status y atribución fija. Pausas no suman foco; descansos no se suman como trabajo.
- [ ] Migrar o mapear historial antiguo conservando su procedencia. Los minutos históricos calculados desde duración planeada se etiquetan como tales; no inventar pausas ni tiempo efectivo.
- [ ] Añadir vista acotada de Pomodoro dentro de Analytics, con filtro de fechas/proyecto e historial. No rediseñar Analytics general/Reparto en esta tarea.
- [ ] Probar pausa, extensión, cancelación, medianoche local y cambios de filtro. El total debe obtenerse de los registros y no del estado visual del reloj.
- [ ] Ejecutar pytest sobre la suite nueva y los casos focus de /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/tests/test_usage_core.py. Guardar commit `feat: report measured pomodoro activity`.

## P4. Gamificación parametrizada y revisión humana

**Crear:**

- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/lib/focus_progress.py
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/tests/test_focus_progress.py

**Modificar:** /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/dash/pomodoro.js.

**Consume:** bloques P1/P3 y una política explícita `{policyVersion, xpPerMinute, xpPerLevel, dailyGoalMinutes, achievements}`. **Produce:** `progress(blocks, policy)` determinista; no cambia por filtrar Analytics.

- [ ] Implementar cálculo puro y ledger de recompensas por `(blockId, policyVersion)`. Usar una política de prueba para verificar que reintentar finalización no duplica puntos, descansos no generan foco y filtros no recalculan la carrera personal.
- [ ] Mostrar a Jesús una finalización y el efecto de la política de ensayo. Pedir D2 con cifras y ejemplos concretos; los valores usados en el mockup siguen siendo propuestas.
- [ ] Después del veredicto, guardar la política aprobada con versión y definir explícitamente si se calcula sobre historial previo. No asignar puntos reales retroactivos antes de esa decisión.
- [ ] Verificar persistencia, rachas con la zona horaria elegida, reglas de cancelación y un único aviso de nivel. Guardar commit `feat: add versioned personal focus progression` solo con la activación correctamente delimitada.

El bloque de gamificación es requisito del usuario. Puede prepararse técnicamente antes de D2, pero no declararse finalizado con la política aún sin elegir.
