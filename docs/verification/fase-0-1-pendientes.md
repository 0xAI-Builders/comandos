# Fase 0/1 — pendientes y decisiones registradas (4 de octubre de 2026)

Lo que la Fase 0/1 dejó diferido a propósito, con la razón, para que el siguiente plan lo recoja y nada
quede suelto. Las decisiones de diseño que se tomaron por el camino van al final.

## Para el siguiente plan (antes de volver a desplegar nada)

1. **Releases versionadas del binario.** `comandos install --stage` sobreescribe el único binario
   `~/.local/share/comandos/bin/comandos`; el rollback registrado solo lleva de vuelta al Python/bash. Hace
   falta `bin/comandos-<sha>` + enlace `current` atómico y `install --rollback-release`. Primera tarea del
   plan siguiente: desde la Fase 2 cada stage malo rompería hooks, `cc-extensions` y broker sin vuelta Rust.
2. **Filtros de herramientas en el cliente fino (Fase 1b).** Los servidores con `enabled_tools`/
   `disabled_tools` van hoy en proxy directo; aplicar el filtro por sesión en el cliente fino para que
   también compartan upstream (el prender/apagar MCP por sesión ya se preserva: vive en el `mcp.json` por
   sesión).
3. **Suscripciones y nivel de log globales en el broker.** `resources/updated` llega a todas las sesiones,
   `resources/unsubscribe` y `logging/setLevel` de una afectan a todas, `roots/list_changed` lo contesta la
   más nueva (spec §4.7 pide repartirlas). Ningún servidor del catálogo las usa hoy.
4. **Daemon monohilo.** Analiza y reserializa cada mensaje (hasta 256 MiB) de todas las sesiones en un
   hilo. Alternativas: reescribir solo el `id` empalmando bytes, o runtime multihilo.
5. **Consolidar helpers de compatibilidad Python** en `comandos-core::py`: `float repr` existe en
   `extensions/lib.rs`, `runtime/hooks/py.rs`, `store/usage.rs` y core; `py_truthy` ≈ `hooks::py::truthy`;
   `hooks::which` ≈ `upstream::find_in_path` (tratan distinto una entrada vacía de PATH).
6. **Limpieza de repo:** partir `comandos-store/src/usage.rs` (1077 líneas) en `usage/{schema,pyval}.rs`;
   mover los binarios de prueba de `crates/comandos-extensions/src/bin` a `crates/comandos-testbins`;
   borrar `tests/test_serve.py` (espera -32603 a 8 MiB, obsoleto) y el caso opencode de
   `tests/test_native_extension_metadata.py` al retirar el Python; `serve_parity.rs` adopte
   `support::oracle`; `expect` sobre invariantes en `events_cli.rs:147`, `store/lib.rs:164`, `usage.rs:107`.
7. **Unidad systemd y envoltorios fuera de rutas del worktree/checkout.** Hoy
   `~/.config/systemd/user/comandos-broker.service` apunta al worktree y los envoltorios `adapters/*` del
   checkout principal están sin commit; un `git checkout -- adapters` o `git worktree remove` deshace el
   cutover. Se resuelve al fusionar la rama (los envoltorios se commitean) e instalando la unidad por
   `comandos install`.
8. **Observar en uso real (broker):** servidores que pidan `roots/list`, `sampling` o `elicitation`
   (el `initialize` interno no anuncia capacidades; marcar `shared:false` a los que lo necesiten);
   ahorro real con el patrón de Jesús (223 pares servidor|cwd distintos entre 316 proxies: la
   compartición es por proyecto); `__deliver` sin `process_group(0)` y orden entre eventos seguidos
   (igual que el bash).

## Menores diferidos por tarea

- **Dispatch (T2):** `tests/dispatch.rs` cubre 3 de 8 alias; `usage()` de `events_cli` dice `comandos-events`.
- **Install (T6):** registro FILE/ABSENT queda obsoleto si falla el swap tras el respaldo; `.orig` previo se
  sobreescribe; rollback de archivo sin fallback EXDEV; nombre inválido sale 1 y no 2.
