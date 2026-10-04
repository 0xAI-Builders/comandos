# Cutover del frente `comandos dash` (Fase 2a)

Procedimiento para el controlador (operador con supervisión humana). Ningún paso lo ejecuta un
subagente. El frente Rust (`comandos dash`) toma el puerto 4777 y reenvía al `cc-dash` Python, que
pasa a escuchar en 4781 con una unidad aparte. Reglas de oro: tmux, sus sesiones y los demás
servicios no se tocan; el único reinicio es el de `cc-dash.service`.

## Qué cambia y qué no

| Elemento | Antes | Después |
|---|---|---|
| Puerto 4777 | Python (`bin/cc-dash`) | Frente Rust (`comandos dash`), mismo puerto y misma puerta de seguridad |
| Puerto 4781 | libre | Python heredado (`cc-dash-legacy.service`) |
| Puerto 4782 | libre | solo la sombra del paso 1 |
| `bin/comandos` (hooks `cc-notify.sh`/`cc-usage-tool.sh`/`cc-status.sh`, `cc-extensions`, broker) | release Fase 1 | **la release nueva al hacer `--stage`**: los hooks la usan en su siguiente turno y el broker en su siguiente arranque; por eso el paso 3 comprueba un hook antes de enlazar `cc-dash` |
| `cc-dash.service` | sin cambios en el archivo: `ExecStart=%h/.local/bin/cc-dash 4777 --no-open` | igual; ahora ese comando es el frente |
| `~/.local/bin/cc-dash` | symlink a `<repo>/bin/cc-dash` | symlink a `bin/comandos` (release activa) |
| Binario `comandos` | `bin/comandos` | `bin/comandos` es symlink a `releases/<sha12>/comandos` |
| Estáticos (`~/.claude/hooks/dash/*`) | los sirve Python | los sirve el frente desde disco, edición en vivo igual que antes |
| Rutas API | Python | el frente las reenvía al Python en 4781 (sin cambios de contrato) |
| Bucles de fondo (modelos, noticias, push, límites, Pomodoro, restaurar webterm, reanudar motor) | Python | siguen en el Python heredado, una sola vez |
| Tailscale (`/` a 4777, `/term` a 4780, `:8443` a 4779) | tal cual | sin cambios; `/` ahora lo atiende el frente |
| ttyd (4779, 4780) | activo | sin cambios (se retira en la Fase 3) |
| `cc-app`, `cc-notifyd`, `cc-next`, hooks | hablan con 4777 | igual |

## Interfaz del frente

`comandos dash [puerto] [--no-open] [--legacy-port N]`

- Puerto por defecto 4777; el primer argumento numérico es el puerto. `--no-open` se acepta y se ignora.
- Heredado: `--legacy-port N` (o `--legacy-port=N`), si no `COMANDOS_DASH_LEGACY_PORT` (vacía cuenta como no definida), si no 4781. La bandera gana al entorno. Si coincide con el puerto propio se rechaza (evita un bucle).
- `COMANDOS_DASH_DIR` cambia el directorio de estáticos (por defecto el `dash/` de `~/.claude/hooks`; admite `~`).
- Token: `~/.claude/hooks/dash-token` (0600; se crea si falta).
- Arranque correcto: `Centro Claude corriendo en http://127.0.0.1:<puerto>  (Ctrl+C para salir)`.
- Salidas: 0 con SIGTERM/SIGINT, 1 configuración inutilizable (directorio de estáticos, token, bind), 2 argumentos o entorno inválidos.
- Escucha solo en 127.0.0.1.

## Diferencias aceptadas

Respecto a `SimpleHTTPRequestHandler` y el despachador Python:

