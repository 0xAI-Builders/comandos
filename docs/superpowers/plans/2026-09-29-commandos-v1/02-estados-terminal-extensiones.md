# Estados, terminal rápida y extensiones: plan de implementación

> **Para el agente implementador:** usar `executing-plans`. Aplican la preparación y restricciones de /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-grill/docs/superpowers/plans/2026-09-29-commandos-v1/README.md.

**Objetivo:** operar estados y terminales con acciones breves, corregir selección global de extensiones y conservar el chat existente.

**Arquitectura:** persistir metadatos por identidad lógica, consumir eventos confirmados de N1 y compartir operaciones entre GTK y web. Los controles de extensiones permanecen ligados al pane.

**Tecnologías:** Python, SQLite, tmux, JavaScript, CSS/SVG y pruebas con reloj/directorios aislados.

## E1. Estados propios e iconos animados

**Crear:**

- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/lib/work_marks.py
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/dash/work-marks.js
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/tests/test_work_marks.py

**Modificar:**

- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/bin/cc-app
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/bin/cc-dash
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/dash/index.html
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/dash/workspace.css

**Consume:** paneKey y sessionKey de W1; `TurnEvent` de N1. **Produce:** `set_mark(scope, key, value, expected_revision)` y `apply_turn_event(event)` con efecto idempotente. Los valores son `none`, `resolved`, `frozen`, `awaiting_reply`. Favorito es un booleano independiente.

- [ ] Escribir estas pruebas con un store temporal: marcar sesión no cambia panes; marcar pane no cambia sesión/hermanos; pane nuevo empieza sin marca; favorito no cambia marca; evento repetido no duplica una transición.
- [ ] Implementar la transición mínima aprobada con evidencia de prompt aceptado, no con foco o escritura de borrador:

```python
def mark_after_event(mark, event):
    if event['kind'] == 'prompt_accepted' and event['evidence'] == 'confirmed':
        return 'none' if mark == 'resolved' else mark
    return mark

def test_accepted_prompt_only_reopens_resolved():
    event = {'kind': 'prompt_accepted', 'evidence': 'confirmed'}
    assert mark_after_event('resolved', event) == 'none'
    assert mark_after_event('frozen', event) == 'frozen'
    assert mark_after_event('awaiting_reply', event) == 'awaiting_reply'
    assert mark_after_event('resolved', {**event, 'evidence': 'inferred'}) == 'resolved'
```

El trabajo activo se representa mediante el evento de actividad y se muestra como Trabajando. Conservar Congelado/Esperando respuesta mientras D1 siga abierta; no equipararlos con SIGSTOP ni con un permiso del harness.

- [ ] Añadir menú táctil/teclado por ámbito, con iconos SVG, nombre accesible y tooltip. Animar continuamente estados visibles y estrella activa; mantener forma identificable y cajas fijas. Pausar vistas ocultas y respetar reduced-motion.
- [ ] No marcar Resuelto al terminar un turno. Mostrar la evidencia de finalización sin asignar una marca organizativa. Presentar a Jesús el icono neutral y la transición de bloqueos, D1, antes de declarar cerrada esa presentación.
- [ ] Ejecutar pytest sobre /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/tests/test_work_marks.py. Probar con dos panes del mismo proyecto y dos sesiones, incluyendo recarga y movimiento.
- [ ] Guardar commit `feat: persist independent work marks and animated indicators`.

## E2. Terminal rápida con carpeta propia

**Crear:**

- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/lib/quick_terminal.py
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/tests/test_quick_terminal.py

**Modificar:**

- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/bin/cc-dash
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/bin/cc-app
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/dash/index.html
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/tests/test_new_session_focus.py

**Consume:** `requestId` y creador de shell existente, sin arrancar IA. **Produce:** nuevo `POST /terminal/quick` que devuelve `{tabId, paneKey, cwd}`. Repetir la misma petición devuelve la misma terminal; otra petición crea otra carpeta.

- [ ] Probar tiempo fijo, dos peticiones en el mismo segundo, concurrencia real de reserva, permiso denegado, fallo de lanzamiento y reintento del mismo requestId.
- [ ] Usar esta reserva atómica con `base` inyectable para las pruebas:

```python
from datetime import datetime
from pathlib import Path
from zoneinfo import ZoneInfo

def reserve_directory(base, now=None):
    base = Path(base)
    base.mkdir(parents=True, exist_ok=True)
    at = now or datetime.now(ZoneInfo('America/Mexico_City'))
    if at.tzinfo is None:
        raise ValueError('Se requiere fecha con zona horaria')
    stem = at.astimezone(ZoneInfo('America/Mexico_City')).strftime('T-%Y-%m-%d-%H-%M-%S')
    suffix = 1
    while True:
        candidate = base / (stem if suffix == 1 else f'{stem}-{suffix}')
        try:
            candidate.mkdir()
            return candidate
        except FileExistsError:
            suffix += 1
```