- **Proxy HTTP (T4/T5):** `initialize` sin `params` → Python -32602, Rust responde; tipado escalar laxo
  (`1e3`→`1000.0`); `repr` no escapa U+00A0/U+2028; EPERM→`OSError` vs `PermissionError`; `-0` entero;
  stdin cerrado antes de init: Python sale 1, Rust 0; transporte sin sufijo `for <name>: <class>`.
  Pendientes de sombra real (no bloqueantes, cubiertos por tests): SSE→POST, peticiones servidor→cliente,
  404 por sesión expirada, 401 con refresh en vivo, `transport:"sse"` legado, paginación de `tools/list`,
  `cancelled`, timeout 60 s, respuestas >8 MiB en vivo.
- **Mux (T8):** `initialize` no traduce `progressToken`; capabilities se sobreescriben en `initialize`
  duplicado; una línea que cruza el tope en el mismo chunk se acepta hasta tope + búfer.
- **Daemon (T9):** un upstream colgado retiene attaches 5 s durante 30 s; carrera de 100 ms en stdin al
  caer al directo; idle corre antes de `listo`; errores de spawn sin cooldown; mapa de fallos sin poda;
  el `PATH`/entorno del primer attach fija el del proceso compartido; el cliente ignora SIGTERM mientras
  el downstream está parado (`send().await`).
- **Hooks (T10/T11):** `__deliver` sin grupo propio; orden de uso entre eventos seguidos; marcador
  `HOME=` sin `\0` inicial en `wait_for_delivery`; colación: espacio bajo `es_MX` difiere, locale no
  instalado cae a C en glibc pero ICU sigue; un id cancelado recibe -32603 espurio tras reinicio del broker.
- **Usage (T10b):** `py_float_repr` sin test unitario; `started_at_ms` REAL/TEXT no coercionado.
- **12a:** entrada vacía de PATH, bits `0o111` vs `X_OK`, rutas no UTF-8.
- **Pruebas con reloj:** `hook_claude_extras_parity.rs` (< 2 s) y la ventana 4–15 s del test de opencode
  pueden fallar bajo carga.
- **Trailer `Claude Opus 5.5`** en 6 commits (convención del proyecto: Fable 5.1): inocuo, se deja.

## Decisiones de diseño tomadas en esta fase

- `rusqlite` con `bundled` (la máquina no tiene `libsqlite3-dev`; binario autocontenido).
- La base de uso real es `~/.claude/hooks/comandos-usage.sqlite` (la del Python); `COMANDOS_USAGE_DB` la
  resuelve el llamador.
- El proxy responde `protocolVersion` como el Python: la pedida si la conoce, si no la última; el broker
  negocia por cliente contra la del upstream (12b). Métodos desconocidos → -32601 (spec); peticiones antes
  de `initialized` se toleran.
- `notifications/progress` no se difunde: se traduce `progressToken` por petición y va solo al dueño.
- La clave de compartición es el spec efectivo (command, args, cwd, env expandido) sin `shared`; catálogo
  o HOME distintos → el attach falla y el `serve` cae al proxy directo en silencio (paridad con Python;
  `COMANDOS_DEBUG=1` lo muestra).
- El `initialize` interno del broker no anuncia capacidades (sin roots/sampling/elicitation cruzados);
  `shared:false` o `DEDICATED` (navegadores, teams, claude-codex, x-suite) para quien lo necesite.
- `MemoryMax=24G` en la unidad (el cgroup contiene todos los upstreams; hoy ≈20 GB de MCP) y
  `LimitNOFILE=65536`.
- JSON inválido en un hook → no se escribe nada (el bash creaba un «Claude terminó» fantasma para `$PWD`).
- La sombra en vivo con hook Rust y bash en paralelo sobre el mismo `state` se sustituyó por la sombra
  offline con payloads reales (dos escritores habrían colisionado y duplicado avisos).
- La versión remota/móvil es la de escritorio salvo controles remotos y layout flex (spec §4.5, 4-oct).