- Estáticos: tabla MIME fija sin `charset`; `.js` se sirve como `text/javascript` (el Python 3.10 daba `application/javascript`). Sin cabeceras `Server` ni `Date` de la librería del Python en las respuestas estáticas.
- `Last-Modified` y 304 por `If-Modified-Since` conservados. El 304 lleva solo `Cache-Control: no-store`. Zonas distintas de UTC o fechas no parseables dan 200.
- `Cache-Control: no-store` en todo.
- `HEAD`: solo se atiende de forma nativa para archivos existentes (cabeceras sin cuerpo, `Content-Length` del archivo). `HEAD` a una ruta reenviada responde `404 {"error":"No encontrado"}` en lugar de lo que diera el Python; la puerta se aplica igual (ruta API remota sin token da 401).
- Todo lo que no es un archivo regular existente (directorios, ausentes, rutas con `..`) se reenvía al Python. Por eso los listados de directorio, el 404 HTML y el 410 de `/operator` siguen saliendo del Python; esto sustituye al «ruling 2» del plan (sin listados, 404 JSON).
- Si un archivo desaparece entre la clasificación y la lectura: `404 {"error":"No encontrado"}`.
- Rutas dinámicas (tabla `DYNAMIC_GET`, copia de las ramas de `_do_GET`) se reenvían aunque exista un archivo homónimo.
- Reenvío: una conexión nueva por petición, sin pool; cabeceras en el orden recibido salvo las hop-by-hop; no se añade `X-Forwarded-For`, `Via` ni `X-Real-IP`.
- Tiempos: una ruta cuyas cabeceras tarden más de 120 s (`handler_timeout`) da 504; el long-poll `/notices/watch` (máx. 25 s) no se ve afectado. El cuerpo reenviado se corta tras 60 s sin datos. Python caído: `502 {"error":"Servidor heredado no disponible"}`.
- `Expect: 100-continue` se reenvía tal cual; sin probar.
- Se rechaza `--legacy-port` igual al puerto propio y un argumento malo ya no se ignora (salida 2).

- Una petición malformada con una cabecera plegada (línea que empieza por espacio, `obs-fold`) recibe
  400 del frente; el Python la pegaba a la cabecera anterior y seguía (p. ej. 401 por token ausente).
  Ningún navegador ni cliente de ComandOS pliega cabeceras.

## Rutas sin llamador vivo (39 → 31 retiradas, 6 reenviadas, 2 nativas)

Hasta la Fase 2a se reenviaban al Python. Desde la 2b, 31 de ellas responden en el frente
`410` con el cuerpo y las cabeceras exactas de `GET /operator`
(`{"error": "El chat de CommandOS se retiró; usa la barra de comandos", "code": "retired"}`);
GET/POST `/events/v2` no se retiran: son nativas (dominio B). Seis siguen reenviadas porque
`dash/index.html` y `dash/session-controls.js` (versión commiteada) aún las llaman. Fuente:
inventario §1.12. La prueba `retired_routes_have_no_live_caller` (recorre `dash/`, `bin/`, `lib/`
y `adapters/`, salvo `bin/cc-dash` y `lib/operator_catalog.py`) falla si alguna de las 31 vuelve a
tener llamador.

Retiradas, sin ningún llamador (9): `GET /pomodoro/report`, `GET /project-profiles`, `GET /events`,
`POST /project-profile`, `POST /pause`, `POST /usage/capture`, `POST /chains/delete`,
`POST /app/command`, `POST /event`.

Retiradas, solo en `lib/operator_catalog.py` (22): GET `/dedication`, `/ui-log/summary`,
`/session-brain`, `/usage/guard`, `/usage/changes`, `/usage/provider-compare`,
`/usage/experiments`, `/usage/analytics`, `/usage/interactions`; POST `/optimization/default`,
`/skill-toggle`, `/mcp-toggle`, `/usage/experiment`, `/usage/rating`, `/usage/refresh`,
`/usage/quota`, `/usage/subscription`, `/usage/settings`, `/news/refresh`, `/models/refresh`,
`/open-with-account`, `/tab-new`.

Siguen reenviadas hasta que la barra de comandos se fusione (6): GET y POST `/proxy`,
POST `/harness/switch`, POST `/model/switch-cancel`, GET `/session-config-history`,
POST `/session/recover`.

Nativas (2): GET y POST `/events/v2`.

## Procedimiento

Todos los comandos desde el worktree `rust-fase2a` salvo que se indique. Detenerse y revertir ante
cualquier fallo.

### 0. Previos

```sh
cargo build --release -p comandos-cli          # binario nuevo; no se instala todavía
systemctl --user is-active cc-dash.service      # active (Python en 4777)
tailscale serve status                          # anotar la salida para compararla en el paso 4
ss -ltn 'sport = :4781 or sport = :4782'        # ambos libres
```