- [ ] Configurar en ejecución normal la base aprobada /home/someguy/codebase/0xJesus/Terminal. Guardar la carpeta reservada y el resultado por requestId antes de responder. Un fallo no borra carpetas con contenido; mostrar el error y permitir reintento sin abrir dos shells.
- [ ] Añadir botón Terminal y conservar Nueva sesión con su formulario actual. Ambos clientes llaman al mismo backend. No heredar cwd del pane activo, pues Jesús eligió explícitamente una carpeta nueva fechada.
- [ ] Ejecutar pytest sobre las dos suites anteriores con base de datos y directorios temporales. En revisión humana crear una terminal de ensayo, comprobar cwd y cerrar solo esa terminal.
- [ ] Guardar commit `feat: open quick shells in uniquely dated directories`.

## E3. Selección global de MCPs/skills y carga comprensible

**Modificar:**

- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/dash/extensions.js
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/dash/extensions.css
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/lib/pane_extensions.py
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/lib/extension_observations.py
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/tests/test_extension_batches.py
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/tests/test_extension_loading.py
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/tests/test_pane_extensions_api.py

**Consume:** inventario, `desired`, `loaded`, `usage`, revisión y operación ya existentes. **Produce:** `Shelf.batch(on, group=null, visibleOnly=false)` donde la opción global ignora búsqueda/filtro. Ningún batch aplica ni reinicia el agente.

Existe una corrección sin commit en el worktree /home/someguy/codebase/0xJesus/ComandOS/.worktrees/extension-selection-responsive. Sus archivos modificados son:

- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/extension-selection-responsive/dash/extensions.js
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/extension-selection-responsive/tests/test_extension_batches.py
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/extension-selection-responsive/tests/session_controls_fixture.cjs

- [ ] Leer ese diff y guardarlo como parche de referencia; no limpiar, resetear ni hacer checkout en ese worktree. Integrar los hunks relevantes en el checkout de implementación después de revisar cambios nuevos de main.
- [ ] Con búsqueda activa, verificar que Seleccionar todos incluye MCPs y skills editables fuera del filtro y que Quitar todos los apaga. Ofrecer acciones separadas para visibles y para categoría. Elementos gestionados externamente o desconocidos no se convierten en editables; contar y explicar las exclusiones.
- [ ] Mantener agrupación por procedencia real, no por clasificación inventada. Conservar la distinción entre tokens de referencia y consumo real; no afirmar tokens consumidos por skill sin telemetría.
- [ ] Mostrar loading de inmediato, conservar geometría del pane y cancelar resultados de peticiones obsoletas. Un inventario desconocido, falta de confirmación y un turno activo son condiciones diferentes; la UI solo habla de esperar al turno cuando existe evidencia de actividad.
- [ ] Reutilizar caché por identidad/revisión y agrupar el batch en una sola mutación. Medir inventario, serialización y render por separado antes de optimizar; no sustituir el fallo por un spinner permanente.
- [ ] Ejecutar las suites de batch, loading, API y observación. Verificar por Chrome remoto carga, error, búsqueda, quitar todo y agregar por grupo. Registrar tiempos de 20 lecturas acotadas en el entorno de prueba; comparar mediana y p95 sin presentar ese resultado como garantía de red móvil.

```sh
/tmp/comandos-v1-qa/bin/python -m pytest -q /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/tests/test_extension_batches.py /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/tests/test_extension_loading.py /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/tests/test_pane_extensions_api.py /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/tests/test_extension_observations.py
```

- [ ] Guardar commit `fix: make extension batches global and distinguish load state`.

## E4. Conservar chat y controles existentes durante la integración

**Modificar solo donde W/E cambien el contenedor:**

- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/dash/workspace.js
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/dash/index.html
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/bin/cc-app

**Pruebas existentes:**

- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/tests/test_operator_chat.py
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/tests/test_operator_stream.py
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/tests/test_session_config_ui.py
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/tests/test_sidebar_parity.py

**Consume:** chat/operador existente y destino de pane. **Produce:** compatibilidad, sin un segundo diseño de chat ni un nuevo backend.

- [ ] Mantener abrir/ocultar chat, borrador, historial, streaming y destino después de mover tabs o cambiar tamaño. La instrucción de conservarlo es posterior a su retirada; no borrar rutas `/operator`, datos ni módulos del chat.
- [ ] Mantener acceso MCPs/skills en el split. No duplicar su selector en la nueva barra ni esconderlo en móvil.
- [ ] Conservar el configurador de IA existente y las correcciones de catálogo. Probar regresión de modelos, cuentas y controles; el reemplazo de la política de cambio pertenece a la segunda fase.
- [ ] Ejecutar esas suites en el entorno aislado, con proveedores simulados. Comprobar abrir/ocultar y conservación del borrador en Chrome remoto. No enviar un prompt real para probar el chat.
- [ ] Si hubo cambios de integración, guardar commit `fix: preserve chat and pane tools during workspace changes`; si no hubo cambios, registrar solo el resultado de las pruebas.
