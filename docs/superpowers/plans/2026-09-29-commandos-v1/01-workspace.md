# Workspace y continuidad: plan de implementación

> **Para el agente implementador:** usar `executing-plans`. Ejecutar cada tarea con su prueba de contrato y guardar un commit. La preparación y las restricciones globales están en /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-grill/docs/superpowers/plans/2026-09-29-commandos-v1/README.md.

**Objetivo:** recuperar el último workspace y reorganizar tabs completas sin cambiar la identidad de sus terminales ni el foco de otros dispositivos.

**Arquitectura:** un documento versionado contiene la distribución; el estado de cada cliente contiene foco, borradores y lectura. Los snapshots de tmux conservan procesos reanudables. El renderizado móvil adapta el árbol sin escribir otra distribución.

**Tecnologías:** Python, SQLite, tmux, GTK/VTE, JavaScript y xterm.

## Contrato propuesto entre módulos

Las siguientes interfaces son nuevas, no endpoints existentes:

```json
{
  "schema": 1,
  "revision": 12,
  "ready": true,
  "groups": [{
    "id": "group-1",
    "tree": {
      "type": "split", "axis": "x", "ratio": 0.6,
      "first": {"type": "tab", "tabId": "tab-a"},
      "second": {"type": "tab", "tabId": "tab-b"}
    }
  }],
  "tabs": {
    "tab-a": {"session": "alpha", "paneKeys": ["logical-pane-a"]},
    "tab-b": {"session": "beta", "paneKeys": ["logical-pane-b"]}
  }
}
```

`paneKey` es una identidad lógica persistente. Su enlace al proceso vivo contiene servidor/generación, sesión tmux, pane, PID/inicio y conversación. Un `%N` reutilizado no hereda el historial, estado o borrador anterior.

- `GET /workspace` devuelve el documento confirmado.
- `POST /workspace` recibe `{requestId, expectedRevision, document}`. Valida, excluye escrituras parciales y devuelve una revisión nueva; 409 entrega la revisión actual sin sobrescribirla.
- `GET/POST /workspace/client` guarda `{deviceId, activeTabId, activePaneKey, drafts, readingAnchors}`. Autenticar al cliente; este estado no altera la distribución.
- `WorkspaceStore(db).commit(expected_revision, document, request_id)` devuelve el resultado persistido de la misma petición si se repite.
- `restore_workspace(document, inspect, resume_exact)` devuelve `{attached, resumed, unavailable}` por identidad; jamás sustituye una conversación por la última del proveedor.

## W1. Persistir y restaurar un documento completo

**Archivos a crear en el checkout de implementación:**

- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/lib/workspace_state.py
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/lib/app_state.py
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/tests/test_workspace_state.py

**Archivos a modificar:**

- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/lib/tmux_snapshot.py
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/bin/cc-app
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/bin/cc-dash
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/tests/test_tmux_snapshot.py
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/tests/test_app_split_snapshot.py

**Consume:** `capture_session`, `carry_resume_ids`, `restore_session`, `read_snapshot`, `write_snapshot` y los registros de tabs existentes. **Produce:** `WorkspaceStore`, `restore_workspace`, `app_state.connect(path)` y `app_state.migrate(connection)`. El índice define la base compartida y el control manual de transacciones.

- [ ] Añadir pruebas para reapertura, revisión concurrente, escritura truncada, inicio vacío, documento explícitamente vacío tras cerrar y pane reutilizado. Usar una base temporal y la fixture de servidor tmux privado de la suite de snapshots.
- [ ] Implementar validación del árbol antes de persistir. Este recorrido establece la unicidad obligatoria; añadir validación estricta de tipos, límites numéricos y profundidad máxima de 64 nodos:

```python
def tab_ids(node):
    if node['type'] == 'tab':
        return [node['tabId']]
    if node['type'] != 'split' or node['axis'] not in ('x', 'y'):
        raise ValueError('Distribución inválida')
    if not 0 < node['ratio'] < 1:
        raise ValueError('Proporción inválida')
    return tab_ids(node['first']) + tab_ids(node['second'])

def validate_membership(document):
    ids = [tab for group in document['groups'] for tab in tab_ids(group['tree'])]
    if len(ids) != len(set(ids)) or set(ids) != set(document['tabs']):
        raise ValueError('Cada tab debe aparecer exactamente una vez')
```