### 1. Sombra en 4782 contra el Python vivo (sin segundo Python)

El frente de sombra reenvía **al `cc-dash` que ya corre en 4777**: ve el estado real, no arranca un
segundo Python y, por tanto, no duplica ningún bucle de fondo. El Python vivo recibe las peticiones
desde loopback con las mismas cabeceras, así que decide igual que si llegaran directas.

```sh
NEW=$HOME/codebase/0xJesus/ComandOS/.build/target/release/comandos   # binario recién compilado desde main
"$NEW" --help | grep -q dash || echo "ESE BINARIO NO TIENE dash: no seguir"
"$NEW" dash 4782 --legacy-port 4777                                    # en una terminal del controlador
```

Desde otra, el arnés (sobre copias de `~/.claude/hooks` dentro de un namespace de red privado; nunca
toca el estado real ni 4777/4778):

```sh
cargo run -p xtask -- parity --fixture xtask/parity/frente.jsonl --hooks ~/.claude/hooks
```

Debe salir sin `DIFF` (código 0). Navegación manual en `http://127.0.0.1:4782` con `chrome-bg`:
índice (tarjetas y pestañas), notificaciones (incluido el long-poll de 25 s) y uso; comparar con
`http://127.0.0.1:4777`. Parar la sombra con Ctrl+C (sale con 0).

### 2. Python heredado en 4781 — justo antes del cutover

Este Python arranca los bucles de fondo una segunda vez mientras el de 4777 siga vivo (push a los
30 s, modelos a los 90 s, noticias a los 120 s, Pomodoro ≤ 30 s): encadenar este paso con el 3 sin
pausa, en la misma terminal.

```sh
ln -sf "$HOME/codebase/0xJesus/ComandOS/systemd/cc-dash-legacy.service" ~/.config/systemd/user/cc-dash-legacy.service   # desde el checkout principal, nunca desde un worktree
systemctl --user daemon-reload
systemctl --user enable --now cc-dash-legacy.service
curl -s 127.0.0.1:4781/prefs >/dev/null && echo "4781 responde"
```

### 3. Cutover

```sh
"$NEW" install --stage                          # instala la release y la enlaza en bin/comandos
~/.local/share/comandos/bin/comandos --help | grep -q dash || echo "STAGE MALO: comandos install --rollback-release"
~/.local/share/comandos/bin/comandos hook claude-status >/dev/null && echo "hooks OK con la release nueva"
~/.local/share/comandos/bin/comandos install --link cc-dash
systemctl --user restart cc-dash.service
```

El tablero queda sin respuesta unos 3 s; `cc-app` reintenta solo. tmux y las sesiones no se tocan.

```sh
curl -s -o /dev/null -w '%{http_code}\n' http://127.0.0.1:4777/        # 200
curl -s 127.0.0.1:4777/prefs                                           # igual que en 4781
readlink ~/.local/bin/cc-dash
```

### 4. Verificación

- Journal del frente: `journalctl --user -u cc-dash.service -n 50 --no-pager` con la línea de arranque y sin errores.
- `tailscale serve status` idéntico a la salida anotada en el paso 0.
- Tablero remoto desde el teléfono (con token): carga el índice, abre notificaciones y una pestaña de terminal.
- Carga sostenida: **nunca `xtask poll` contra 4777** (su `POST /presence` registra un dispositivo falso `xtask-poll` sin audio y silencia chimes y push reales durante ~11 min). Medir con la pila aislada: `cargo run -p xtask -- poll --shadow --minutes 10` (namespace de red privado, copia de `~/.claude/hooks`); el Pss del frente debe quedar plano (pendiente ≈ 0 entre el minuto 5 y el 10). Para el frente real, basta `grep Pss /proc/$(systemctl --user show -p MainPID --value cc-dash.service)/smaps_rollup` a intervalos.
- `ss -ltn 'sport = :4777 or sport = :4781'` muestra ambos puertos con su proceso.

### 5. Reversión

Fallo del enrutado o de la puerta (vuelve el Python a 4777):

