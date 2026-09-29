# CommandOS 1.0 — activación y reversión

Estado: **preparado, no ejecutado**. La activación requiere el recorrido R2 con Jesús y su visto bueno sobre el
registro `docs/verification/commandos-v1.md`. Nada de este documento se ha ejecutado sobre la instalación real.

## Versiones

- Versión previa en uso: checkout principal `/home/someguy/codebase/0xJesus/ComandOS`, rama `main`, commit `c0c68cf`.
- Candidato: rama `implementation/comandos-v1` (worktree
  `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-implementation`). Anotar aquí el commit final
  aprobado antes de activar.

## Qué cambia en almacenamiento y servicios

| Área | Ruta / unidad | Cambio | Reversión |
| --- | --- | --- | --- |
| Estado compartido | `~/.local/state/comandos/app-state.sqlite3` | Nueva base SQLite (migraciones 1–9: workspace, eventos, marcas, Pomodoro, progreso, ediciones, terminales rápidas, push, avisos). Cada migración sobre datos existentes deja `app-state.sqlite3.pre-migration-v<N>-<ts>` (0600). | La app anterior no la lee; se conserva intacta. |
| Snapshots de layout | `~/.claude/hooks/app-sessions-v2.json` (+ `.bak`, `.history/`) | Cada pane lleva además `start` (tick de inicio) y `key` (paneKey). Campos añadidos, no se quitan los anteriores. | La app anterior ignora los campos nuevos. |
| Registro de tabs | `~/.claude/hooks/app-tabs.json` | Sin cambio de formato; el orden de la tira lo manda el documento de workspace. | — |
| Foco Pomodoro antiguo | `~/.claude/hooks/focus.json` | Se adopta una vez si sigue vigente y se renombra a `focus.json.migrated-<ts>` (no se borra). | Renombrar de vuelta. |
| Preferencias de estilo Pomodoro | `focus_settings` de `cc_usage` (clave `style`) | Clave nueva. | Ignorada por la app anterior. |
| Eventos históricos | `~/.claude/hooks/events.jsonl` | Se importan una vez (marca `events.legacy_import`); el archivo no se modifica. | — |
| Push (claves VAPID) | `$COMANDOS_PUSH_DIR`, si no junto a la base de estado, si no `$XDG_STATE_HOME/comandos/push` | Se generan en el primer `GET /push/key`. **Conservarlas entre despliegues**: regenerarlas invalida todas las suscripciones. | No borrar al revertir. |
| Dependencias push | `requirements-push.txt` (pywebpush 2.5.0 y fijadas con hash) | Hay que instalarlas en el Python que ejecuta cc-dash; sin ellas `/push/key` responde 503 y el resto funciona. | Desinstalar no afecta a los datos. |
| Ediciones de noticias | `~/.claude/hooks/news-editions.json` (copiar de `config/news-editions.example.json`) | Inertes hasta configurarlas: sin config no se inventan noticias. | Borrar el archivo de config. |
| Telegram | unidad `cc-telegram.service` y enlaces `~/.local/bin/cc-telegram`, `~/.claude/hooks/md2tg.py` | Retirada solo con `COMANDOS_RETIRE_TELEGRAM=1 ./install.sh`; actúa únicamente si los enlaces apuntan a un checkout de ComandOS. No borra `telegram.env` ni historial. | Volver a enlazar la unidad desde `main` y `systemctl --user enable --now cc-telegram.service`. |
| tmux (servidor real) | root key table | Al usar "Paneles → seleccionar" desde un navegador se instalan `user-keys[900..931]` y `User900..User931 → select-pane -t :.N` (foco por dispositivo). No toca `user-keys[990/991]` existentes. | `tmux unbind -n User900` … `User931` (opcional; inocuos). |
| Attach remoto | `bin/cc-webterm-attach` | `tmux attach -f active-pane` en tmux ≥ 3.2 (foco por cliente remoto). | Vuelve con la app anterior. |
| Unidades de usuario | `cc-dash.service`, `cc-notifyd.service`, `cc-proxy.service`, `cc-webterm.service`, `cc-webterm-path.service` | Solo se reinician; no se añaden unidades nuevas. | Reiniciar con el checkout anterior. |