- [ ] Crear la base mediante `app_state`, habilitar claves foráneas y un busy timeout acotado, y guardar `workspace_current`, `workspace_previous`, `workspace_requests` y `workspace_clients`. La transacción usa `BEGIN IMMEDIATE`, comprueba requestId y revisión, valida el documento, copia current a previous y escribe la siguiente revisión antes de confirmar. Un mismo requestId con contenido diferente devuelve 409. Las pruebas de migración y rollback usan la base temporal de W1.
- [ ] Distinguir `restoring`, `ready` y `failed` en el arranque. Solo `ready` admite guardado automático. Un cierre humano sí puede persistir un documento vacío; un inventario temporalmente vacío no reemplaza el último estado.
- [ ] Migrar las tabs/snapshots existentes una vez, conservando archivos originales. Adoptar sesiones vivas por identidad antes de intentar `resume`. Con datos incompletos mantener un lugar marcado como no disponible y el snapshot intacto; no crear otra conversación ni abrir un selector histórico.
- [ ] Ejecutar las suites siguientes. Esperado: todas pasan y no se llama al socket tmux del usuario.

```sh
/tmp/comandos-v1-qa/bin/python -m pytest -q /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/tests/test_workspace_state.py /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/tests/test_tmux_snapshot.py /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/tests/test_app_split_snapshot.py
```

- [ ] Guardar commit `feat: persist and restore complete workspace revisions`.

## W2. Mover tabs completas y renderizar el árbol

**Crear:**

- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/dash/workspace-layout.js
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/lib/workspace_layout.py
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/tests/test_workspace_layout.py
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/tests/workspace_layout_checks.cjs

**Modificar:**

- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/bin/cc-app
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/dash/index.html
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/dash/workspace.css

**Consume:** documento W1. **Produce:** operaciones puras `move_tab(document, tab_id, target_id, edge)`, `detach_tab(document, tab_id, index)` y `resize_split(document, node_path, ratio)`. JavaScript reproduce el mismo contrato usando fixtures JSON comunes. `edge` admite left/right/top/bottom internamente; esas palabras no se convierten en botones del producto.

- [ ] Crear fixtures de mover dentro del grupo, mover entre grupos, extraer la última hoja de un grupo, anidar en borde interior/exterior y cancelar. Cada operación conserva exactamente el conjunto original de tabIds.
- [ ] Implementar movimiento sobre una copia: extraer la hoja por tabId, colapsar el padre con un solo hijo, resolver el destino por identidad en la copia y envolverlo con un split. Rechazar destino igual al origen. Desacoplar agrega una raíz nueva en el índice solicitado. Cancelar devuelve el documento original sin POST.
- [ ] Usar el mockup aprobado `round=dock&variant=A` como referencia de gestos. El renderer GTK usa contenedores divisibles y conserva los widgets de terminal; el web reutiliza sus nodos/iframes por tabId. No lanzar un segundo cliente de terminal por cada render ni copiar texto para simular un terminal vivo.
- [ ] Enlazar pointer capture, umbral de arrastre y pulsación prolongada táctil a una sola intención de movimiento. Mostrar preview antes de soltar, Escape cancela y el strip permite desacoplar/reordenar. El separador acepta arrastre y flechas.
- [ ] En anchura pequeña apilar en el orden del árbol sin escribir el árbol adaptado al servidor. El indicador de grupo usa número seguido de `tabs`. Quitar el botón Mosaico visible; conservar el mecanismo interno y su atajo hasta decidir su retirada por separado.
- [ ] Ejecutar fixtures Python/Node y comprobar en Chrome remoto 1440×1000, 820×1180, 390×844, 320×568 y 667×375. Añadir teclado abierto y cambio de orientación. Verificar que mover no cambia PID, conversación, borrador ni cantidad de conexiones.

```sh
/tmp/comandos-v1-qa/bin/python -m pytest -q /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/tests/test_workspace_layout.py
node /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/tests/workspace_layout_checks.cjs
```

- [ ] Guardar commit `feat: dock whole tabs with nested drop previews`. Pedir a Jesús que agrupe, desacople y redimensione desde su Android; conservar cualquier defecto como pendiente del bloque.

## W3. Foco independiente y cierres con destino fijo

**Modificar:**

- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/bin/cc-webterm-attach
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/bin/cc-app
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/bin/cc-dash
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/lib/terminal_panes.py
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/tests/test_terminal_panes.py

**Crear:**

- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/tests/test_workspace_clients.py
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/tests/test_workspace_close.py

**Consume:** W1/W2 e identidad real de proceso. **Produce:** foco local por deviceId y `close_group(group_id, expected_revision, identities, request_id)` con resultados por miembro.

- [ ] Probar dos clientes PTY de ensayo sobre el mismo servidor tmux privado. La documentación instalada de tmux 3.2a contiene `active-pane`, pero el attach actual no lo usa. Verificar el efecto real antes de adaptar ambos clientes.
- [ ] Activar foco independiente con la bandera documentada en cada attach compatible:

```sh
tmux -L comandos-v1-client-test-UNIQUE attach-session -f active-pane -t '=fixture-session'
```

Este comando ilustra el attach sobre la sesión creada por la prueba. El harness genera un UUID para el sufijo UNIQUE y lo usa en todos sus comandos. La bandera no convierte hooks/estilos globales en locales: la selección visual del cliente debe leer su propia identidad. Los endpoints de input ya deben dirigirse al pane explícito; no llamar al foco global para luego escribir.

- [ ] Separar viewport local de proporción compartida. Probar que una conexión móvil no impone continuamente su tamaño a escritorio; aplicar la política de tamaño del cliente tmux solo después de medir ambos terminales. Una implementación sin prueba de input aislado no satisface este requisito.
- [ ] Unificar cierre nativo/remoto usando snapshot previo e identidad comprobada. La acción de cerrar un split termina ese proceso. El último pane usa el flujo de cierre de tab; conservar la protección de la tab LOCAL existente.
- [ ] Para un grupo: preparar lista fija de tabs/panes, pedir confirmación, guardar todos los snapshots, revalidar y terminar únicamente esas identidades. Si una cambió antes de actuar, cancelar. Si un fallo ocurre tras un cierre, registrar los miembros efectivamente cerrados y los restantes; no simular atomicidad ni terminar otros como compensación.
- [ ] Probar cancelación, reutilización de `%N`, cambio de foco durante confirmación, snapshot fallido, fallo intermedio y reapertura posterior sin resucitar cierres intencionales.

```sh
/tmp/comandos-v1-qa/bin/python -m pytest -q /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/tests/test_workspace_clients.py /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/tests/test_workspace_close.py /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/tests/test_terminal_panes.py
```

- [ ] Guardar commit `fix: isolate device focus and guard workspace close operations`.

## W4. Recuperar lectura y texto sin reenviar comandos

**Modificar:**

- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/lib/workspace_state.py
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/dash/term.html
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/dash/workspace.js
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/bin/cc-app

**Crear:** /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/tests/test_workspace_reopen.py.

**Consume:** estado local W1 y buffers del compositor existente. **Produce:** restauración por identidad de borrador y ancla de lectura, sin input PTY durante la restauración.

- [ ] Guardar borradores y selección del compositor por deviceId/paneKey/conversationId, con escritura acotada y confirmación antes de cerrar la vista. No guardar solo al salir: el proceso puede terminar abruptamente.
- [ ] Recuperar el ancla de scroll contra el historial disponible; si el contenido ya no existe, mostrar ese límite sin saltar a otra conversación. Preservar las secuencias ANSI en el terminal, sin perder resaltado al reconstruir vistas.
- [ ] Diferenciar texto aún en el compositor de bytes ya entregados al TUI. No reenviar un comando parcial para aparentar restauración. Si un harness no permite recuperar su editor interno tras morir, registrar esa limitación y conservar el texto recuperable en el compositor, sin Enter automático.
- [ ] Probar cerrar/reabrir app, refrescar remoto, reconectar después de desconexión, caída durante guardado, arranque fallido y cambio de orientación. Verificar títulos, orden, proporciones, panes internos, conversación, configuración, scroll y texto pendiente.
- [ ] Ejecutar la prueba siguiente y repetir el flujo con Jesús en un candidato aislado. Esperado: no se envían bytes al PTY al restaurar un borrador. Guardar commit `feat: restore per-device reading and unsent input`.

```sh
/tmp/comandos-v1-qa/bin/python -m pytest -q /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation/tests/test_workspace_reopen.py
```

Este bloque no promete revivir memoria de un proceso arbitrario tras un apagón. Sí exige reutilizar lo vivo, reanudar la conversación exacta cuando sea posible y explicar lo que falta. No declarar restauración completa si alguna de esas comprobaciones queda sin evidencia.