```sh
~/.local/share/comandos/bin/comandos install --rollback cc-dash
systemctl --user restart cc-dash.service
```

Si se abandona el frente definitivamente:

```sh
systemctl --user disable --now cc-dash-legacy.service
rm ~/.config/systemd/user/cc-dash-legacy.service && systemctl --user daemon-reload
```

Si el fallo es del binario (no del enlace), volver a la release anterior:

```sh
~/.local/share/comandos/bin/comandos install --releases   # '*' marca la actual
~/.local/share/comandos/bin/comandos install --rollback-release
systemctl --user restart cc-dash.service
```

`--rollback-release` solo afecta a procesos nuevos; el reinicio es lo que aplica la release anterior.
Tras revertir con `--rollback cc-dash`, el Python vuelve a 4777 y el de 4781 queda sobrante: apagarlo
con `disable --now` para no duplicar los bucles de fondo.

## Ejecutado — 4 de octubre de 2026, 12:04 (release `17aa1bea2309`, main `8917c9d`)

Sombra previa (frente de main en 4782 → Python vivo 4777): 19 rutas idénticas en estado, cabeceras y
cuerpo (solo cambian `serverNowMs` y `burn`/`pace`, dependientes del instante); long-poll real de
`/notices/watch?rev=…` por el frente: 25,1 s y 200; `xtask parity` 16/16 en namespace. Ráfaga de 20
`/prefs` en paralelo: por el frente todas < 0,11 s; directas al Python, 5 de 20 esperaron 1,03 s
(backlog 5 del `socketserver`) — la dosificación del frente elimina esa espera. `chrome-bg` estaba
caído, así que la navegación manual no se hizo; la cobertura HTTP de arriba la sustituye.

Cutover (pasos 2+3 encadenados, 12:04:48–12:04:52): `cc-dash-legacy.service` enlazado desde el
checkout principal y activo en 4781; `install --stage` migró el archivo plano de la Fase 1 a
`releases/1421852285eb` (`previous`) e instaló `17aa1bea2309`; `comandos hook claude-status` OK con la
release nueva; `install --link cc-dash`; `systemctl --user restart cc-dash.service`. Tablero sin
respuesta ≈ 2 s. tmux, cc-app y el broker no se tocaron (el broker sigue con la release anterior
hasta su siguiente arranque; el protocolo cliente↔daemon no cambió entre ambas).

Verificación: journal del frente con la pancarta y `NRestarts=0`; heredado sin errores; `tailscale
serve status` idéntico al anotado; `ss` muestra `cc-dash` (Rust) en 4777 y `python3` en 4781;
cc-app (4 conexiones WebKit) y notifyd (python3) siguen conectados a 4777; remoto simulado
(`Host: nodo-01.tail63a117.ts.net` + `X-Forwarded-For`): `/state` sin token 401, con token 200,
índice y estáticos como en el Python; `POST /pane/type` con cuerpo no objeto 400 (del Python);
`/webterm-token` 200. Pss del frente 4,8 MiB al minuto 2 (el Python nuevo en 4781: 389 MiB).

Reversión disponible: `~/.local/share/comandos/bin/comandos install --rollback cc-dash` (vuelve el
Python a 4777) o `--rollback-release` (vuelve a `1421852285eb`), ambos con reinicio de la unidad.

## 2b: dominios nativos

Procedimiento para el controlador, como el de la 2a. El frente ya está en 4777 (release
`17aa1bea2309`) y el Python heredado en 4781 (`cc-dash-legacy.service`); la 2b solo cambia el
binario del frente. Ni el Python ni las unidades ni tmux se tocan.

### Qué cambia

El frente responde él mismo, sin el Python, 57 rutas de cinco dominios; lo demás sigue igual
(reenviado a 4781). Detalle y líneas del Python en
`docs/superpowers/plans/2026-10-04-fase-2b-dominios-nativos-i.md` («Mapa de rutas nativas»).

