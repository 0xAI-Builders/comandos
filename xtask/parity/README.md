# xtask parity

Compara `bin/cc-dash` (oráculo Python) con `comandos dash` (frente Rust) sobre copias
privadas de `~/.claude/hooks`.

```
cargo build -p comandos-cli
cargo run -p xtask -- parity --fixture xtask/parity/frente.jsonl [--fixture xtask/parity/2f/tabs.jsonl …] --hooks ~/.claude/hooks [--keep] [--comandos RUTA] [--state-db RUTA] [--usage-db RUTA] [--no-native]
```

- `--fixture <ruta>` es repetible: las líneas se ejecutan en el orden de los archivos.

- `--state-db <ruta>`: copia de solo lectura (backup de SQLite) de la base de estado a los dos
  HOME; usar con `~/.local/state/comandos/app-state.sqlite3`.
- `--usage-db <ruta>`: copia de solo lectura (backup) de la base de uso a los dos HOME, sobre la
  que trajo `cp -a`; usar con `~/.claude/hooks/comandos-usage.sqlite` para que las rutas que la
  leen (GET `/pomodoro`) comparen sobre datos consistentes.

**Aislamiento.** El arnés (y `poll`) se reejecuta dentro de `unshare -Urn` con el loopback
levantado: ahí 4777/4778/4779/4780/4781 no existen, así que el oráculo Python no puede llegar a
servicios reales (cc-webterm, tailscale, web push, el 4778 de notificaciones). Si `unshare -Urn`
falla, no se lanza nada (sin opción para saltárselo). Además, en las copias se borran
`webterm-enabled`, `news-editions.json*`, `news-watch.json`, `model-watch.json`, se antepone al
`PATH` un `fakebin` con ejecutables vacíos (`cc-webterm`, `tailscale`, `systemd-run`,
`notify-send`, `pw-play`, `xdg-open`…) y todo symlink de la copia que apunte fuera de
`temp_dir()` o del repo se sustituye por una copia del archivo (o se borra). Las copias se
borran al terminar o al fallar, salvo `--keep`.

**Procedimiento.** `--hooks` se copia (`cp -a`) dos veces. Python A (copia 1) es el oráculo
directo; el frente Rust B y un segundo Python C (ambos copia 2) son el lado bajo prueba: B
reenvía a C, de modo que las rutas reenviadas también se comparan (Python directo vs Python a
través de Rust). Tmux privado por copia (`TMUX_TMPDIR`), sin `$TMUX`, puertos libres, mismo
directorio estático. Solo si el frente responde 502 «Servidor heredado no disponible» una ruta
`forwarded` se marca `SKIP`.

Salida: tabla `OK`/`DIFF`/`SKIP` y resumen; código 1 si hay `DIFF`. Los pares que difieren
quedan en `<tmp>/comandos-parity-results-<t>/<n>.{py,rs}`.

`xtask poll --shadow --hooks <ruta> --minutes N` monta la misma pila y carga el frente con dos
clientes (tablero y `cc-app`), cada uno con su calendario completo y su long-poll; anota en un
archivo temporal (usar `--out` para añadir a `docs/verification/rss.jsonl`).

## Fixture JSONL (una petición por línea)

| Campo | Significado |
|---|---|
| `name`, `method`, `path` | identificación y petición |
| `headers` | objeto; `{{token}}` se sustituye por el `dash-token` de la copia |
| `body` | `null`, texto literal o JSON (se serializa) |
| `volatile` | punteros JSON (con `*` para índices) sustituidos por `"<volátil>"` en ambos lados |
| `expect` | `same` (estado + todos los valores de `content-type`/`content-length`/`cache-control`/`connection` + cuerpo), `static-accepted` (estado + cuerpo), `status-only` (solo estado), `skip` |
| `forwarded` | `true` si el frente la reenvía: solo si responde 502 «Servidor heredado no disponible» → `SKIP` |
| `reason` | texto para `skip` |
| `setup` | lista de órdenes de tmux (sin `tmux`), p. ej. `[["new-session","-d","-s","p2f"]]`: antes de la línea, cada una corre en los **dos** servidores tmux privados (`-f /dev/null -S <socket de la copia>`, HOME de la copia, sin `DISPLAY` ni DBus). Una orden `kill-*` (o vacía) es un error al cargar el fixture: el arnés nunca mata servidores ni sesiones por fixture. Si una orden falla, la línea cuenta como `DIFF`. |
| `files` | archivos que se comparan entre las dos copias tras la respuesta (solo si la respuesta ya dio `OK`): relativos a `~/.claude/hooks`, o al HOME de la copia si empiezan por `~/`. Ambos se normalizan como el gemelo de las pruebas (`term-r<n>`, `ts`/`closedAt`/`updated`/`at`/`heartbeatAt` numéricos y nombres `<19 dígitos>-<hex>.json`); ausente en las dos cuenta como igual. Una diferencia es `DIFF` con el nombre del archivo. |

Ejemplo de línea que muta (registrar una pestaña sobre una sesión que el `setup` crea en los dos
servidores y comparar los archivos que escribe):

```json
{"name":"t-tab-register","method":"POST","path":"/tab-register","headers":{"Host":"127.0.0.1","Content-Type":"application/json"},"body":{"session":"p2f","label":"Proyecto"},"setup":[["new-session","-d","-s","p2f"]],"files":["app-tabs.json","app-tabs-meta.json"],"volatile":[],"expect":"same"}
```

Cuerpos: **sin `volatile` se comparan los bytes crudos** (el orden de claves, el espaciado y el
formato de números cuentan). Con `volatile` se parsean ambos, se sustituyen los punteros y se
reserializan con `comandos_core::json::response_dumps` (orden de inserción) antes de comparar;
en ese modo `content-length` no se compara (los valores volátiles cambian la longitud), y el formato de floats y de
escapes **no** queda cubierto (lo canoniza el volcado). Las peticiones van con keep-alive para
que el `Connection: close` de los rechazos sea observable.