## Paso 0 — base de estado contaminada por pruebas (obligatorio)

Durante la implementación, pruebas que ejecutaban código de cc-dash sin base aislada crearon
`~/.local/state/comandos/app-state.sqlite3` en la ruta real (129 eventos de ensayo `announcement`, un workspace con
solo `local`, cinco backups `app-state.sqlite3.pre-migration-v*`). La app actual no usa ese archivo. Desde el commit
que añade `tests/conftest.py`, la suite aísla la base y falla si la ruta real cambia.

Antes de activar, **apartar (no borrar)** esos archivos para arrancar limpio, con la app parada:

```sh
Q=~/.local/state/comandos/quarantine-test-pollution-20260929
mkdir -p "$Q" && mv ~/.local/state/comandos/app-state.sqlite3* "$Q"/
```

## Respaldo previo (antes de instalar)

```sh
B=~/.local/state/comandos/backup-pre-v1-$(date +%Y%m%d-%H%M%S)
mkdir -p "$B" && chmod 700 "$B"
cp -a ~/.claude/hooks/app-sessions-v2.json ~/.claude/hooks/app-sessions-v2.json.bak \
      ~/.claude/hooks/app-tabs.json ~/.claude/hooks/app-tabs-meta.json \
      ~/.claude/hooks/events.jsonl ~/.claude/hooks/cc-notify.conf "$B"/ 2>/dev/null
[ -f ~/.claude/hooks/focus.json ] && cp -a ~/.claude/hooks/focus.json "$B"/
git -C /home/someguy/codebase/0xJesus/ComandOS rev-parse HEAD > "$B/previous-commit.txt"
```

No copiar tokens (`dash-token`, `telegram.env`) ni contenido de conversaciones al informe.

## Activación (solo servicios necesarios, sin terminar agentes)

1. Actualizar el checkout principal a la rama aprobada (merge o fast-forward acordado con Jesús).
2. `./install.sh` (sin `COMANDOS_RETIRE_TELEGRAM`). Luego, cuando Jesús confirme, `COMANDOS_RETIRE_TELEGRAM=1 ./install.sh`.
3. Reiniciar únicamente: `systemctl --user restart cc-dash.service cc-notifyd.service cc-webterm.service cc-webterm-path.service`.
   **No** reiniciar `tmux.service` ni matar sesiones tmux: la app se reconecta a las sesiones vivas.
4. Reabrir la app de escritorio por su lanzador (instancia única).
5. Opcional push: instalar `requirements-push.txt` en el Python de cc-dash y reiniciar solo `cc-dash.service`.

## Comprobaciones posteriores (acotadas)

- `curl -s -o /dev/null -w '%{http_code}' http://127.0.0.1:4777/` → 200.
- `GET /workspace` responde `ready: true` y contiene las tabs abiertas; la tira muestra los mismos grupos en
  escritorio y remoto.
- Restauración normal: cerrar y reabrir la app; mismas tabs, splits y conversaciones (sin selector de backup).
- Apertura remota por Tailscale: terminal, franja de avisos y lector cargan; `GET /notices` 200.
- `sqlite3 ~/.local/state/comandos/app-state.sqlite3 'select max(version) from schema_migrations'` → 9.

## Reversión de la app (conservando datos)

1. `git -C /home/someguy/codebase/0xJesus/ComandOS checkout <commit de previous-commit.txt>`.
2. `./install.sh` (vuelve a enlazar hooks y dashboard de esa versión).
3. `systemctl --user restart cc-dash.service cc-notifyd.service cc-webterm.service cc-webterm-path.service`.
4. **No** borrar `app-state.sqlite3`, backups, snapshots ni claves push: la versión anterior no los lee ni los
   sobrescribe; quedan para volver a activar.
5. Si se retiró Telegram y se quiere recuperar: re-enlazar la unidad desde el checkout anterior y habilitarla.
6. No reiniciar tmux para arreglar la interfaz.