| Dominio | Rutas nativas |
|---|---|
| D · lecturas ligeras (8) | GET `/prefs`, `/tabs`, `/tab-history`, `/tab-models`, `/active-tab`, `/tmux-mouse`; POST `/prefs-set`, `/tmux-mouse` |
| B · eventos y marcas (4) | GET/POST `/work-marks`, GET/POST `/events/v2` |
| A · notificaciones (8) | GET `/notices`, `/notices/prefs`, `/notices/watch`, `/notifs/count`; POST `/presence`, `/notices/read`, `/notices/sound`, `/notices/prefs` |
| C · workspace (6) | GET `/workspace`, `/workspace/close-group`, `/workspace/client`; POST `/workspace`, `/workspace/sort` (solo `restore`), `/workspace/client` |
| E · retiradas (31) | las 31 de la sección anterior → 410 de `/operator` |

Siguen en el Python: POST `/workspace/close-group` (cierra pestañas con tmux), `/workspace/sort`
con `by`, las 6 rutas de la barra de comandos sin fusionar (sección anterior), `/operator*`,
`/notify-popup`, `/test`, push, `/state` y todo lo no listado. Una petición nativa con una entrada
que Rust no reproduce con certeza (dígitos no ASCII, enteros enormes, formas JSON raras, snapshot
de tmux exótico) también se reenvía, sin haber escrito nada.

El frente abre `~/.local/state/comandos/app-state.sqlite3` (misma resolución que
`lib/app_state.py`: `COMANDOS_STATE_DB`, `XDG_STATE_HOME`, `~/.local/state`) con un único hilo de
base. Nunca baja la versión del esquema.

Interruptores:

- `comandos dash --no-native` o `COMANDOS_DASH_NATIVE=0`: todo reenviado, la 2a exacta (A/B).
- `COMANDOS_DASH_TRACE_FORWARD=1`: una línea `comandos dash: reenvío MÉTODO /ruta` por reenvío,
  sin la consulta (diagnóstico: qué sigue llegando al heredado).

### Diferencias y comportamientos aceptados (2b)

- Las 31 rutas retiradas dejan de ejecutar su acción y dan 410 con el cuerpo de `/operator`.
- Puerta de base: si al admitir el primer trabajo la base tiene una migración que el binario no
  conoce (un Python más nuevo migró) o falla de forma transitoria (`SQLITE_BUSY`), **todo** el
  conjunto nativo se apaga hasta reiniciar la unidad: el journal muestra una sola línea
  `comandos dash: … rutas nativas desactivadas, todo se reenvía al heredado` y todo se reenvía (la
  2a). Arreglo: con la base libre (o con un binario que conozca la migración),
  `systemctl --user restart cc-dash.service`.
- `/notices/watch` que declina a mitad de la espera: el Python vuelve a esperar desde cero (hasta
  ≈ 50 s en total). El `urlopen` de 35 s de cc-app vence y reintenta a los 2 s; el tablero no lo
  nota.
- GET `/workspace` y `/workspace/close-group` pueden confirmar una revisión de sincronización desde
  Rust, como ya hacía el Python. El `requestId` es determinista (`sync-<rev>-<sha256[:24]>`): Rust
  y Python sincronizando a la vez no duplican revisiones.
- La caché de familias de `fc-list` (60 s) es por proceso: tras instalar una fuente, Rust y el
  Python pueden tardar distinto en verla (≤ 60 s).

### 0. Previos

```sh
cd ~/codebase/0xJesus/ComandOS && git log -1 --oneline        # main con la Fase 2b fusionada
CARGO_TARGET_DIR=$PWD/.build/target nice -n 10 cargo build --release -p comandos-cli -j 6
CARGO_TARGET_DIR=$PWD/.build/target nice -n 10 cargo build -p xtask -j 6
NEW=$HOME/codebase/0xJesus/ComandOS/.build/target/release/comandos
XT=$HOME/codebase/0xJesus/ComandOS/.build/target/debug/xtask
grep -qa COMANDOS_DASH_NATIVE "$NEW" || echo "BINARIO SIN 2b: no seguir"   # `dash --help` arrancaría el servidor
systemctl --user is-active cc-dash.service cc-dash-legacy.service    # active active
~/.local/share/comandos/bin/comandos install --releases              # anotar la actual ('*': 17aa1bea2309 o posterior)
ss -ltn 'sport = :4782'                                               # libre
```

### 1. Sombra en 4782 con nativo, contra el heredado 4781

La sombra reenvía al Python de 4781 (4777 ya es Rust) y usa la base real: sus rutas nativas
escriben lo mismo que el tablero (presencia, avisos leídos, workspace). **Nunca** `xtask poll`
contra la sombra, 4777 ni 4781: la carga va siempre con `--shadow` (pila aislada).

```sh
COMANDOS_DASH_TRACE_FORWARD=1 "$NEW" dash 4782 --legacy-port 4781 2> /tmp/sombra-2b.log
```

Desde otra terminal, en `~/codebase/0xJesus/ComandOS`:

```sh
"$XT" parity --fixture xtask/parity/frente.jsonl --hooks ~/.claude/hooks \
  --state-db ~/.local/state/comandos/app-state.sqlite3 --comandos "$NEW"          # 0 DIFF
"$XT" poll --shadow --minutes 10 --hooks ~/.claude/hooks \
  --state-db ~/.local/state/comandos/app-state.sqlite3 --comandos "$NEW"          # Pss plano
for r in /prefs /tabs /tab-history /active-tab /work-marks /notifs/count /notices/prefs /workspace/client; do
  cmp -s <(curl -s 127.0.0.1:4782$r) <(curl -s 127.0.0.1:4781$r) && echo "igual $r" || echo "DISTINTO $r"
done                                    # un DISTINTO aislado puede ser estado que cambió entre ambas: repetir
```

`--state-db` copia la base real en solo lectura (backup de SQLite) a los dos HOME del namespace.

Navegación manual en `http://127.0.0.1:4782` con `chrome-bg` (vía `cc-browser-expose start 4782`):
índice, cajón de avisos (abrirlo, marcar uno leído, dejarlo abierto 30 s para ver el long-poll),
cambiar el tema (POST `/prefs-set`), activar el ratón de una pestaña, ordenar grupos y deshacer
(POST `/workspace/sort` con `restore`). Luego:

```sh
grep -c 'rutas nativas desactivadas' /tmp/sombra-2b.log                # 0
grep 'reenvío' /tmp/sombra-2b.log | sed 's/.*reenvío //' | sort | uniq -c | sort -rn
```

Ninguna ruta de la tabla «Qué cambia» debe aparecer. Sí pueden aparecer `GET /operator`, `/state`,
estáticos inexistentes, POST `/workspace/close-group` y las 6 de la barra de comandos. Ctrl+C
para parar (sale con 0).

### 2. Cutover

```sh
"$NEW" install --stage                     # instala la release y la enlaza en bin/comandos
grep -qa COMANDOS_DASH_NATIVE "$(readlink -f ~/.local/share/comandos/bin/comandos)" || echo "STAGE MALO: ~/.local/share/comandos/bin/comandos install --rollback-release"
~/.local/share/comandos/bin/comandos hook claude-status >/dev/null && echo "hooks OK con la release nueva"
systemctl --user restart cc-dash.service
curl -s -o /dev/null -w '%{http_code}\n' http://127.0.0.1:4777/        # 200
journalctl --user -u cc-dash.service --since -2min --no-pager | grep -c 'rutas nativas desactivadas'   # 0
```

`~/.local/bin/cc-dash` ya apunta a `bin/comandos` desde la 2a: no hace falta `install --link`. El
tablero queda sin respuesta unos 3 s; `cc-app` reintenta solo. El Python de 4781 no se toca. Los
hooks usan la release nueva en su siguiente turno y el broker en su siguiente arranque.

### 3. Verificación por dominio

Trazar 10 minutos de tráfico real y comprobar qué sigue llegando al heredado:

```sh
systemctl --user set-environment COMANDOS_DASH_TRACE_FORWARD=1 && systemctl --user restart cc-dash.service
sleep 600
journalctl --user -u cc-dash.service --since -11min --no-pager | grep 'reenvío' \
  | sed 's/.*reenvío //' | sort | uniq -c | sort -rn
systemctl --user unset-environment COMANDOS_DASH_TRACE_FORWARD && systemctl --user restart cc-dash.service
```

`/state` (el sondeo principal del tablero y de cc-app) sigue reenviado: es lo más frecuente de la
lista y es lo esperado.

- **D**: no aparecen `/active-tab`, `/tab-models`, `/prefs`, `/tabs`, `/tab-history`, `/tmux-mouse`, POST `/prefs-set`, POST `/tmux-mouse`. Cambiar el tema en cc-app se refleja en el tablero y viceversa.
- **B**: no aparecen `/work-marks` ni `/events/v2`. Una marca puesta desde el tablero se ve en cc-app.
- **A**: no aparecen `/notices*`, `/notifs/count`, `/presence`. Con el cajón abierto, un aviso nuevo de un agente aparece en ≤ 1 s (long-poll) y la campana coincide con la de cc-app.
- **C**: de workspace solo aparece POST `/workspace/close-group` (al cerrar un grupo). Mover pestañas entre grupos en el tablero y en cc-app mantiene la misma `revision` en ambos (`curl -s 127.0.0.1:4777/workspace | jq .revision` y `curl -s 127.0.0.1:4781/workspace | jq .revision` iguales).
- **E**: ninguna de las 31. Las 6 de la barra de comandos sí pueden aparecer.
- Memoria: `grep Pss /proc/$(systemctl --user show -p MainPID --value cc-dash.service)/smaps_rollup` al minuto 1 y al 10: plano (± 1 MiB).

### 4. Reversión

A/B inmediato sin cambiar binario (todo reenviado, comportamiento 2a):

```sh
systemctl --user set-environment COMANDOS_DASH_NATIVE=0 && systemctl --user restart cc-dash.service
# deshacer: systemctl --user unset-environment COMANDOS_DASH_NATIVE && systemctl --user restart cc-dash.service
```

Volver a la release anterior (`17aa1bea2309`, la 2a):

```sh
~/.local/share/comandos/bin/comandos install --releases   # '*' marca la actual
~/.local/share/comandos/bin/comandos install --rollback-release
systemctl --user restart cc-dash.service
grep -qa COMANDOS_DASH_NATIVE "$(readlink -f ~/.local/share/comandos/bin/comandos)" && echo "SIGUE LA 2b"
```

Las escrituras hechas por las rutas nativas están en los mismos archivos y la misma base que usa
el Python, con el mismo formato: revertir no necesita migrar ni limpiar nada. Volver a la 2b tras
una reversión por release: repetir el paso 2 con el mismo `$NEW`.

### Medido antes del cutover (rama `migration/rust-fase2b`, `5c666ed`)

Arnés en namespace de red privado, copia de `~/.claude/hooks` y copia de solo lectura de
`app-state.sqlite3`, binario release de la rama.

- Suite del workspace: 659 pruebas, 0 fallos, 1 ignorada (herramienta manual de RSS).
- `xtask parity` con nativo: 98 OK, 0 DIFF, 0 SKIP de 98. Reenviadas al heredado, 5 rutas
  distintas: GET `/state` (2), GET `/operator`, GET `/no-existe.css`, GET `/vendor/`,
  POST `/workspace/close-group`.
- `xtask parity --no-native` sobre una copia del fixture sin los 31 casos `e-*` (con nativo apagado
  dan DIFF por diseño): 67 OK, 0 DIFF, 0 SKIP de 67. Reenviadas, 31 rutas distintas: las 26
  nativas de los dominios A–D y las 5 de arriba.
- `xtask poll --shadow --minutes 10` (con nativo): 2910 peticiones, 0 errores de transporte,
  300 no-2xx (tantas como POST `/terminal-panes` a la sesión inexistente `poll`, que responde el
  heredado). Pss del frente 12,8 MiB al minuto 0 y 18,2 MiB del minuto 1 al 10
  (18 693 → 18 685 KiB); desde el minuto 5, 18 657–18 685 KiB, pendiente 267 KiB/h (+28 KiB en
  5 min): plano. Reenviadas, 5 rutas distintas: GET `/state` (500), POST `/terminal-panes` (300),
  GET `/usage/state` (60), GET `/pomodoro` (40), GET `/analytics/week` (10); ninguna nativa
  (`/prefs`, `/active-tab`, `/notices`, `/notices/watch`, `/work-marks`, `/presence`,
  `/tab-models`, `/workspace`). El Python de la pila aislada pasó de 45 a 273 MiB en el mismo
  tiempo.
