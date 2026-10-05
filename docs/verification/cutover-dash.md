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
# Lo anterior vive solo en la memoria del gestor de usuario (se pierde al reiniciar o reloguear).
# Para que sobreviva: drop-in persistente y reinicio.
mkdir -p ~/.config/systemd/user/cc-dash.service.d && printf '[Service]\nEnvironment=COMANDOS_DASH_NATIVE=0\n' > ~/.config/systemd/user/cc-dash.service.d/no-native.conf
systemctl --user daemon-reload && systemctl --user restart cc-dash.service
# Deshacer: rm ~/.config/systemd/user/cc-dash.service.d/no-native.conf && systemctl --user daemon-reload && systemctl --user restart cc-dash.service
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

### Ejecutado — 4 de octubre de 2026, 15:58 (release `2bae7f9cd7d6`, main `77cd5a5`)

Sombra previa (`"$NEW" dash 4782 --legacy-port 4781` con traza de reenvío): `xtask parity` desde
main con `--state-db` sobre la base real (copia de solo lectura): **98 OK, 0 DIFF**; comparación en
vivo de 13 rutas nativas (prefs, tabs, tab-history, tab-models, active-tab, work-marks,
notifs/count, notices/prefs, notices, events/v2, workspace, workspace/client, tmux-mouse) contra
el Python de 4781: idénticas byte a byte; `/pomodoro/report` y `/dedication` 410 por diseño
(retiradas); long-poll nativo `wait=3` → 3,07 s y cuerpo idéntico al del Python; remoto simulado
a `/prefs`: 401 sin token, 200 con token; cero reenvíos y cero «nativas desactivadas» en la traza.
`chrome-bg` seguía caído: la navegación manual no se hizo.

Cutover (15:58:05–15:58:08): `install --stage` instaló `2bae7f9cd7d6` (anterior `17aa1bea2309`);
`comandos hook claude-status` OK con la release nueva; `systemctl --user restart cc-dash.service`
(≈2 s sin tablero). Sin operaciones de sesión en vuelo. tmux, cc-app y el broker sin tocar.

Verificación: pancarta en el journal, `NRestarts=0`, 0 «rutas nativas desactivadas»; latencias
reales en 4777: `/notifs/count` 30 ms, `/notices?limit=20` 33 ms, `/work-marks` 16 ms,
`/active-tab` 5 ms, `/workspace` 1,2 ms, `/prefs` 0,4 ms; 7 rutas nativas idénticas al heredado;
conexiones del frente al heredado: 0–1 por segundo (solo `/state`, `/terminal-panes`,
`/usage/state`, `/pomodoro`, `/analytics/week`). Pss del frente 19,2 MiB al arrancar (el heredado,
688 MiB tras 4 h desde su arranque de las 12:04).

Reversión disponible: `COMANDOS_DASH_NATIVE=0` (env o drop-in) + reinicio, o
`~/.local/share/comandos/bin/comandos install --rollback-release` (vuelve a `17aa1bea2309`) + reinicio.

## 2c: dominios nativos II

Procedimiento para el controlador, como el de la 2b. El frente ya está en 4777 (release
`2bae7f9cd7d6`) y el Python heredado en 4781; la 2c solo cambia el binario del frente. Ni el
Python ni las unidades ni tmux se tocan.

### Qué cambia

El frente responde 12 rutas más. Detalle, líneas del Python y razones de lo que sigue reenviado
en `docs/superpowers/plans/2026-10-04-fase-2c-dominios-nativos-ii.md` («Mapa de rutas nativas»).

| Dominio | Rutas nativas |
|---|---|
| F · snippets, uso, Pomodoro, catálogos (8) | GET `/snippets`, `/pomodoro`, `/model-tiers`, `/sovereignty`; POST `/snippets`, `/snippets/update`, `/snippets/delete`, `/ui-log` |
| G · terminal (3) | POST `/terminal-panes` (salvo `action:"close"`), `/terminal-history`, `/pane/type` |
| H · operaciones (1) | GET `/model/status` |

Siguen en el Python: POST `/pomodoro` (despierta su scheduler en memoria), GET `/analytics/week`
(límites de proveedor en memoria, red), `/providers`, `/optimization/plans`, `/accounts`,
`/opencode/models`, `/session-profiles`, `/extension-usage`, POST `/terminal/quick`, `/state`,
`/usage/*` y todo lo no listado. Dentro de las nativas se reenvían: `/terminal-panes` con
`close`, `/model/status` sin registro en el journal o esperando confirmación, `/pane/type` cuando
un estado de `~/.claude/hooks/state` nombra la sesión, y cualquier escritura con el `flock` de
`snippets.json`/`ui-events.jsonl` tomado.

Bases nuevas, cada una con su hilo y su apagado propio (no global):

- `~/.claude/hooks/comandos-usage.sqlite` (la base de uso que usa `cc-dash`, que **no** honra
  `COMANDOS_USAGE_DB`): si un Python más nuevo la migra por encima de `user_version` 11, una
  línea `comandos dash: …comandos-usage.sqlite: tiene esquema N…; GET /pomodoro y GET /sovereignty
  se reenvían al heredado` y solo esas dos se reenvían hasta reiniciar.
- `~/.claude/hooks/session-operations.sqlite3`: si `session_operations` cambia de columnas, una
  línea análoga y solo GET `/model/status` se reenvía. El primer sondeo espera hasta 15 s si la
  base está ocupada (el `timeout=15` del Python), así un `SQLITE_BUSY` pasajero no apaga el
  carril.
- En los dos carriles, solo un esquema más nuevo o desconocido (o un pánico) apaga el carril. El
  primer sondeo de la base de uso espera hasta 10 s (el `timeout=10` de `bin/cc_usage.py`), y
  cualquier otro fallo al abrir (ocupada más allá de esa espera, permisos) reenvía esa petición
  y se reintenta en la siguiente, con una sola línea `…: apertura fallida (…); se reenvía esta
  petición de … y se reintenta en la siguiente`, que no es un apagado.
- app-state, respecto de la 2b: un fallo pasajero de la puerta (`SQLITE_BUSY` al leer
  `schema_migrations`) reenvía solo esa petición y el conjunto nativo sigue. Solo una migración
  desconocida, un esquema incompatible o un worker retirado lo apagan entero (la línea `rutas
  nativas desactivadas`). Un fallo al abrir app-state en la primera petición sigue apagándolo,
  como en la 2b (`StateBackend::open` ya reintenta tres veces ante `SQLITE_BUSY`).

`/model-tiers` lee `config/model-tiers.json` del checkout del heredado (`REPO_ROOT` del Python):
`COMANDOS_DASH_REPO` si está definida; si no, el destino canónico de
`~/.claude/hooks/dash/index.html` dos niveles arriba. Sin raíz o sin archivo legible, la ruta se
reenvía. Es la única ruta que usa esa raíz (`/sovereignty` solo lee `~/.claude/hooks`). Comprobar
en el paso 0 que es el mismo checkout que ejecuta `cc-dash-legacy.service`.

### Diferencias y comportamientos aceptados (2c)

- GET `/pomodoro` registra una vez por proceso la política de foco (idempotente) y puede crear la
  base de uso si faltara, como ya hacía el Python. GET `/model/status` marca como fallidas las
  operaciones de dueños muertos (`recover_abandoned`), como el Python: la vida del dueño es la
  misma llamada `kill(pid, 0)` y `updated`/`at` son el `double` de `time.time()`. Un dueño que el
  `os.kill` del Python no aceptaría (no entero, fuera de `pid_t`) hace que se reenvíe sin escribir.
- `/pane/type`: la caché de 256 respuestas por `requestId` es del frente; un `requestId` que el
  frente declinó se declina siempre, así un reintento nunca teclea dos veces. Caché y marcas viven
  en memoria: un reinicio del frente las pierde. El reintento de un `requestId` que tecleó el
  Python lo teclearía Rust solo si, tras el reinicio, ningún estado de hooks nombra ya esa sesión;
  el único llamador (la barra de comandos) no reintenta.
- Candados separados. El frente y el Python no comparten candado de proceso para `/pane/type`
  (candado por pane) ni para `/terminal-panes` (el `_LOCK` de `lib/terminal_panes.py` contra el
  `SERIAL` de Rust). cc-app llama a `/terminal-panes` por HTTP (`bin/cc-app:4365,4371,8843,8847`):
  antes lo serializaba el `_LOCK` del Python y ahora el `SERIAL` del frente, salvo el `close`, que
  se reenvía y toma el `_LOCK` del Python. Un `close` puede cruzarse, pues, con un `list`,
  `select`, `split` o `resize` nativo. Ninguno de los dos candados cubre a otros clientes de tmux
  (el usuario en su terminal, los hooks), y lo que protege a los panes está en tmux:
  `select`, `split` y `close` van en un `if-shell` que comprueba la identidad del pane (y, para
  `close`, que no sea el último) en la cola de tmux, y un `resize` de un pane ya cerrado falla
  con el 400 del Python. En `/pane/type` un pane se resuelve siempre al mismo lado mientras el
  estado de hooks no cambie; dos tecleos cruzados al mismo pane equivalen a dos clientes
  tecleando a la vez.
- Una escritura de snippets o de `ui-log` con el candado tomado se reenvía (el Python sí espera).

### 0. Previos

```sh
cd ~/codebase/0xJesus/ComandOS && git log -1 --oneline        # main con la Fase 2c fusionada
CARGO_TARGET_DIR=$PWD/.build/target nice -n 10 cargo build --release -p comandos-cli -j 6
CARGO_TARGET_DIR=$PWD/.build/target nice -n 10 cargo build -p xtask -j 6
NEW=$HOME/codebase/0xJesus/ComandOS/.build/target/release/comandos
XT=$HOME/codebase/0xJesus/ComandOS/.build/target/debug/xtask
grep -qa COMANDOS_DASH_REPO "$NEW" || echo "BINARIO SIN 2c: no seguir"
systemctl --user is-active cc-dash.service cc-dash-legacy.service    # active active
~/.local/share/comandos/bin/comandos install --releases              # anotar la actual ('*': 2bae7f9cd7d6)
readlink -f ~/.claude/hooks/dash/index.html                          # …/codebase/0xJesus/ComandOS/dash/index.html
systemctl --user cat cc-dash-legacy.service | grep ExecStart         # …%h/codebase/0xJesus/ComandOS/bin/cc-dash 4781 --no-open
git status --short | head; git diff --stat | tail -1                  # ver la nota de abajo
git diff -U0 -- bin/cc-dash bin/cc-app dash lib \
  | grep -E '^[+-].*(/snippets|/ui-log|/pomodoro|/model-tiers|/sovereignty|/terminal-panes|/terminal-history|/pane/type|/model/status)'
ss -ltn 'sport = :4782'                                               # libre
```

Las dos primeras rutas deben ser el mismo checkout: de él salen `config/model-tiers.json`, el
Python de 4781 y los estáticos que sirve el frente. Ese checkout tiene hoy cambios sin commit
(`bin/cc-dash`, `bin/cc-app`, `dash/*`, `lib/*`): el Python vivo y los estáticos salen de ese
árbol, no del oráculo commiteado contra el que se probó la paridad. A 4 de octubre el diff no
cambia ninguna de las 12 rutas ni el cuerpo que envían sus llamadores; la única línea que muestra
el `grep` es de `dash/session-controls.js` (borrado), que retira un llamador de `/model/status`.
Si el `grep` muestra otra línea, revisar esa ruta antes de seguir.

### 1. Sombra en 4782 con nativo, contra el heredado 4781

Igual que en la 2b: la sombra usa las bases reales (sus rutas nativas escriben lo mismo que el
tablero: snippets, `ui-events.jsonl`, `recover_abandoned`); nunca `xtask poll` contra 4777/4781/4782.

```sh
COMANDOS_DASH_TRACE_FORWARD=1 "$NEW" dash 4782 --legacy-port 4781 2> /tmp/sombra-2c.log
```

Desde otra terminal, en `~/codebase/0xJesus/ComandOS`:

```sh
"$XT" parity --fixture xtask/parity/frente.jsonl --hooks ~/.claude/hooks \
  --state-db ~/.local/state/comandos/app-state.sqlite3 \
  --usage-db ~/.claude/hooks/comandos-usage.sqlite --comandos "$NEW"        # 0 DIFF
"$XT" poll --shadow --minutes 10 --hooks ~/.claude/hooks \
  --state-db ~/.local/state/comandos/app-state.sqlite3 \
  --usage-db ~/.claude/hooks/comandos-usage.sqlite --comandos "$NEW"        # Pss plano, 0 no-2xx, mismos hilos min 1 y 10
for r in /snippets /model-tiers /pomodoro; do
  cmp -s <(curl -s 127.0.0.1:4782$r | sed 's/"serverNowMs": [0-9]*//') \
         <(curl -s 127.0.0.1:4781$r | sed 's/"serverNowMs": [0-9]*//') && echo "igual $r" || echo "DISTINTO $r"
done
S=$(tmux list-sessions -F '#{session_name}' | head -1)
for p in /terminal-panes /terminal-history; do
  B="{\"session\":\"$S\"}"
  cmp -s <(curl -s -H 'Content-Type: application/json' -d "$B" 127.0.0.1:4782$p) \
         <(curl -s -H 'Content-Type: application/json' -d "$B" 127.0.0.1:4781$p) && echo "igual $p" || echo "DISTINTO $p"
done                                    # el historial puede cambiar entre las dos lecturas: repetir
```

Navegación manual en `http://127.0.0.1:4782` con `chrome-bg` (vía `cc-browser-expose start 4782`):
crear, editar y borrar un snippet; abrir el Pomodoro; abrir «Soberanía». La sombra usa el tmux
real del usuario: para la terminal, abrir antes una pestaña de terminal nueva con el botón «+»
(un shell vacío) y hacer **solo en ella** el split, el redimensionado, la selección de otro pane
y el tecleo de un comando desde la barra de comandos (sin Enter), con esa pestaña como destino.
**Nunca en un pane de agente**: un split reflowa su TUI y el tecleo deja texto en su caja de
entrada. Al terminar, cerrar esa pestaña (con sus panes). Luego:

```sh
grep -c 'se reenvían al heredado\|rutas nativas desactivadas' /tmp/sombra-2c.log   # 0
grep 'reenvío' /tmp/sombra-2c.log | sed 's/.*reenvío //' | sort | uniq -c | sort -rn
```

No deben aparecer las rutas de «Qué cambia» salvo los casos reenviados descritos arriba. Ctrl+C.

### 2. Cutover

Antes de reiniciar, nada en vuelo: ni un cambio de cuenta o de modelo, ni un tecleo de la barra
de comandos. Un reinicio a mitad de un `/pane/type` corta el texto en el pane (igual que matar el
Python, pero evitable). El journal de operaciones se mira en solo lectura; una fila reciente en un
estado no final es un cambio en curso (las `awaiting_confirmation` y `recovery_required`
antiguas son residuo y no cuentan):

```sh
sqlite3 "file:$HOME/.claude/hooks/session-operations.sqlite3?mode=ro" \
  "SELECT id, state, datetime(updated, 'unixepoch', 'localtime') FROM session_operations
   WHERE state NOT IN ('confirmed', 'failed', 'rolled_back')
     AND updated > strftime('%s', 'now') - 900"                       # vacío
```

`/pane/type` no deja rastro (es nativo y la traza solo registra reenvíos): su único llamador es la
barra de comandos del tablero, así que la comprobación es no usarla durante el reinicio y que no
quede un envío suyo pendiente en pantalla.

```sh
"$NEW" install --stage
grep -qa COMANDOS_DASH_REPO "$(readlink -f ~/.local/share/comandos/bin/comandos)" || echo "STAGE MALO: ~/.local/share/comandos/bin/comandos install --rollback-release"
~/.local/share/comandos/bin/comandos hook claude-status >/dev/null && echo "hooks OK con la release nueva"
systemctl --user restart cc-dash.service
curl -s -o /dev/null -w '%{http_code}\n' http://127.0.0.1:4777/        # 200
systemctl --user show -p NRestarts --value cc-dash.service           # 0
# Abre el carril del journal (perezoso): sin registro declina y responde el Python.
curl -s '127.0.0.1:4777/model/status?operationKey=local' >/dev/null
for r in /snippets /model-tiers /pomodoro /sovereignty; do              # /pomodoro abre el carril de uso
  curl -s -o /dev/null -w "$r %{http_code} %{time_total}s\n" 127.0.0.1:4777$r
done                                                                  # 200 y < 0,1 s
journalctl --user -u cc-dash.service --since -2min --no-pager | grep -c 'se reenvían al heredado\|rutas nativas desactivadas'   # 0
```

El tablero queda sin respuesta unos 3 s; cc-app reintenta solo. Una línea `apertura fallida (…);
se reenvía esta petición` no es un apagado: el carril reintenta en la siguiente petición.

### 3. Verificación por dominio

```sh
systemctl --user set-environment COMANDOS_DASH_TRACE_FORWARD=1 && systemctl --user restart cc-dash.service
sleep 600
journalctl --user -u cc-dash.service --since -11min --no-pager | grep 'reenvío' \
  | sed 's/.*reenvío //' | sort | uniq -c | sort -rn
systemctl --user unset-environment COMANDOS_DASH_TRACE_FORWARD && systemctl --user restart cc-dash.service
```

- **F**: no aparecen `/snippets*`, `/ui-log`, GET `/pomodoro`, `/model-tiers`, `/sovereignty`. Un snippet creado en el tablero aparece en cc-app y viceversa; POST `/pomodoro` (iniciar/pausar) sí aparece.
- **G**: POST `/terminal-panes` solo aparece al cerrar un pane; `/terminal-history` y `/pane/type` no aparecen. Con dos iframes remotos abiertos, `/terminal-panes` cada 2 s no se acumula en la traza.
- **H**: GET `/model/status` solo aparece en un cambio de cuenta que espera confirmación o sin operación registrada; el resto del cambio se sigue desde Rust.
- `/state`, `/usage/state` y `/analytics/week` siguen apareciendo: es lo esperado.
- Memoria e hilos, con dos iframes remotos abiertos, al minuto 1 y al 10:
  `P=$(systemctl --user show -p MainPID --value cc-dash.service); grep Pss /proc/$P/smaps_rollup; ls /proc/$P/task | wc -l`.
  Pss plano (± 1 MiB) e hilos sin crecer entre las dos lecturas.

### 4. Reversión

Igual que en la 2b. A/B sin cambiar binario:

```sh
systemctl --user set-environment COMANDOS_DASH_NATIVE=0 && systemctl --user restart cc-dash.service
# deshacer: systemctl --user unset-environment COMANDOS_DASH_NATIVE && systemctl --user restart cc-dash.service
```

Lo anterior vive solo en la memoria del gestor de usuario (se pierde al reiniciar o reloguear).
Para que sobreviva, drop-in persistente:

```sh
mkdir -p ~/.config/systemd/user/cc-dash.service.d
cat > ~/.config/systemd/user/cc-dash.service.d/no-native.conf <<'EOF'
[Service]
Environment=COMANDOS_DASH_NATIVE=0
EOF
systemctl --user daemon-reload && systemctl --user restart cc-dash.service
systemctl --user show -p Environment cc-dash.service | grep -o 'COMANDOS_DASH_NATIVE=0'
```

Deshacer el drop-in:

```sh
rm ~/.config/systemd/user/cc-dash.service.d/no-native.conf
systemctl --user daemon-reload && systemctl --user restart cc-dash.service
```

Volver a la release anterior (`2bae7f9cd7d6`, la 2b):

```sh
~/.local/share/comandos/bin/comandos install --releases
~/.local/share/comandos/bin/comandos install --rollback-release
systemctl --user restart cc-dash.service
grep -qa COMANDOS_DASH_REPO "$(readlink -f ~/.local/share/comandos/bin/comandos)" && echo "SIGUE LA 2c"
```

Las escrituras nativas (snippets, `ui-events.jsonl`, journal, política de foco) están en los mismos
archivos y bases que usa el Python, con el mismo formato: revertir no necesita limpiar nada.
Revertir pierde la caché de `/pane/type` del frente (ver «Diferencias»).

### Medido antes del cutover (rama `migration/rust-fase2c`, `d33e520`; `xtask` de `5fbf1ac`)

Arnés en namespace de red privado, copia de `~/.claude/hooks`, copias de solo lectura de
`app-state.sqlite3` (`--state-db`) y de `comandos-usage.sqlite` (`--usage-db`), binario release de
la rama con los arreglos de la revisión final.

- Suite del workspace: 732 pruebas, 0 fallos, 1 ignorada (herramienta manual de RSS); `fmt` y
  `clippy -D warnings` limpios.
- `xtask parity` con nativo: 133 OK, 0 DIFF, 0 SKIP de 133. Reenviadas al heredado, 8 rutas
  distintas: GET `/state` (2), GET `/operator`, GET `/no-existe.css`, GET `/vendor/`,
  POST `/workspace/close-group` (las 5 de la 2b) y, por diseño, POST `/terminal-panes`
  (`g-terminal-panes-close`), GET `/model/status` (`h-model-status-sin-registro`) y GET `/pomodoro`
  (`f-pomodoro-consulta`, con consulta: llave `Raw`).
- `xtask poll --shadow --minutes 10` (con nativo). `/terminal-panes` va contra sesiones reales del
  tmux privado de la pila: `local` y `poll-b`, de tres panes cada una (inventario completo:
  `list-panes`, un `display-message` y una lectura de `/proc/<pid>/stat` por pane y
  `list-clients`, bajo `SERIAL`). El tablero lista `local` cada 2 s; un segundo iframe lista
  `poll-b` cada 2 s, redimensiona un pane de `local` cada 10 s y selecciona otro con
  `scope:"client"` cada 20 s. Resultado: 3390 peticiones (las 2910 del calendario más 480 de
  terminal), 0 errores de transporte, 0 no-2xx. Pss del frente 15,0 MiB al minuto 0 (15 337 KiB),
  21,2 MiB al minuto 1 (21 689 KiB) y 21,2 MiB al minuto 10 (21 669 KiB, −20 KiB); desde el
  minuto 5, 21 573–21 845 KiB (272 KiB de banda; la pendiente de mínimos cuadrados sobre esos
  seis puntos, 1947 KiB/h, la marca el pico de 21 845 KiB del minuto 8, y el minuto 10 queda
  por debajo): plano. Hilos del frente: 3 al minuto 0, 4 del minuto 1 al 10. Reenviadas, 3 rutas
  distintas: GET `/state` (500), GET `/usage/state` (60), GET `/analytics/week` (10); ningún
  `/terminal-panes` ni `/pomodoro`. El Python de la pila aislada pasó de 45 a 226 MiB en el mismo
  tiempo.
- Corrida anterior (`76d8e46`, `/terminal-panes` contra la sesión inexistente `poll`, solo el
  400 tras un `list-panes`): 2910 peticiones, 300 no-2xx, Pss 21 337 KiB al minuto 1 y
  21 425 KiB al minuto 10.

### Ejecutado (4 de octubre de 2026, 19:08, release `4200180ce8ab`)

- Paso 0: main en `0aa4ae1` (fusión FF de `migration/rust-fase2c`); unidades `active active`;
  release previa `2bae7f9cd7d6`; el `grep` del diff sucio solo mostró la línea esperada de
  `dash/session-controls.js`.
- Paso 1, sombra en 4782: paridad 133 OK, 0 DIFF, 0 SKIP (bases reales copiadas); la sombra
  respondió igual byte a byte que el Python vivo en `/snippets`, `/model-tiers`, `/pomodoro`,
  `/terminal-panes` y `/terminal-history`; `xtask poll --shadow` 10 min: 3390 peticiones, 0 errores,
  0 no-2xx, Pss del frente 18 065 → 18 197 KiB (min 1 → 10), 4 hilos constantes. Sin `chrome-bg`
  (servidor MCP desconectado): el recorrido manual se sustituyó por HTTP sobre la sombra (crear,
  editar y borrar un snippet, visto al instante por el Python y sin restos; `/sovereignty`;
  `/ui-log`). La parte de terminal del recorrido no se hizo: sin navegador no hay pestaña «+».
  0 apagados en el registro de la sombra.
- Paso 2: journal sin operaciones en vuelo, 0 tecleos ni cambios en los 3 min previos; stage,
  hooks OK, reinicio; tablero 200, `NRestarts=0`, las 4 rutas en 0,3–6 ms; 0 apagados.
- Paso 3, traza 10 min: reenviadas solo `/state` (781), `/usage/state` (72) y rutas de fases
  futuras (`/commands/catalog`, `/chains`, `/conf`, `/analytics/week`, `/ssh`, `/remote-state`,
  `/remote-qr.png`, `/providers`, `/models/latest`); ninguna de las 12 rutas de la 2c; 0 avisos.
- Memoria en vivo sin traza (reinicio 19:19): Pss 21 437 KiB (min 1), 22 841 (min 10),
  22 877 (min 20), 23 029 (min 30); hilos 3–7 (los de `spawn_blocking` van y vuelven). Con la
  traza activada el minuto 10 marcó 25 165 KiB: el registro de cada reenvío infla el montón.
  Crecimiento tras el calentamiento ≈ 0,5 MiB/h: se vuelve a medir al cerrar la 2d.

## 2d: `/state` nativo y terminal rápida

Procedimiento para el controlador, como el de la 2c. El frente está en 4777 y el Python heredado
en 4781; la 2d solo cambia el binario del frente. Ni el Python ni las unidades ni tmux se tocan.

La release se construye desde `main` **después** de fusionar la rama `migration/rust-fase2d`, así
que también lleva lo que entró en `main` mientras tanto y la rama no tiene: el arreglo del broker
de extensiones (`969c3a0`, `f95ac83`, `c87973f`), `/analytics/week?sidebar=1` y `/accounts` del
Python confirmados (`cf193ba`), las pruebas con tmux privado por `-S` (`accab21`) y la sección
«Ejecutado» del cutover de la 2c (`e032c64`, release `4200180ce8ab`). La fusión tiene su propio
procedimiento (abajo, «Fusión en `main`»). La paridad y la sombra del paso 1 se repiten con ese
binario, no con el de la rama.

### Qué cambia

| Ruta | Nativa | Sigue en el Python |
|---|---|---|
| GET `/state` (y `/state?…`) | sí | cuando algo es incierto (abajo); `/states` y `/state/…` |
| POST `/workspace/sort` | `restore` (2b) y `by` | `by` que no es cadena; `app-tabs.json` ilegible; `/state` que declina |
| POST `/terminal/quick` | `place: "sidebar"` | el resto (registra pestaña: candado de proceso del Python); sin `systemd-run` en el `PATH` del frente |
| POST `/workspace/close-group` | no | todo (`close_app_tab`) |

GET `/state` escribe `~/.claude/hooks/app-tab-models.json` como el Python, solo cuando la respuesta
es nativa. Se reenvía entero (sin escribir) si un `~/.claude/hooks/state/*.json` no se lee con
certeza, si `motor-results.json` está ilegible, si el registro de proveedores no valida, si el
contexto de sugerencias está vencido y el heredado no responde, o por los demás casos de la
decisión D2 del plan `docs/superpowers/plans/2026-10-04-fase-2d-state-nativo.md`. También se
reenvía siempre que el carril de la base de uso esté apagado (la línea `…; GET /pomodoro, GET
/sovereignty y GET /state se reenvían al heredado`) o que el frente no tenga raíz del checkout
(`COMANDOS_DASH_REPO` o el destino de `~/.claude/hooks/dash/index.html`, como en la 2c).

La terminal rápida de la barra nace dentro de un scope del gestor de **usuario**
(`systemd-run --user --scope --collect --quiet tmux new-session …`, el `scope_cmd` del Python).
Sin `systemd-run` en el `PATH` del frente se reenvía: un servidor tmux que naciera en el cgroup de
`cc-dash.service` moriría en el siguiente `restart` con todas las sesiones.

### Diferencias y comportamientos aceptados (2d)

- El rastreador de configuración (`StateTracker`) es del frente: tras reiniciar el frente, la
  primera observación de cada pane equivale a un reinicio del Python («la evidencia que cambió
  gana» empieza de cero). Un cálculo de `/state` que declina o falla no deja rastro en él; solo
  los que responden.
- `guard` y `latency` de las sugerencias los da el heredado (GET `/usage/guard`, GET
  `/usage/analytics?days=7`) cada 60 s y solo cuando alguna tarjeta los necesita; `routes` lo
  calcula el frente. Los dos procesos refrescan su contexto en instantes distintos. Cada
  subconsulta tiene 2 s de plazo. Solo las respuestas que el heredado da cuando la función lanza
  equivalen al `except` del Python (`{}` o tabla vacía): el 500 `Error interno del tablero`, el 504
  `Tiempo de espera agotado` y, en la analítica, el 400 de su `except ValueError`. Si el heredado
  no responde, responde otro status (401, 404, 502…) o un 200 que el frente no puede leer como el
  Python, el fallo se recuerda 5 s y durante ese tiempo `/state` se reenvía entero sin tocar tmux.
  Presupuesto de un refresco: las rutas van antes y en serie (E/S de cuentas y el sondeo del proxy,
  hasta 0,3 s) y luego las dos subconsultas en paralelo (hasta 2 s): **≈ 2,3 s** en el peor caso,
  una vez por minuto, dentro del vuelo que esperan todos los sondeos de ese momento (cc-notifyd usa
  `timeout=2`: ese sondeo puede perderse y lo repite). Medido en vivo contra 4781 (5 lecturas GET,
  4 de octubre, 23:50): `/usage/guard` 21–70 ms, `/usage/analytics?days=7` 143–310 ms, muy por
  debajo. Esta dependencia desaparece en la 2e, que porta `token_guard_report` y
  `experiment_analytics`.
- Un GET `/state` que declina (cualquier causa de D2) se recuerda 1,2 s, como un resultado: los
  sondeos de esa ventana se reenvían sin repetir el cómputo, así una incertidumbre persistente no
  multiplica las llamadas a tmux sobre el servidor de las sesiones vivas. Cada `Decline` deja en
  el journal, como mucho una vez por minuto, la línea `comandos dash: GET /state declina y se
  reenvía al heredado (fase: …; N más callados desde la línea anterior; …)`, con la fase del
  cómputo y nunca datos de los panes. Un 500 o un 504 no se recuerdan (el Python tampoco).
- `app-tab-models.json` tiene dos escritores hasta la 2e, y no es raro: cada GET `/usage/state`
  (reenviado; `dash/index.html` lo pide cada 10 s por tablero abierto, `tickUsage`) llama a
  `_pane_models_for_live_state` → `read_states_cached()` → `read_states` → `write_app_tab_models`
  (`bin/cc-dash:286-296`, `7176`), y `ensure_observed_configs` (`:315`) lo hace cada 60 s. Tras el
  cutover la caché del Python está fría casi siempre, así que cada una es un `read_states`
  completo, con su tmux. Las escrituras son atómicas con temporales distintos (sin corrupción) y
  gana la última. Cada proceso tiene su propio `StateTracker` y muestrea en instantes distintos:
  con evidencia en conflicto (la pantalla dice un modelo, el transcript otro) «la evidencia que
  cambió gana» puede resolver distinto en cada uno, y entonces `model`/`effort` de ese pane
  alternarían en el archivo cada ~10 s, y con ellos la barra de modelo de cc-app que lo lee. El
  paso 3 lo comprueba; si oscila, se revierte a la 2c y `/usage/state` (o la lectura de modelos)
  se lleva a la 2e.
- `motor-results.json` es el espejo en disco del `MOTOR_RESULT` del Python, que lo reescribe en
  cada cambio. Si una escritura suya falla (su `except: pass`), el frente ve el valor anterior
  hasta la siguiente.
- POST `/workspace/sort` con `by` lee `/state` una vez antes de sincronizar el workspace; el Python
  sincroniza primero (puede confirmar una revisión `auto`) y relee `/state` en cada reintento. La
  sincronización es idempotente y la hace el siguiente GET `/workspace`. Una excepción de
  `read_states_cached` dentro de ese `try` (el 400 con `str(exc)`) se reenvía en vez de reproducir
  su texto. Si `/state` declina después de que el frente haya sincronizado (registro ilegible), la
  sincronización ya pudo confirmar una revisión `auto`: es idempotente, el `sync` del Python que
  responde el reenvío es un no-op y el resultado coincide.
- Textos de error del Python que el frente no reproduce letra a letra:
  - `/terminal/quick`: una salida no UTF-8 de tmux tras reservar la carpeta responde
    `No se pudo abrir la terminal: UnicodeDecodeError` (el Python daría el texto del códec).
  - `/terminal/quick`: si `os.makedirs` falla, el Python nombra el componente que falló; el frente
    nombra la carpeta entera (`[Errno 13] Permission denied: '/…/Terminal/…'`).
  - `/terminal/quick`: en esos textos, una ruta con caracteres Unicode no imprimibles va sin
    escapar (el `repr` de Python los escaparía).
  - Un `OSError` al arrancar tmux sin `errno` (no ocurre al lanzar un ejecutable) sale como
    `OSError`; con `errno` es el `[Errno N] …: 'tmux'` del Python.
- `/terminal/quick` en la barra: tras reservar la carpeta ya no se reenvía nunca. Si el cliente se
  desconecta a mitad (también con el reclamo todavía en el worker), el reclamo, el lanzamiento y
  el cierre de la fila terminan igual (como el hilo del Python): la fila nunca queda en
  `launching` sin lanzador. Si el cierre falla, responde 500.

### Fusión en `main`

El checkout de `main` (`~/codebase/0xJesus/ComandOS`) tiene trabajo sin commit del usuario. La
fusión no lo toca salvo en un archivo: `lib/pane_snapshot.py` está modificado sin commit en `main`
con exactamente el contenido que trae la rama, y `git merge` aborta igual («Your local changes …
would be overwritten by merge») aunque sea idéntico. **Nunca `git stash`**: la pila es compartida
con los worktrees y otras sesiones. Solo si es idéntico se descarta la copia de trabajo y se
fusiona en seguida:

```sh
cd ~/codebase/0xJesus/ComandOS && git rev-parse --abbrev-ref HEAD    # main
cmp lib/pane_snapshot.py <(git show migration/rust-fase2d:lib/pane_snapshot.py) \
  && echo IDENTICO || echo "DISTINTO: parar, no fusionar"
```

Con `IDENTICO`, y solo entonces:

```sh
git checkout -- lib/pane_snapshot.py && git merge --no-ff migration/rust-fase2d
```

Con `DISTINTO`, parar: el usuario cambió el módulo después y hay que decidir con él qué versión
ejecuta el oráculo. Entre el `checkout` y la fusión el archivo es el de `HEAD`; el Python vivo ya
tiene el módulo cargado y solo un reinicio del heredado en esa ventana lo notaría.

La fusión tiene un conflicto esperado, solo en `docs/verification/cutover-dash.md` (comprobado con
`git merge-tree` entre `main` en `accab21` y la rama): `main` añadió «Ejecutado (4 de octubre de
2026, 19:08, release `4200180ce8ab`)» al final de la sección 2c y la rama añadió «## 2d» en el
mismo sitio. Se resuelve conservando las dos, la de `main` primero (cierra la 2c) y la sección 2d
detrás; ningún otro archivo choca. Tras resolverlo:

```sh
git add docs/verification/cutover-dash.md && git commit --no-edit
git status --short -- lib/pane_snapshot.py                          # vacío
```

### 0. Previos

```sh
cd ~/codebase/0xJesus/ComandOS && git log -1 --oneline        # main con la Fase 2d fusionada
readlink -f ~/.claude/hooks/dash/index.html                    # …/codebase/0xJesus/ComandOS/dash/index.html
CARGO_TARGET_DIR=$PWD/.build/target nice -n 10 cargo build --release -p comandos-cli -j 6
CARGO_TARGET_DIR=$PWD/.build/target nice -n 10 cargo build -p xtask -j 6
NEW=$HOME/codebase/0xJesus/ComandOS/.build/target/release/comandos
XT=$HOME/codebase/0xJesus/ComandOS/.build/target/debug/xtask
grep -qa 'GET /pomodoro, GET /sovereignty y GET /state' "$NEW" || echo "BINARIO SIN 2d: no seguir"
systemctl --user is-active cc-dash.service cc-dash-legacy.service    # active active
~/.local/share/comandos/bin/comandos install --releases              # anotar la actual ('*': 4200180ce8ab, la 2c)
for u in cc-dash.service cc-dash-legacy.service; do
  echo "== $u"
  systemctl --user show -p WorkingDirectory "$u"
  systemctl --user show -p Environment "$u" | tr ' ' '\n' \
    | grep -E '^(Environment=)?(PATH|COMANDOS_QUICK_TERMINAL_BASE|CODEX_HOME|GROK_HOME)='
done
ss -ltn 'sport = :4782'                                               # libre
git status --short | head -40                                         # ver la nota de abajo
git status --short -- lib/pane_snapshot.py lib/tui_state.py lib/providers.py lib/quick_terminal.py   # vacío
git diff -U0 -- dash bin/cc-app bin/cc-app-mac bin/cc-notifyd lib/operator_dispatch.py \
  | grep -E '^[+-].*(/state|/terminal/quick|/workspace/sort|quick-terminal|"place")'       # vacío
git diff -U0 -- bin/cc-dash \
  | grep -E 'def (read_states|read_states_cached|write_app_tab_models|workspace_sort|quick_terminal_request|_annotate_suggestion|_suggestion_context|observe_pane|reconcile_card_config)\b'   # vacío
```

`readlink` tiene que dar el `dash/index.html` del mismo checkout: de él sale la raíz del frente
(`config/model-tiers.json`, `lib/`), y sin raíz GET `/state` declina siempre (la sombra y el
cutover no probarían nada).

El `git status --short` completo **no** sale vacío y es lo esperado: `main` tiene trabajo sin
commit del usuario (a 5 de octubre, `M bin/cc-dash`, `M bin/cc-app`, `dash/*`, varios `lib/*` y
`tests/*`, y archivos sin seguimiento) que el Python vivo y los estáticos ya ejecutan. No se toca.
`M bin/cc-dash` es `/account/add` y la confianza de Codex: no cambia el camino de `/state`, de
`/workspace/sort` ni de `/terminal/quick`, y por eso el último `grep` sale vacío. El de los cuatro
módulos del oráculo sí tiene que salir vacío tras la fusión. Los dos `grep` buscan llamadores sin
commit de las tres rutas en `dash/` y en los clientes (`bin/cc-app`, `cc-app-mac`, `cc-notifyd`,
`operator_dispatch.py`) y funciones del Python que cambiarían lo que compara la paridad. Si alguno
muestra una línea, revisar esa ruta o esa función antes de seguir.

Las dos unidades deben tener el mismo `WorkingDirectory` y el mismo `PATH` (o ninguno de los dos
lo fija), y `COMANDOS_QUICK_TERMINAL_BASE`, `CODEX_HOME`, `GROK_HOME` iguales o ausentes. El
frente resuelve una vez al arrancar `systemd-run`, `cc-model-proxy` y los binarios de los agentes
con su `PATH`: una entrada vacía o relativa del `PATH` se resuelve contra el `WorkingDirectory`
de la unidad, así que dos unidades con el mismo `PATH` y distinto directorio ven binarios
distintos. Con el `PATH` y el directorio del frente, `systemd-run` tiene que existir:

```sh
P=$(systemctl --user show -p Environment cc-dash.service | tr ' ' '\n' | sed -n 's/^\(Environment=\)\{0,1\}PATH=//p')
D=$(systemctl --user show -p WorkingDirectory --value cc-dash.service)
(cd "${D:-$HOME}" && PATH="${P:-$PATH}" command -v systemd-run)     # una ruta absoluta
```

Sin ella, `/terminal/quick` de la barra se reenvía siempre (correcto, pero sin la 2d).

`lib/pane_snapshot.py` es la versión que ejecuta `cc-dash-legacy` (`codex --yolo` como
`--dangerously-bypass-approvals-and-sandbox`); un cambio ahí cambia las tarjetas de los panes de
Codex, por eso tiene que estar confirmado y sin diferencias.

### 1. Sombra en 4782 con nativo, contra el heredado 4781

La sombra usa los archivos reales: su GET `/state` escribe `app-tab-models.json` (lo mismo que
escribiría el Python) y su `/terminal/quick` abre una terminal de verdad (probarla solo a mano).
**Nunca** `xtask poll` contra la sombra, 4777 ni 4781: la carga va siempre con `--shadow`.

```sh
COMANDOS_DASH_TRACE_FORWARD=1 "$NEW" dash 4782 --legacy-port 4781 2> /tmp/sombra-2d.log
```

Desde otra terminal, en `~/codebase/0xJesus/ComandOS`:

```sh
"$XT" parity --fixture xtask/parity/frente.jsonl --hooks ~/.claude/hooks \
  --state-db ~/.local/state/comandos/app-state.sqlite3 \
  --usage-db ~/.claude/hooks/comandos-usage.sqlite --comandos "$NEW"        # 0 DIFF
"$XT" poll --shadow --minutes 10 --hooks ~/.claude/hooks \
  --state-db ~/.local/state/comandos/app-state.sqlite3 \
  --usage-db ~/.claude/hooks/comandos-usage.sqlite --comandos "$NEW"        # criterios de abajo
T=$(cat ~/.claude/hooks/dash-token)
norm() { python3 -c 'import json,sys
v=json.load(sys.stdin)
for i in v:
    o=i.get("observedConfig") or {}
    for k in ("observedAt","evidenceAt"):
        if k in o: o[k]="<volátil>"
print(json.dumps(v))'; }
for i in 1 2 3; do
  A=$(curl -s -H "X-Comandos-Token: $T" 127.0.0.1:4782/state | norm)
  B=$(curl -s -H "X-Comandos-Token: $T" 127.0.0.1:4781/state | norm)
  [ "$A" = "$B" ] && echo "igual /state" || echo "DISTINTO /state (repetir: un turno pudo cambiar entre lecturas)"
  sleep 2
done
for r in /state /prefs; do
  curl -s -o /dev/null -w "$r %{time_total}s\n" -H "X-Comandos-Token: $T" 127.0.0.1:4782$r
done
```

`norm` es una línea de inspección en la terminal del controlador, no código del repositorio. Las
primeras lecturas pueden diferir en `observedConfig` (el rastreador de la sombra empieza vacío,
ver «Diferencias»). A partir de la segunda vuelta tienen que coincidir salvo en un caso aceptado:
los panes ociosos cuya pantalla y transcript discrepan (cada proceso tiene su propio rastreador,
ver `app-tab-models.json` en «Diferencias»). Un `DISTINTO` se acepta solo si, repitiendo la
lectura, las diferencias se limitan a `model`, `effort`, `modelSource` y `observedConfig` de
panes así; se ven con

```sh
diff <(curl -s -H "X-Comandos-Token: $T" 127.0.0.1:4782/state | norm | python3 -m json.tool) \
     <(curl -s -H "X-Comandos-Token: $T" 127.0.0.1:4781/state | norm | python3 -m json.tool)
```

y en el pane de cada línea distinta se mira si la pantalla muestra otro modelo que el último del
transcript. Cualquier otra diferencia (estado, cuenta, sugerencia, orden, un pane de más o de
menos) que se repite en dos lecturas seguidas es un fallo: no seguir.

Navegación manual en `http://127.0.0.1:4782` con `chrome-bg` (vía `cc-browser-expose start 4782`):
el tablero muestra las mismas tarjetas que en 4777 (modelos, cuentas, sugerencias, estado
waiting/working de un turno en curso); «Ordenar» por nombre, recientes y pendientes, y Deshacer;
abrir una terminal rápida desde la barra de comandos dos veces seguidas con el mismo clic doble
(se abre una sola). Cerrarla al terminar. Luego:

```sh
grep -c 'se reenvían al heredado\|rutas nativas desactivadas' /tmp/sombra-2d.log   # 0
grep 'reenvío' /tmp/sombra-2d.log | sed 's/.*reenvío //' | sort | uniq -c | sort -rn
```

`GET /state` no debe aparecer (o solo de forma aislada, con un registro incierto en ese momento).
Ctrl+C.

### 2. Cutover

Antes de reiniciar, nada en vuelo: ni un cambio de cuenta o de modelo, ni un tecleo de la barra
de comandos, ni una terminal rápida abriéndose. Las dos bases se miran en solo lectura; una fila
reciente en un estado no final es algo en curso (las `awaiting_confirmation` y
`recovery_required` antiguas son residuo y no cuentan):

```sh
sqlite3 "file:$HOME/.claude/hooks/session-operations.sqlite3?mode=ro" \
  "SELECT id, state, datetime(updated, 'unixepoch', 'localtime') FROM session_operations
   WHERE state NOT IN ('confirmed', 'failed', 'rolled_back')
     AND updated > strftime('%s', 'now') - 900"                       # vacío
sqlite3 "file:$HOME/.local/state/comandos/app-state.sqlite3?mode=ro" \
  "SELECT request_id, datetime(updated_at, 'unixepoch', 'localtime') FROM quick_terminal_requests
   WHERE state = 'launching' AND lease_until > strftime('%s', 'now')"  # vacío
```

`/pane/type` no deja rastro: la comprobación es no usar la barra de comandos durante el reinicio y
que no quede un envío suyo pendiente en pantalla.

```sh
"$NEW" install --stage
grep -qa 'GET /pomodoro, GET /sovereignty y GET /state' "$(readlink -f ~/.local/share/comandos/bin/comandos)" || echo "STAGE MALO: ~/.local/share/comandos/bin/comandos install --rollback-release"
~/.local/share/comandos/bin/comandos hook claude-status >/dev/null && echo "hooks OK con la release nueva"
systemctl --user restart cc-dash.service
curl -s -o /dev/null -w '%{http_code}\n' http://127.0.0.1:4777/        # 200
systemctl --user show -p NRestarts --value cc-dash.service           # 0
curl -s -o /dev/null -w '/state %{http_code} %{time_total}s\n' \
  -H "X-Comandos-Token: $(cat ~/.claude/hooks/dash-token)" 127.0.0.1:4777/state   # 200
journalctl --user -u cc-dash.service --since -2min --no-pager | grep -c 'se reenvían al heredado\|rutas nativas desactivadas'   # 0
```

El tablero queda sin respuesta unos 3 s; cc-app y cc-notifyd reintentan solos. Una línea
`apertura fallida (…); se reenvía esta petición` no es un apagado: el carril reintenta en la
siguiente petición.

### 3. Verificación

```sh
systemctl --user set-environment COMANDOS_DASH_TRACE_FORWARD=1 && systemctl --user restart cc-dash.service
sleep 600
journalctl --user -u cc-dash.service --since -11min --no-pager | grep 'reenvío' \
  | sed 's/.*reenvío //' | sort | uniq -c | sort -rn
journalctl --user -u cc-dash.service --since -11min --no-pager \
  | grep -c 'GET /pomodoro, GET /sovereignty y GET /state se reenvían'   # 0: carril de uso vivo
systemctl --user unset-environment COMANDOS_DASH_TRACE_FORWARD && systemctl --user restart cc-dash.service
P=$(systemctl --user show -p MainPID --value cc-dash.service)
L=$(systemctl --user show -p MainPID --value cc-dash-legacy.service)
grep Pss /proc/$P/smaps_rollup; ls /proc/$P/task | wc -l; grep Pss /proc/$L/smaps_rollup   # al minuto 1 y al 10
```

- `GET /state` no aparece en la traza (o menos de 1 de cada 100 sondeos); `GET /usage/guard` y
  `GET /usage/analytics` aparecen como mucho una vez por minuto cada una. POST `/terminal/quick`
  solo aparece al abrir una terminal fuera de la barra (botón «+»).
- Pss del frente plano (± 1 MiB entre el minuto 1 y el 10) e hilos sin crecer tras el minuto 2;
  el heredado crece menos que antes del cutover en el mismo intervalo.
- Una pestaña nueva, un turno que pasa a «esperando» y un cambio de modelo se reflejan en el
  tablero y en cc-app en ≤ 3 s; las popups de cc-notifyd siguen llegando. Una terminal rápida de
  la barra se abre una vez y aparece en el tablero.
- Sin oscilación de modelos entre los dos escritores de `app-tab-models.json`: con el tablero
  abierto (que pide `/usage/state` cada 10 s), leer el archivo cada segundo durante 2 minutos y
  contar los valores distintos de `model`/`effort` de cada pane (`panes` de cada pestaña). Ningún
  pane puede alternar (A, B, A…); un único cambio coincidente con un cambio de modelo hecho a
  propósito es normal. Comprobación abajo.
- Si GET `/state` aparece en la traza en más de 1 de cada 100 sondeos, la causa está en el
  journal, con la fase del cómputo y como mucho una línea por minuto (comando abajo). Si no es
  pasajera (un registro que se está escribiendo), la salida es volver a la release de la 2c
  (paso 4, `install --rollback-release`), no `COMANDOS_DASH_NATIVE=0`, que apagaría también todo
  lo nativo de la 2b y la 2c.

```sh
journalctl --user -u cc-dash.service --since -15min --no-pager | grep 'GET /state declina'
```

Comprobación de oscilación de `app-tab-models.json` (2 min):

```sh
python3 -c 'import json,os,time
path=os.path.expanduser("~/.claude/hooks/app-tab-models.json")
seen={}
for _ in range(120):
    try: v=json.load(open(path))
    except (OSError,ValueError): v={}
    for session,info in (v.items() if isinstance(v,dict) else []):
        for p in (info.get("panes") or []) if isinstance(info,dict) else []:
            key=session+"|"+str(p.get("pane"))
            cur=(p.get("model"),p.get("effort"))
            h=seen.setdefault(key,[])
            if not h or h[-1]!=cur: h.append(cur)
    time.sleep(1)
for key,h in sorted(seen.items()):
    print(("OSCILA " if len(h)>2 else "ok ")+key,h)'
```

Es inspección en la terminal del controlador, no código del repositorio. Una línea `OSCILA`
(más de un cambio en 2 min sin tocar el modelo) es motivo para revertir a la 2c (paso 4).

### 4. Reversión

La salida preferida es volver a la release de la 2c (`4200180ce8ab`, la que instaló el cutover de
la 2c el 4 de octubre a las 19:08 y la que el paso 0 anota como actual): conserva todo lo nativo
de la 2b y la 2c y solo quita lo de la 2d. `install --rollback-release` intercambia el enlace
`bin/comandos` (→ `releases/<id>/comandos`) con la release que nombra `releases/previous`, que el
`--stage` del paso 2 dejó apuntando a la que había. Comprobarlo antes:

```sh
cat ~/.local/share/comandos/releases/previous                       # 4200180ce8ab
~/.local/share/comandos/bin/comandos install --rollback-release
systemctl --user restart cc-dash.service
readlink -f ~/.local/share/comandos/bin/comandos                    # …/releases/4200180ce8ab/comandos
grep -qa 'GET /pomodoro, GET /sovereignty y GET /state' "$(readlink -f ~/.local/share/comandos/bin/comandos)" && echo "SIGUE LA 2d"
```

Si `previous` no es `4200180ce8ab` (otro `--stage` entre medias), no usar `--rollback-release`:
apagar lo nativo con el A/B de abajo y decidir la release con el usuario.

`COMANDOS_DASH_NATIVE=0` apaga **todo** lo nativo (2b, 2c y 2d); es para un A/B rápido sin cambiar
binario o si la release anterior tampoco sirve:

```sh
systemctl --user set-environment COMANDOS_DASH_NATIVE=0 && systemctl --user restart cc-dash.service
# deshacer: systemctl --user unset-environment COMANDOS_DASH_NATIVE && systemctl --user restart cc-dash.service
```

Lo anterior vive solo en la memoria del gestor de usuario (se pierde al reiniciar o reloguear).
Para que sobreviva, drop-in persistente:

```sh
mkdir -p ~/.config/systemd/user/cc-dash.service.d
cat > ~/.config/systemd/user/cc-dash.service.d/no-native.conf <<'EOF'
[Service]
Environment=COMANDOS_DASH_NATIVE=0
EOF
systemctl --user daemon-reload && systemctl --user restart cc-dash.service
systemctl --user show -p Environment cc-dash.service | grep -o 'COMANDOS_DASH_NATIVE=0'
```

Deshacer el drop-in:

```sh
rm ~/.config/systemd/user/cc-dash.service.d/no-native.conf
systemctl --user daemon-reload && systemctl --user restart cc-dash.service
```

`app-tab-models.json`, las filas de `quick_terminal_requests` y el orden del workspace tienen el
mismo formato que escribe el Python: revertir no necesita limpiar nada.

### Medido antes del cutover (rama `migration/rust-fase2d`; binario de `1ccb496`, `xtask` de este commit)

Arnés en namespace de red privado, copias de `~/.claude/hooks`, `app-state.sqlite3`
(`--state-db`) y `comandos-usage.sqlite` (`--usage-db`), binario release de la rama con las
revisiones de las Tareas 5 y 6. Las dos partes de la pila leen el `/proc` real (no está
aislado) y, si `app-tabs.json` nombra sesiones `ssh-*`, ejecutan el `ssh -O check` real, que solo
consulta el socket de control (preflight R8). Las pruebas de la suite nunca llaman al `ssh` real.

- Suite del workspace: 821 pruebas, 0 fallos, 1 ignorada (herramienta manual de RSS); `fmt` y
  `clippy -D warnings` limpios.
- `xtask parity` con nativo: 139 OK, 0 DIFF, 0 SKIP de 139. Reenviadas al heredado, 8 rutas
  distintas, una vez cada una y todas por diseño: GET `/states`, `/operator`, `/no-existe.css`,
  `/vendor/`, `/model/status` (sin registro), `/pomodoro` (con consulta) y POST
  `/workspace/close-group`, `/terminal-panes` (`close`). GET `/state` ya no aparece.
- `xtask poll --shadow --minutes 10` con nativo. Además de las sesiones `cat` de la 2c, la pila
  crea dos agentes falsos en su tmux privado (`poll-claude` y `poll-codex`: `/bin/sleep` copiado
  con ese nombre, sesión de Claude en `~/.claude/sessions`, transcript con modelo y un registro de
  estado `working`/`waiting` cada uno), así GET `/state` recorre la observación completa
  (`display-message`, `capture-pane -S -45`, la pista de Codex de 2 s, cuentas, transcript) y el
  contexto de sugerencias contra el heredado. 3390 peticiones, 0 errores de transporte, 0 no-2xx.
  Pss del frente 17 865 KiB al minuto 0, 32 057 al minuto 1 y 34 665 al minuto 10; desde el
  minuto 5, 34 645–34 681 KiB (36 KiB de banda) y pendiente de 192 KiB/h. Hilos del frente: 3 al
  minuto 0, 4 al minuto 1 y 5 del minuto 2 al 10 (estable). Heredado de
  la pila: 161 420 KiB al minuto 1 y 184 658 al minuto 10 (+23 238 KiB; oscila con su recolector,
  máximo 230 718 al minuto 8). Reenviadas: solo GET `/usage/state` (60) y GET `/analytics/week`
  (10). Latencia de GET `/state` (500 sondeos): p50 112 ms, p95 207 ms, p99 253 ms.
- Lo mismo con `--no-native` (todo reenviado, la 2a): 3390 peticiones, 0 errores, 0 no-2xx;
  heredado 168 097 KiB al minuto 1 y 288 480 al minuto 10 (+120 383 KiB). Latencia de GET `/state`
  (500 sondeos, cada uno un `read_states_cached` del Python): p50 171 ms, p95 276 ms, p99 534 ms.
- Criterios: frente plano, sí (36 KiB ≤ 1 MiB y 192 ≤ 1024 KiB/h); heredado ≤ 50 % del
  crecimiento sin nativo, sí (23 238 / 120 383 = 19 %); p95 nativo ≤ p95 sin nativo, sí
  (207 ≤ 276 ms), y p99 nativo ≤ 1000 ms, sí (253 ms).
- Contexto de sugerencias en vivo (heredado de 4781, 5 lecturas GET): `/usage/guard` 21–70 ms y
  `/usage/analytics?days=7` 143–310 ms, contra el plazo de 2 s por subconsulta.
- Tras los arreglos de la revisión final de la rama (`6c52d79`: el `Decline` recordado 1,2 s con
  su línea en el journal, la terminal rápida segura ante cancelación en el reclamo, solo la
  excepción del heredado como `except` del contexto): suite del workspace 828 pruebas, 0 fallos,
  1 ignorada; `fmt` y `clippy -D warnings` limpios; `xtask parity` con nativo 139 OK, 0 DIFF,
  0 SKIP de 139, con las mismas 8 rutas reenviadas una vez cada una. La sombra y el `poll
  --shadow` se repiten en el paso 1 con el binario de `main` fusionado.

### Ejecutado — 5 de octubre de 2026, 00:43 (release `9f6fa07f26f0`, main `80d718a`)

- Fusión según «Fusión en `main`»: `lib/pane_snapshot.py` idéntico (`cmp`), `checkout --` y merge
  `a744e4e`; único conflicto el esperado en este documento.
- Paso 0 sin hallazgos (los dos `grep` vacíos; `systemd-run` en `/usr/bin` con el entorno de la
  unidad; ambas unidades con `WorkingDirectory=!/home/someguy` y sin `PATH` propio).
- Sombra en 4782: `xtask parity` 139 OK / 0 DIFF; GET `/state` 4782 vs 4781 igual salvo
  `observedConfig.source`/`modelSource` (`pane` ↔ `conversation`) en 8 panes de Codex ociosos con
  el mismo modelo y esfuerzo: el rastreador recién nacido termina su primera observación en
  `pane` (`StateTracker.observe`), aceptado. `/state` 0,34 s con 241 panes; 0 reenvíos en la traza.
- `xtask poll --shadow` 10 min: 3390 peticiones, 0 errores, Pss 28,8–28,9 MiB desde el minuto 5,
  5 hilos; `/state` p50/p95/p99 111/182/250 ms.
- Nada en vuelo (las dos consultas vacías); `install --stage`, centinela OK, hook OK; reinicio
  solo de `cc-dash.service` (00:43:30); 200, `NRestarts=0`, `/state` 200 en 0,33 s.
- Verificación: sin oscilación de `app-tab-models.json` (24 panes, 0 `OSCILA` en 2 min); traza de
  10 min: **GET `/state` no aparece**; reenviadas `/usage/state` (112), `/commands/catalog` (19),
  `/chains` (19), `/analytics/week` (10), todas de fases futuras; 0 líneas de carril apagado,
  0 `GET /state declina`. CPU del heredado ≈ 19 ticks/s tras el cutover.
- **Pendiente de ligereza**: con la carga real el frente sube a 24 hilos (pool de bloqueo de tokio
  sin tope) y a ≈ 180–198 MiB de Pss (meseta, sin fuga), frente a ≈ 23 MiB en la 2c. Se corrige en
  la rama `migration/rust-ligero` antes de la 2e.

## Ligereza tras la 2d

### Lo que se vio en vivo (release `9f6fa07f26f0`, solo lectura de `/proc`)

Tras el cutover de la 2d el frente pasó de ~23 MiB y 3–7 hilos (release `4200180ce8ab`) a 44,7 MiB
de Pss al minuto 1 y 197,7 MiB al minuto 10, plano después (Pss_Anon ~193 MiB), con 24 hilos y
≈ 14 ticks/s de CPU. Una segunda vida del mismo binario repitió el patrón: 7 hilos y 60–72 MiB
durante 4 min, y en un mismo instante (284,5 s tras el arranque) nacieron 17 hilos
`tokio-rt-worker` de golpe; en los 90 s siguientes el Pss subió 72 → 110 → 153 → 183 → 216 MiB y
los hilos quedaron en 24 (a los 41 min: 26 hilos, 22 del pool de bloqueo, 256 MiB).

### Causa

- El runtime es `new_current_thread()` con el pool de bloqueo por defecto de tokio (512 hilos,
  10 s de ocio). Una ola de peticiones simultáneas que ocupa un hilo de bloqueo cada una crea
  ~17 hilos. La ola real: `cc-app` vuelve al frente y `visibilitychange` despierta todos los
  iframes de `term.html`, cada uno con POST `/terminal-panes` (que espera a tmux en su hilo de
  bloqueo) y GET `/tab-models`.
- Antes de la 2d esos hilos se retiraban tras 10 s ociosos. Desde la 2d, GET `/state` da cientos
  de saltos de bloqueo por cómputo (escaneo y, por pane con agente, cuenta, Grok y evidencia de
  `/proc`), varias veces por segundo. El pool despierta a sus hilos ociosos por turno, así que
  ninguno llega a 10 s de ocio y la recolección se reparte entre todos.
- Cada hilo tiene su arena de glibc y la recolección la engorda: el Pss crece con el número de
  hilos, no con el tiempo. Prueba: con el binario de la 2d y `MALLOC_ARENA_MAX=2`, la misma carga
  deja 22 hilos pero 38 MiB (contra 79–81 MiB sin la variable).
- El vuelo único de `/state` (1,2 s) funciona: la concurrencia no viene de `/state`.

### Arreglo (rama `migration/rust-ligero`)

- `dash::runtime()`: `max_blocking_threads(8)` y `thread_keep_alive(2 s)`.
- `states::serial`: los saltos de bloqueo de `/state` (escaneo; cuenta, Grok y evidencia por
  pane; las rutas del contexto de sugerencias) van a un hilo propio (un `BackendWorker<()>` que
  se reemplaza si un trabajo entra en pánico), cada uno con un plazo de 10 s que, vencido,
  declina (falla cerrado: responde el heredado) en vez de colgar hasta los 120 s del manejador.
  Solo la escritura de `app-tab-models.json` sigue en el pool, para completarse aunque el
  cliente se vaya.
- `terminal::PANES_GATE`: un semáforo asíncrono de 1 antes del `spawn_blocking` de
  POST `/terminal-panes`. La librería ya serializa todo el proceso con su `SERIAL` (el `RLock`
  del Python, `terminal_panes.rs`), pero lo toma dentro del hilo de bloqueo: sin la puerta, una
  ola de 17 iframes llenaba el pool de hilos aparcados en ese candado y los estáticos, el
  historial y el resto de saltos esperaban detrás (con tmux lento, 5 s × 17 seguidos). Con la
  puerta la ola ocupa un hilo como mucho; el permiso viaja con el trabajo, así que un cliente
  que se va no abre la puerta antes de que el hilo termine.
- Con eso ningún trabajo del pool espera a otro del pool (los demás candados que se toman
  dentro son `try_lock` de archivo), así que el tope no puede interbloquear: lo que no cabe
  espera en la cola. Ocho deja sitio a varios tecleos largos de `/pane/type` sin frenar los
  estáticos. La primera versión (tope 4 y sin puerta) sí podía congelar el pool detrás de una
  ola con tmux lento; ver la tabla.
- `MALLOC_ARENA_MAX=2` en un drop-in de `cc-dash.service` también contiene la memoria (no los
  hilos) sin tocar código. Queda documentado como opción operativa; no se aplica.

### Carga para medirlo

`xtask poll --shadow` acepta `--agents N` (agentes falsos alternando Claude y Codex),
`--extra-panes N` (ventanas `cat` repartidas), `--pollers N` (GET `/state` extra a 1 Hz),
`--burst N` (cada 60 s, N iframes de terminal que refrescan a la vez) y los criterios
`--slow-tmux-ms N` (cada `tmux list-panes` de la pila tarda N ms) y los criterios
`--max-threads`, `--max-pss-mib` y `--max-static-p95-ms` (la orden falla si alguna muestra los
supera, o si el p95 de los estáticos pedidos durante la ola lo supera):

```sh
"$XT" poll --shadow --minutes 10 --hooks ~/.claude/hooks \
  --state-db ~/.local/state/comandos/app-state.sqlite3 \
  --usage-db ~/.claude/hooks/comandos-usage.sqlite --comandos "$NEW" \
  --agents 48 --extra-panes 190 --pollers 3 --burst 17 \
  --max-threads 12 --max-pss-mib 48 --max-static-p95-ms 100
# lo mismo con tmux lento: añadir --slow-tmux-ms 1500
```

### Antes y después (`poll --shadow`, 10 min, 5 de octubre de 2026)

Carga realista: `--agents 48 --extra-panes 190 --pollers 3 --burst 17` (≈ 244 panes, 48 agentes
observados, `/state` del tablero, de `cc-app` y tres sondeadores a 1 Hz, y cada 60 s una ola de
17 iframes de terminal). «Antes» es el binario de `80d718a` (la release `9f6fa07f26f0`); latencias
en ms (p50/p95/p99; el p50 de GET `/state` baja a 1–2 ms cuando la mayoría sale de la caché).

| Binario (carga realista) | Pss min 1 → 10 (KiB) | Hilos | GET `/state` | `/state` 1 Hz | `/terminal-panes` (ola) |
|---|---|---|---|---|---|
| Antes | 61 504 → 79 284 | 21 | 573/726/845 | 2/650/780 | 119/206/229 |
| Antes, `MALLOC_ARENA_MAX=2` | 36 916 → 38 500 | 21–22 | — | — | — |
| Solo tope 4 | 44 904 → 45 757 | 7 | 2/701/761 | 2/640/777 | 125/218/244 |
| Tope 4 + hilo de `/state` | 37 701 → 38 477 | 5 | 510/628/780 | 2/599/749 | 131/210/228 |
| Antes (máquina cargada, carga 13) | 70 729 → 80 973 | 21–22 | 1/883/1245 | 1/917/1465 | 131/480/755 |
| Primera versión (misma máquina cargada) | 37 921 → 35 893 | 5–6 | 1/901/1295 | 1/978/1540 | 126/364/426 |

Sin la ola (`--burst 0`) el binario de antes se queda en 5 hilos y 33–36 MiB: la ola dispara los
hilos y la recolección de `/state` los mantiene vivos. Con tope 2 y 3 (sin el hilo de `/state`):
40 652 KiB y 5 hilos, 42 848 KiB y 6 hilos. El par en máquina cargada (durante un reinicio del
controlador) tuvo 167 y 172 no-2xx en los dos binarios por igual; las demás corridas, 0.

Carga estándar de la 2d (sin opciones nuevas): antes 32 177 → 34 657 KiB, 5 hilos, GET `/state`
127/281/545; primera versión 32 381 → 32 745 KiB, 5–6 hilos, 127/289/724 (p99 con carga de máquina 13).

Tras la revisión (tope 8, `PANES_GATE`, plazo de 10 s en `serial`), misma carga realista, más
el caso de tmux lento (`--slow-tmux-ms 1500`). «Estáticos» es GET `/term.html` cada 100 ms
mientras dura cada ola (100 muestras):

| Binario | tmux | Pss min 1 → 10 (KiB) | Hilos | GET `/state` | `/state` 1 Hz | Estáticos en la ola |
|---|---|---|---|---|---|---|
| Final | normal | 33 833 → 33 697 | 5–6 | 539/701/835 | 2/672/870 | 0/5/7 |
| Antes | lento | 62 694 → 88 127 | 23–24 | 340/2195/2319 | 0/2127/2298 | 0/4/7 |
| Primera versión (tope 4, sin puerta) | lento | 37 969 → 38 767 | 6 | 114/2234/22 937 | 0/2097/18 248 | 0/22 310/23 347 |
| Final | lento | 33 601 → 34 011 | 5–6 | 300/2185/2463 | 0/2113/2271 | 0/3/5 |

La primera versión confirma el hallazgo de la revisión: con tmux lento la ola llenaba el pool de
4 hilos aparcados en el `SERIAL` y los estáticos y `/state` esperaban hasta 23 s. Con la puerta,
los estáticos quedan en 5 ms de p95 y el frente en 6 hilos. Con tmux lento, una ola de 17
`/terminal-panes` serializados (≈ 1,5 s cada uno) pasa de los 30 s de plazo del cliente de
`xtask`: 2 errores de transporte en el final, 22 en el de antes y 6 en la primera versión; es la
serialización del `RLock` del Python, igual en los tres.

- Suite del workspace: 846 pruebas, 0 fallos, 1 ignorada; `fmt` y `clippy -D warnings` limpios.
  `tests/dash_terminal_wave.rs` comprueba la puerta: sin ella la ola de 8 con tmux lento usa
  8 hilos de bloqueo; con ella, ≤ 2, y el estático responde en < 250 ms durante la ola.
- `xtask parity` con nativo: 139 OK, 0 DIFF, 0 SKIP de 139, las mismas 8 rutas reenviadas.
- Criterio para el cutover de este arreglo: `poll --shadow` con la carga realista y
  `--max-threads 12 --max-pss-mib 48 --max-static-p95-ms 100` sale 0 (con y sin
  `--slow-tmux-ms 1500`), y en vivo, tras una vuelta de `cc-app` al frente, el frente sigue en
  ≤ 12 hilos (principal, 2 `comandos-handler`, el de `/state` y hasta 8 del pool).

### Ejecutado — 5 de octubre de 2026, 03:02 (release `790e58866107`, main `81e8e14`)

- Fusión de `migration/rust-ligero` (`81e8e14`); fmt, clippy, pruebas de server/cli/xtask (318) y
  `xtask parity` 139 OK / 0 DIFF con el binario de `main`.
- Carga realista aislada 10 min (`--agents 48 --extra-panes 190 --pollers 3 --burst 17` con
  `--max-threads 12 --max-pss-mib 48 --max-static-p95-ms 100`): sale 0; `/state` p95 802 ms,
  estáticos durante la ola p95 6 ms.
- Nada en vuelo; `install --stage` (la release instalada es idéntica, `cmp`, al binario
  compilado; centinela 2d presente); hook OK; reinicio solo de `cc-dash.service`.
- En vivo antes: 270 MiB de Pss y 26 hilos. Después, 15 min de muestras: **40–42 MiB y 4–5
  hilos, planos**, `NRestarts=0`, 0 líneas de pánico/error/«GET /state declina». `/state` 200 en
  ≈ 10 ms (231 panes) con la caché caliente.
- Reversión: `install --rollback-release` (→ `9f6fa07f26f0`) + reinicio de la unidad.

## 2e: uso y analítica nativos

Procedimiento para el controlador, como el de la 2d. El frente está en 4777 y el Python heredado
en 4781; la 2e cambia el binario del frente y añade un drop-in de entorno a `cc-dash.service`.
Ni el Python, ni su unidad, ni el `ExecStart` del frente, ni tmux se tocan.

### Qué cambia

| Ruta | Nativa | Sigue en el Python |
|---|---|---|
| GET `/usage/state` | sí, con sus efectos | solo con la base de uso apagada para el frente (más nueva, ilegible) |
| GET `/analytics/week` | sí | `?sidebar` si el Python commiteado no lo soporta |
| GET `/accounts`, `/providers`, `/optimization/plans` | sí | registro de proveedores que no valida con certeza |
| GET `/extension-usage` | sí | — |
| GET y POST `/session-profiles`, POST `/session-profile-apply`, GET `/opencode/models` | no | todo |

`/state` ya no pide `/usage/guard` ni `/usage/analytics` al heredado: la guardia y la latencia de
las sugerencias las calcula el frente.

**Dueño único.** Desde el cutover, el frente hace lo que el `/usage/state` del Python hacía como
efecto: importar transcripts de Claude, Codex, Grok y OpenCode cada ≥ 60 s (la primera vez 75 s
después de arrancar), escribir los bordes `@ccmodel` de tmux y `~/.claude/hooks/pane-models.txt`,
avisar al pasar un pane a un modelo de nivel alto y registrar los paneles vivos en la base. El
Python deja de hacerlo porque ya no recibe `/usage/state`. Si el frente apaga su carril de uso
(una base más nueva) o se le retira el de importación (un fallo interno: una línea
`… GET /usage/state y su importación de uso se reenvían al heredado` en el journal), reenvía
`/usage/state` y el Python vuelve a hacerlo todo. Dos frentes sobre
el mismo HOME no importan a la vez (`~/.claude/hooks/comandos-usage-import.lock`).

**Límites al arrancar.** Con `/usage/state` nativo, el frente lee los límites de proveedor una vez
al arrancar (una petición OAuth por cuenta de Claude), para que la barra lateral y la guardia de
`/state` tengan los % de cuota sin esperar al primer `/usage/state`. Después refresca con su
caché (60 s; 180 s tras un error de la cuenta `main`) cuando alguien pide límites.

**Memoria del frente.** La ruta nativa reconstruye el memo de `/usage/state` tras cada importación.
Con glibc por defecto, cada liberación de un bloque grande sube el umbral dinámico de `mmap` y las
arenas de los hilos dejan de devolver memoria: bajo la carga realista el frente se queda en
≈ 61 MiB. Con `GLIBC_TUNABLES=glibc.malloc.mmap_threshold=131072:glibc.malloc.arena_max=2` (umbral
fijo de 128 KiB, dos arenas) baja a ≈ 45 MiB. Va en un drop-in de `cc-dash.service`: es
configuración reversible, sin dependencia nueva. Sin el drop-in todo funciona igual, con más
memoria. El valor vive en `comandos_core::malloc_tuning::PRODUCTION_GLIBC_TUNABLES`; `xtask poll
--shadow` y `xtask parity` se lo dan al frente que miden (`--glibc-tunables ''` para medir sin él).
Ningún hijo del frente lo hereda: tmux, `systemd-run --scope … tmux new-session` de la terminal
rápida, `ssh`, `git` y `fc-list` salen todos de `Program::command`, que lo quita
(`CHILD_ENV_REMOVE`; prueba gemela en `crates/comandos-cli/tests/dash_boot.rs`).

### Diferencias y comportamientos aceptados (2e)

- Límites de proveedor: el frente tiene su propia caché (60 s; 180 s tras un error de la cuenta
  `main`) y el Python conserva su bucle de 5 min hasta la 2g. Antes compartían una caché de 60 s;
  ahora son dos: ≈ +20 % de llamadas a `api.anthropic.com` en régimen (60 s del frente + 300 s del
  Python) y hasta ≈ 2× en arranques y tras errores, cuando los dos piden a la vez. Se vigila como
  ≈ 2×. Las fotos de cuota convergen (solo gana la más nueva). El ritmo del frente es el del
  Python antes del cutover: `usage_provider_limits()` refresca con el mismo TTL de 60 s (180 s
  tras un error de `main`) cada vez que una ruta pide límites, y el tablero los pide en cada
  sondeo de `/usage/state`; los 300 s son solo el bucle de fondo, que sigue en el Python. Mismas
  cuentas (`~/.claude` y cada `~/.claude-accounts/<alias>` con `.credentials.json`).
- `credential_health.*.error` de un fallo de TLS o de un cuerpo que no es JSON tiene el texto del
  frente, no el de `urllib`.
- GET `/accounts` no espera la primera lectura de límites: tras un reinicio cuya primera petición
  OAuth da 429, el menú de cuentas sale sin límites hasta 180 s (el TTL de error), y luego con
  ellos.
- La guardia de `/state` sí espera la primera lectura (≤ 2 s por vuelta). Si esa lectura no puede
  hacerse nunca porque falla antes de la red (credenciales ilegibles, `AbortRefresh`), el contexto
  declina siempre y GET `/state` se reenvía al heredado: falla cerrado, se ve en la traza del
  paso 3.
- La sombra del paso 1 corre con `--no-usage-effects`: nunca carga límites, así que su contexto
  de sugerencias declina siempre y su GET `/state` se reenvía. La guardia nativa solo se comprueba
  con efectos encendidos, en el paso 3.
- La latencia medida de las sugerencias (`experiment_analytics(USAGE_DB, 7)`) incluye
  `_paired_experiment_analytics`, que lee todos los experimentos sin ventana de tiempo, como el
  Python.
- Si `/state` es incierto en el momento de un `/usage/state`, esa vuelta no escribe bordes (la
  siguiente, ≤ 10 s después, sí) y la importación de ese ciclo no registra configuraciones
  observadas (el ciclo siguiente sí).
- Tras reiniciar el frente, la deduplicación de avisos de nivel empieza de cero (como tras
  reiniciar el Python).
- Una línea `reenvío GET /usage/state` en la traza es un ciclo en que el Python fue dueño: el
  frente declinó (carril de uso caído, base más nueva) y el Python importó, escribió bordes y
  pudo avisar. Una línea aislada es aceptable; repetidas son motivo para revertir (paso 4). En ese
  ciclo puede llegar un aviso de nivel duplicado: el `_TIER_LAST` del Python conserva el nivel que
  vio antes del cutover y no sabe de los avisos del frente. También los bordes pueden quedar mal
  unos 10 s: el `_pane_model_state` del Python (`applied` y el texto de `pane-models.txt`) es el
  de antes del cutover, así que puede saltarse un `set-option` que cree ya aplicado o reescribir
  el archivo con su versión vieja. La vuelta siguiente del frente (≤ 10 s) vuelve a leer las
  opciones de tmux y reescribe el archivo.
- Un archivo de transcript que sale del corte de 21 días y vuelve sin cambiar se relee (el
  Python lo recordaba); la base queda igual.
- `/proc/<pid>/environ` del frente muestra `GLIBC_TUNABLES` partido en dos líneas: glibc 2.35
  convierte cada `:` de la cadena en NUL al leerla. Las dos opciones se aplican; el valor completo
  está en `systemctl --user show -p Environment cc-dash.service`.

### 0. Previos

```sh
cd ~/codebase/0xJesus/ComandOS && git log -1 --oneline        # main con la Fase 2e fusionada
CARGO_TARGET_DIR=$PWD/.build/target nice -n 10 cargo build --release -p comandos-cli -j 6
CARGO_TARGET_DIR=$PWD/.build/target nice -n 10 cargo build -p xtask -j 6
NEW=$HOME/codebase/0xJesus/ComandOS/.build/target/release/comandos
XT=$HOME/codebase/0xJesus/ComandOS/.build/target/debug/xtask
TUN='glibc.malloc.mmap_threshold=131072:glibc.malloc.arena_max=2'
grep -qa 'GET /extension-usage y GET /usage/state' "$NEW" || echo "BINARIO SIN 2e: no seguir"
grep -qa 'GLIBC_TUNABLES' "$NEW" || echo "BINARIO QUE NO QUITA GLIBC_TUNABLES A SUS HIJOS: no seguir"
ldd --version | head -1                                              # glibc ≥ 2.26 (aquí 2.35)
systemctl --user is-active cc-dash.service cc-dash-legacy.service    # active active
~/.local/share/comandos/bin/comandos install --releases              # la actual ('*') es 790e58866107
systemctl --user cat cc-dash.service | grep ExecStart                # %h/.local/bin/cc-dash 4777 --no-open
readlink -f ~/.local/bin/cc-dash                                     # …/releases/790e58866107/comandos
ls ~/.config/systemd/user/cc-dash.service.d/ 2>/dev/null             # sin no-native.conf ni malloc.conf
for u in cc-dash.service cc-dash-legacy.service; do
  systemctl --user show -p Environment -p WorkingDirectory "$u"; done # mismo PATH, WorkingDirectory y TZ;
                                                                      # COMANDOS_*USAGE*, *_TOKEN_LIMIT,
                                                                      # COMANDOS_CLAUDE_PROJECTS_DIR,
                                                                      # COMANDOS_OPENCODE_DB, LANG iguales o ausentes;
                                                                      # ninguna con GLIBC_TUNABLES ni MALLOC_*
ss -ltn 'sport = :4782'                                               # libre
```

La release actual tiene que ser `790e58866107` (la 2d con la ligereza, 42 MiB y 5 hilos en vivo):
es la que la reversión del paso 4 restaura. Hoy `releases/previous` apunta a `9f6fa07f26f0`, la
2d anterior a la ligereza (270 MiB en vivo); el `--stage` del paso 2 lo sobrescribe con
`790e58866107`. Si la actual no es `790e58866107`, parar y decidir con el usuario.

### 1. Sombra en 4782 con nativo, contra el heredado 4781

La sombra arranca **sin efectos de uso**: no importa, no escribe bordes ni avisa (el dueño sigue
siendo el Python, que recibe el `/usage/state` de producción). Lleva el ajuste de malloc, como
producción.

```sh
GLIBC_TUNABLES="${TUN:?}" COMANDOS_DASH_TRACE_FORWARD=1 \
  "$NEW" dash 4782 --legacy-port 4781 --no-usage-effects 2> /tmp/sombra-2e.log
```

Desde otra terminal, en `~/codebase/0xJesus/ComandOS`:

```sh
"$XT" parity --fixture xtask/parity/frente.jsonl --hooks ~/.claude/hooks \
  --state-db ~/.local/state/comandos/app-state.sqlite3 \
  --usage-db ~/.claude/hooks/comandos-usage.sqlite --comandos "$NEW"        # 0 DIFF
T=$(cat ~/.claude/hooks/dash-token)
for r in /providers /optimization/plans '/accounts?harness=claude' '/analytics/week?offset=-1'; do
  A=$(curl -s -H "X-Comandos-Token: $T" "127.0.0.1:4782$r"); B=$(curl -s -H "X-Comandos-Token: $T" "127.0.0.1:4781$r")
  [ "$A" = "$B" ] && echo "igual $r" || echo "DISTINTO $r (límites: repetir tras 60 s)"
done
for r in /usage/state /analytics/week; do
  curl -s -o /dev/null -w "$r %{time_total}s\n" -H "X-Comandos-Token: $T" "127.0.0.1:4782$r"
done
```

`/usage/state` y `/analytics/week` de esta semana no se comparan a mano: dependen de cuándo cada
proceso refrescó límites e importó; los compara el arnés con sus campos de tiempo enmascarados.
Navegación manual en `http://127.0.0.1:4782` con `chrome-bg` (vía `cc-browser-expose start 4782`):
tarjetas de uso por proveedor, Analytics (Cuentas, Comparar, Pomodoro), la columna de límites de
la barra lateral, el botón «Cuenta» de un pane y el selector de motor (proveedores y planes).
Luego:

```sh
grep -c 'se reenvían al heredado\|rutas nativas desactivadas' /tmp/sombra-2e.log   # 0
grep 'reenvío' /tmp/sombra-2e.log | sed 's/.*reenvío //' | sort | uniq -c | sort -rn
```

Ni `GET /usage/state`, ni `GET /analytics/week`, ni `GET /accounts` deben aparecer. `GET /state`
sí puede aparecer: la sombra no carga límites (ver «Diferencias»). Ctrl+C.

### 2. Cutover

Antes de reiniciar, nada en vuelo: ni un cambio de cuenta o de modelo, ni un tecleo de la barra
de comandos, ni una terminal rápida abriéndose. Las dos bases se miran en solo lectura; una fila
reciente en un estado no final es algo en curso (las `awaiting_confirmation` y
`recovery_required` antiguas son residuo y no cuentan):

```sh
sqlite3 "file:$HOME/.claude/hooks/session-operations.sqlite3?mode=ro" \
  "SELECT id, state, datetime(updated, 'unixepoch', 'localtime') FROM session_operations
   WHERE state NOT IN ('confirmed', 'failed', 'rolled_back')
     AND updated > strftime('%s', 'now') - 900"                       # vacío
sqlite3 "file:$HOME/.local/state/comandos/app-state.sqlite3?mode=ro" \
  "SELECT request_id, datetime(updated_at, 'unixepoch', 'localtime') FROM quick_terminal_requests
   WHERE state = 'launching' AND lease_until > strftime('%s', 'now')"  # vacío
```

Línea base antes del reinicio (el paso 3 la compara):

```sh
sqlite3 "file:$HOME/.claude/hooks/comandos-usage.sqlite?mode=ro" \
  'select max(turn_finished_at), count(*) from usage_turns' | tee /tmp/base-2e-turnos.txt
L=$(systemctl --user show -p MainPID --value cc-dash-legacy.service)
awk '/^Pss:/{print $2}' /proc/$L/smaps_rollup | tee /tmp/base-2e-heredado.txt   # Pss del heredado (KiB)
```

Primero la release. `--stage` deja `releases/previous` en la release que había (`790e58866107`):

```sh
"$NEW" install --stage
grep -qa 'GET /extension-usage y GET /usage/state' "$(readlink -f ~/.local/share/comandos/bin/comandos)" || echo "STAGE MALO: ~/.local/share/comandos/bin/comandos install --rollback-release"
cat ~/.local/share/comandos/releases/previous                       # 790e58866107
~/.local/share/comandos/bin/comandos hook claude-status >/dev/null && echo "hooks OK con la release nueva"
```

Después el drop-in del ajuste de malloc, el `daemon-reload` y el reinicio. El drop-in no cambia el
`ExecStart`, que sigue ejecutando la release activa por los enlaces `~/.local/bin/cc-dash` →
`~/.local/share/comandos/bin/comandos` → `releases/<sha12>/comandos`:

```sh
mkdir -p ~/.config/systemd/user/cc-dash.service.d
cat > ~/.config/systemd/user/cc-dash.service.d/malloc.conf <<'EOF'
# Fase 2e: ajuste de glibc malloc del frente (docs/verification/cutover-dash.md, «2e»).
# Solo el frente: sus hijos (tmux, systemd-run, ssh, git, fc-list) no lo heredan.
[Service]
Environment=GLIBC_TUNABLES=glibc.malloc.mmap_threshold=131072:glibc.malloc.arena_max=2
EOF
systemctl --user daemon-reload
systemctl --user show -p Environment cc-dash.service \
  | grep -o 'GLIBC_TUNABLES=glibc.malloc.mmap_threshold=131072:glibc.malloc.arena_max=2'   # una línea
systemctl --user show -p Environment cc-dash-legacy.service | grep -c GLIBC_TUNABLES        # 0
systemctl --user set-environment COMANDOS_DASH_TRACE_FORWARD=1   # traza del paso 3 desde este arranque
systemctl --user restart cc-dash.service
date '+%F %T' | tee /tmp/arranque-2e.txt                               # hora del reinicio
curl -s -o /dev/null -w '%{http_code}\n' http://127.0.0.1:4777/        # 200
journalctl --user -u cc-dash.service --since -2min --no-pager | grep -c 'se reenvían al heredado\|rutas nativas desactivadas'   # 0
P=$(systemctl --user show -p MainPID --value cc-dash.service)
tr '\0' '\n' < /proc/$P/environ | grep -A1 '^GLIBC_TUNABLES='         # dos líneas (glibc parte en ':')
grep -lz '^GLIBC_TUNABLES=' /proc/[0-9]*/environ 2>/dev/null | cut -d/ -f3   # solo $P
```

El último comando lista los procesos con la variable: solo el frente. Si aparece otro, ver su
padre (`ps -o pid,ppid,cmd -p <pid>`): un hijo del frente que la hereda es un fallo (revertir el
drop-in, paso 4).

### 3. Verificación

Con efectos encendidos el frente es dueño de los límites: la guardia de `/state` usa la lectura
del arranque. El frente arrancó en el paso 2 ya con `COMANDOS_DASH_TRACE_FORWARD=1`, así que la
traza y la vigilancia corren sobre ese mismo proceso, sin un segundo reinicio (que repetiría el
refresco OAuth del arranque y la gracia de 75 s de la importación). Una muestra por minuto desde
el reinicio (minuto, Pss del frente en KiB, hilos, Pss del heredado):

```sh
P=$(systemctl --user show -p MainPID --value cc-dash.service)
L=$(systemctl --user show -p MainPID --value cc-dash-legacy.service)
for i in $(seq 10); do
  sleep 60
  echo "$i $(awk '/^Pss:/{print $2}' /proc/$P/smaps_rollup) $(ls /proc/$P/task | wc -l) $(awk '/^Pss:/{print $2}' /proc/$L/smaps_rollup)"
done | tee /tmp/vigilancia-2e.txt
awk '$2 > 57344 || $3 > 12 {print "REVERTIR: minuto " $1 ": " $2 " KiB, " $3 " hilos"}
     $1 == 5 {m5 = $2} $1 == 10 && $2 - m5 > 3072 {print "REVERTIR: +" $2 - m5 " KiB del minuto 5 al 10"}' \
  /tmp/vigilancia-2e.txt                                              # nada
journalctl --user -u cc-dash.service --since "$(cat /tmp/arranque-2e.txt)" --no-pager \
  | grep -c 'GET /state declina'                                     # 0
journalctl --user -u cc-dash.service --since "$(cat /tmp/arranque-2e.txt)" --no-pager | grep 'reenvío' \
  | sed 's/.*reenvío //' | sort | uniq -c | sort -rn
systemctl --user unset-environment COMANDOS_DASH_TRACE_FORWARD     # sin reiniciar: el mismo proceso sigue
sqlite3 "file:$HOME/.claude/hooks/comandos-usage.sqlite?mode=ro" \
  'select max(turn_finished_at), count(*) from usage_turns'; cat /tmp/base-2e-turnos.txt   # avanza respecto a la base
diff <(tmux list-panes -a -F '#{pane_id} #{@ccmodel}' | sed -E 's/#\[[^]]*\]//g; s/ ▸ / /' \
         | awk 'NF > 1' | LC_ALL=C sort) \
     <(LC_ALL=C sort ~/.claude/hooks/pane-models.txt)               # vacío
```

La última orden lee los bordes del tmux real (socket por omisión, solo lectura), les quita los
estilos y los compara con `pane-models.txt`: vacío. Una diferencia en un pane que cambia de modelo
en ese segundo se repite a los 10 s.

La primera importación del frente empieza unos 75 s después del reinicio (la gracia) y, con el
`seen` vacío, relee todos los transcripts de los últimos 21 días: unos 30–35 s con los datos de
hoy (ver «Medido antes del cutover»). Cae en las muestras de los minutos 1 y 2. Con el descarte
en flujo de las líneas largas que no usa, la sonda midió un pico de +5–6 MiB sobre la base con el
ajuste de malloc (antes, +11 MiB): sobre los ≈ 45 MiB de régimen quedan unos 5 MiB de margen
hasta la puerta de 56 MiB. El criterio no cambia: una muestra por encima es motivo para
revertir. Ese mismo minuto el frente hace sus
primeras llamadas OAuth con el cliente compartido: la ruta TLS real se mide aquí por primera vez
(la pila de `poll --shadow` no tiene DNS).

Seguimiento a los 30 y a los 60 minutos del reinicio, sobre el mismo proceso:

```sh
P=$(systemctl --user show -p MainPID --value cc-dash.service)
awk '/^Pss:/{print $2}' /proc/$P/smaps_rollup; ls /proc/$P/task | wc -l   # minuto 30: anotar
awk '/^Pss:/{print $2}' /proc/$P/smaps_rollup; ls /proc/$P/task | wc -l   # minuto 60: anotar
```

Vigilancia larga, sobre el mismo proceso: una muestra entre las 4 y las 6 horas del reinicio y otra
al día siguiente (la pendiente medida en sombra, ≈ 10 MiB/h en 10 min con cinco puntos que oscilan,
no descarta una fuga lenta que cruce la puerta en unas horas). Antes de cada una, comprobar que
el proceso es el mismo (`NRestarts=0` y el mismo `MainPID`); si no, la muestra no vale y se
anota el reinicio:

```sh
P=$(systemctl --user show -p MainPID --value cc-dash.service)
systemctl --user show -p NRestarts -p ActiveEnterTimestamp cc-dash.service
echo "$(date '+%F %T') $(awk '/^Pss:/{print $2}' /proc/$P/smaps_rollup) $(ls /proc/$P/task | wc -l)" \
  | tee -a /tmp/vigilancia-2e-larga.txt                             # 4–6 h; mañana, otra línea
awk '$3 > 57344 || $4 > 12 {print "REVERTIR: " $1 " " $2 ": " $3 " KiB, " $4 " hilos"}' \
  /tmp/vigilancia-2e-larga.txt                                       # nada
```

Revertir (paso 4, release `790e58866107`) si cualquier muestra pasa de 56 MiB (57 344 KiB) o de
12 hilos, si del minuto 5 al 10 crece más de 3 MiB, o si del minuto 30 al 60 crece más de 2 MiB.
Las mismas dos cotas (56 MiB, 12 hilos) valen para las muestras de las 4–6 h y del día siguiente.

Dónde se anota: como en la 2d, en una subsección «Ejecutado — <fecha> (release `<sha12>`, main
`<commit>`)» al final de esta sección 2e, con las diez muestras de `/tmp/vigilancia-2e.txt`, las de
los minutos 30 y 60, las dos de `/tmp/vigilancia-2e-larga.txt`, el recuento de reenvíos y si se
revirtió. La del día siguiente se añade a esa misma subsección cuando se toma.

- Ninguna ruta de uso en la traza; `GET /usage/guard` y `GET /usage/analytics` tampoco (la 2d ya
  no los pide). Una línea `GET /usage/state` aislada es un ciclo del Python (ver «Diferencias»);
  repetidas, revertir. `GET /state` no aparece (o menos de 1 de cada 100 sondeos): si aparece en
  cada sondeo, la guardia no tiene límites (credenciales ilegibles: falla cerrado, ver
  «Diferencias»).
- La barra lateral muestra los % de cuota desde el primer minuto tras el reinicio (refresco del
  arranque). El menú «Cuenta» de un pane puede salir sin límites hasta 180 s si la primera
  petición OAuth dio 429.
- Tras un turno de Claude o Codex, la tarjeta de uso lo refleja en ≤ 2 min; el borde del pane
  cambia al cambiar de modelo en ≤ 10 s; el `diff` de bordes y `pane-models.txt` sale vacío.
- Memoria: las diez muestras ≤ 56 MiB y ≤ 12 hilos; del minuto 5 al 10, ≤ 3 MiB; del 30 al 60,
  ≤ 2 MiB; a las 4–6 h y al día siguiente, ≤ 56 MiB y ≤ 12 hilos. El heredado (última columna) apenas crece frente a `/tmp/base-2e-heredado.txt`: ya no
  importa ni reconstruye el estado de uso.
- Llamadas OAuth: el frente refresca como mucho cada 60 s y el Python cada 300 s. Si
  `api.anthropic.com` empieza a dar 429 con frecuencia, anotarlo: la caché de error del frente
  (180 s) lo absorbe, pero es la señal para adelantar la 2g (un solo dueño de límites).

### 4. Reversión

A/B sin cambiar binario (el Python vuelve a ser dueño de importación y bordes en su primer
`/usage/state`; su primera respuesta puede traer el estado de antes del cutover durante un ciclo).
Si la reversión llega dentro de la ventana del paso 3, quitar antes la traza del entorno del
gestor (`systemctl --user unset-environment COMANDOS_DASH_TRACE_FORWARD`):

```sh
systemctl --user set-environment COMANDOS_DASH_NATIVE=0 && systemctl --user restart cc-dash.service
# persistente: drop-in ~/.config/systemd/user/cc-dash.service.d/no-native.conf con
# [Service]\nEnvironment=COMANDOS_DASH_NATIVE=0, daemon-reload y restart (deshacer: rm + daemon-reload + restart)
```

Quitar solo el ajuste de malloc (la 2e sigue, con ≈ 61 MiB en vez de ≈ 45):

```sh
rm ~/.config/systemd/user/cc-dash.service.d/malloc.conf
systemctl --user daemon-reload && systemctl --user restart cc-dash.service
systemctl --user show -p Environment cc-dash.service | grep -c GLIBC_TUNABLES   # 0
```

Volver a la release de la 2d con la ligereza, `790e58866107` (la actual del paso 0), nunca a
`9f6fa07f26f0` (la 2d de antes de la ligereza, 270 MiB en vivo). `install --rollback-release`
vuelve a la que nombra `releases/previous`, que el `--stage` del paso 2 dejó en `790e58866107`.
Comprobarlo antes, y quitar también el drop-in: el binario de la 2d no quita la variable a sus
hijos (un `tmux` que arrancara el servidor se la pasaría a los panes):

```sh
cat ~/.local/share/comandos/releases/previous                       # 790e58866107
~/.local/share/comandos/bin/comandos install --rollback-release
readlink -f ~/.local/share/comandos/bin/comandos                    # …/releases/790e58866107/comandos
rm -f ~/.config/systemd/user/cc-dash.service.d/malloc.conf && systemctl --user daemon-reload
systemctl --user restart cc-dash.service
grep -qa 'GET /extension-usage y GET /usage/state' "$(readlink -f ~/.local/share/comandos/bin/comandos)" && echo "SIGUE LA 2e"
grep -qa 'GET /pomodoro, GET /sovereignty y GET /state' "$(readlink -f ~/.local/share/comandos/bin/comandos)" || echo "NO ES LA 2d"
systemctl --user show -p Environment cc-dash.service | grep -c GLIBC_TUNABLES   # 0
```

Si `previous` no es `790e58866107` (otro `--stage` entre medias), no usar `--rollback-release`:
apagar lo nativo con el A/B de arriba (`COMANDOS_DASH_NATIVE=0`), quitar el drop-in y decidir la
release con el usuario.

El centinela de la 2d (`'GET /pomodoro, GET /sovereignty y GET /state'`) solo lo lleva el binario
de la 2d. Desde el rebase de la 2e sobre main, `UsageBackend::ROUTES` dice `"GET /pomodoro, GET
/sovereignty, GET /state, GET /analytics/week y GET /extension-usage"`; tras la Tarea 8, `"…, GET
/analytics/week, GET /accounts, GET /extension-usage y GET /usage/state"`. El centinela de la 2e
(`'GET /extension-usage y GET /usage/state'`) exige ese orden.

Las filas que escribe el frente (turnos, tramos, configuraciones, paneles, fotos de cuota) tienen
el formato y las claves del Python: revertir no necesita limpiar nada. `comandos-usage-import.lock`
puede quedarse en `~/.claude/hooks`.

### Medido antes del cutover (rama `migration/rust-fase2e`)

Rama de la 2e, binario release, arnés en namespace de red privado, copias de `~/.claude/hooks`,
`app-state.sqlite3` y `comandos-usage.sqlite`. El frente de la pila lleva el `GLIBC_TUNABLES` de
producción (lo pone `xtask`); los dos Python, no. Los sondeos usaron el binario de `47bb705`; la
suite y la paridad, el de `7f060d6`, que solo añade el contador de escrituras de fotos de cuota
en vuelo que las pruebas esperan (sin cambio en lo que sirve el frente).

```sh
D="--hooks $HOME/.claude/hooks --state-db $HOME/.local/state/comandos/app-state.sqlite3 \
   --usage-db $HOME/.claude/hooks/comandos-usage.sqlite --comandos $NEW"
"$XT" poll --shadow --minutes 5 $D --agents 48 --extra-panes 190 --pollers 3 --burst 17 \
  --max-threads 12 --max-pss-mib 56 --max-static-p95-ms 100     # puerta: sale 0
"$XT" poll --shadow --minutes 10 $D
"$XT" poll --shadow --minutes 10 $D --no-native
# sin el ajuste de malloc (para comparar): añadir --glibc-tunables ''
```

`poll` anuncia en su primera línea el `GLIBC_TUNABLES` con que arranca el frente.

- Suite del workspace: 1012 pruebas, 0 fallos, 1 ignorada (herramienta manual de RSS);
  `fmt` y `clippy -D warnings` limpios.
- `xtask parity`: 165 OK, 0 DIFF, 0 SKIP de 165. Reenviadas, 13 rutas distintas, una vez cada
  una y todas por diseño: GET `/states`, `/operator`, `/no-existe.css`, `/vendor/`,
  `/model/status`, `/pomodoro` (con consulta), `/analytics/weekly`, los prefijos `/accountsX`,
  `/providersX`, `/extension-usageX` y `/usage/stateX`, y POST `/workspace/close-group`,
  `/terminal-panes` (`close`). GET `/usage/state`, `/analytics/week` y `/accounts` ya no aparecen.
  Una corrida dio 164 OK y 1 DIFF en `f-sovereignty`: `usage_tool_calls` con 118 555 filas en el
  oráculo y 118 556 en el frente. El arnés copia la base viva dos veces (una por HOME) y un hook
  insertó una fila entre las dos copias; la corrida siguiente dio 165 OK.
- Puerta de ligereza (5 min, carga realista `--agents 48 --extra-panes 190 --pollers 3 --burst 17
  --max-threads 12 --max-pss-mib 56 --max-static-p95-ms 100`, con el ajuste de malloc): pasa.
  Pss del frente 21 609 → 47 369 → 46 773 → 45 477 → 44 341 → 45 069 KiB (minutos 0 a 5), 6–7
  hilos, 2815 peticiones, 0 errores, 0 no-2xx, ninguna ruta reenviada. GET `/usage/state`
  124/916/3175 ms, GET `/state` 1/800/905, estáticos en la ola 0/23/36 (la máquina compilaba a
  la vez en los minutos 1–4: las latencias de esta corrida sirven para comparar, no como
  referencia). Sin el ajuste, la misma carga quedó en 58–61 MiB (ronda 3 de la memoria).
- `xtask poll --shadow --minutes 10` con nativo (carga estándar): 3390 peticiones, 0 errores,
  0 no-2xx, ninguna ruta reenviada. Pss del frente 21 569 KiB al minuto 0, 40 457 al minuto 1 y
  42 077 al minuto 10; desde el minuto 5, 41 301–42 713 KiB (1412 KiB de banda) y pendiente de
  10 121 KiB/h (cinco puntos que oscilan: 42 713 al minuto 8, 42 077 al 10). Hilos: 6–7. Heredado
  de la pila: 45 722 KiB al minuto 1 y 47 131 al minuto 10 (+1409 KiB). GET `/usage/state`
  p50 77 ms, p95 414, p99 434 (60); GET `/analytics/week` 305/393/393 (10); GET `/state`
  114/159/220.
- Lo mismo con `--no-native` (todo reenviado): 3390 peticiones, 0 errores, 0 no-2xx; heredado
  187 544 KiB al minuto 1 y 306 276 al minuto 10 (+118 732 KiB). GET `/usage/state`
  144/733/805 ms; GET `/analytics/week` 450/531/531; GET `/state` 174/236/446.
- Criterios: frente plano, **no en sentido estricto**: +1620 KiB del minuto 1 al 10 y banda de
  1412 KiB desde el minuto 5, sin tendencia clara y por debajo de la puerta de 56 MiB; se vuelve a
  mirar en vivo (paso 3). Heredado ≤ 50 % del crecimiento sin nativo, sí (1409 / 118 732 = 1,2 %).
  p95 nativo de `/usage/state` ≤ p95 sin nativo, sí (414 ≤ 733 ms), y p99 ≤ 1000 ms, sí (434 ms).

Lo que **no** cubre la pila de `poll --shadow`: corre con un HOME que solo tiene copias de los
hooks y de las dos bases (transcripts sintéticos de una línea, sin `~/.codex/sessions`, sin Grok
ni OpenCode) y sin DNS, así que nunca hace la primera importación en frío real ni la ruta TLS de
los límites. Esas dos cosas se miden aparte o en vivo:

- **Importación en frío, medida con datos reales** (5 de octubre de 2026, 10:40–10:50). Sonda
  `crates/comandos-server/tests/import_cold_probe.rs` (`#[ignore]`, pide
  `COMANDOS_COLD_IMPORT_PROBE=1`), binario de pruebas en release: corre los `record_local_*` en el
  orden de la vuelta del frente (Codex → Grok → Claude por cuenta → OpenCode), con el `seen`
  vacío, contra `~/.codex/sessions`, `~/.grok/sessions`, `~/.claude/projects`,
  `~/.claude-accounts/*/projects` y una copia de `opencode.db`, hacia una base de uso nueva en un
  directorio temporal. Solo lectura sobre el HOME; sin red ni credenciales. Ventana de 21 días:
  2274–2277 archivos leídos (≈ 5,4 GB de Claude y ≈ 3,3 GB de Codex), 120 342 turnos escritos
  (`claude_jsonl` 71 891, `codex_rollout` 47 473, `grok_updates` 769, `opencode_db` 209).

  | Malloc | Caché de páginas | Pared | VmRSS antes | VmHWM | Pico sobre la base | VmRSS después |
  |---|---|---|---|---|---|---|
  | por omisión | fría (7,8 GiB leídos de disco) | 33,3 s | 5 656 KiB | 22 348 KiB | +16 692 KiB | 19 556 KiB |
  | por omisión | caliente | 32,8 s | 5 652 KiB | 20 984 KiB | +15 332 KiB | 18 684 KiB |
  | `GLIBC_TUNABLES` de producción | caliente | 28,6 s | 5 640 KiB | 16 652 KiB | +11 012 KiB | 9 128 KiB |
  | `GLIBC_TUNABLES` de producción | caliente | 35,2 s | 5 640 KiB | 16 564 KiB | +10 924 KiB | 9 008 KiB |

  Ese pico era de la fase de Codex (+10,6 MiB con el ajuste, +14,9 MiB sin él): hay rollouts de
  hoy con líneas de hasta 6,5 MB (`response_item` con salidas de herramientas, `compacted`,
  `event_msg` de tipo `item_completed`), que se guardaban enteras en el búfer de línea y se
  analizaban; Grok subía 7 MiB por sus trozos de mensaje. **Arreglado** (`8a8c6ae`): una línea
  de más de 128 KiB deja de guardarse y se lee en flujo (`comandos-store/src/line_scan.rs`)
  anotando solo `type` y `payload.type` (Codex), `type` (Claude) o
  `params.update.sessionUpdate` (Grok), con la última clave repetida ganando como en
  `json.loads`; si es de un tipo que el Python ignora se salta, y si no se puede probar (escapes
  en una clave o en el valor) se relee entera. El oráculo `big_lines_skip_matches_python_and_full_read`
  da las mismas filas con el descarte, sin él y en el Python. La misma sonda tras el arreglo
  (`COMANDOS_PROBE_BIG_LINES=read` la corre sin él, para comparar en el mismo estado de caché):

  | Malloc | Líneas largas | Caché | Pared | VmRSS antes | VmHWM | Pico sobre la base | VmRSS después |
  |---|---|---|---|---|---|---|---|
  | por omisión | descartadas | fría (8,1 GiB de disco) | 29,7 s | 5 632 KiB | 9 452 KiB | +3 820 KiB | 9 452 KiB |
  | `GLIBC_TUNABLES` de producción | descartadas | caliente | 27,2 s | 5 700 KiB | 11 368 KiB | +5 668 KiB | 9 104 KiB |
  | `GLIBC_TUNABLES` de producción | enteras (comparación) | caliente | 35,3 s | 5 680 KiB | 17 084 KiB | +11 404 KiB | 9 164 KiB |
  | `GLIBC_TUNABLES` de producción | descartadas | caliente | 27,7 s | 5 656 KiB | 10 864 KiB | +5 208 KiB | 9 156 KiB |

  Con el ajuste, la fase de Codex sube ahora 2,8–4,0 MiB, Grok ≤ 1 MiB y cada cuenta de Claude
  1,8–2,5 MiB; unos 3,5 MiB del pico se quedan como caché de SQLite y `seen` (9,1 MiB después),
  así que lo transitorio es ≈ 2 MiB. Mismas filas (120 803 turnos, 2278 archivos). En el frente
  (≈ 45 MiB con el ajuste) la primera importación queda hacia los 50–51 MiB, con ≈ 5 MiB de
  margen a la puerta.
- **Ruta OAuth/TLS.** Desde la ronda de arreglo el frente comparte un solo cliente `reqwest` entre
  cuentas y vueltas (antes, uno por cuenta cada 60 s), sin conexiones ociosas en el pool. Su
  memoria y sus errores reales se miden por primera vez en vivo, en el paso 3: el refresco del
  arranque sale en el primer segundo y la primera importación unos 75 s después del reinicio,
  ambos dentro de la ventana de vigilancia.
- **Tras el descarte de líneas largas** (`8a8c6ae`, binario release de ese commit): suite del
  workspace 1021 pruebas, 0 fallos, 2 ignoradas; `fmt` y `clippy -D warnings` limpios; `xtask
  parity` 165 OK, 0 DIFF, 0 SKIP. Puerta de ligereza de 5 min con la carga realista: sale 0; Pss
  21 945 → 42 349 → 43 113 → 44 549 → 43 393 → 43 773 KiB (minutos 0 a 5), 6 hilos, 2815
  peticiones, 0 errores, 0 no-2xx, ninguna ruta reenviada; GET `/usage/state` 92/438/500 ms, GET
  `/state` 575/699/734, estáticos en la ola 0/23/32.
- **Tras la ronda de arreglo** (fusión de `main` `3f0a01a`, I1, I2, M1, M3; binario release de
  `7d75841`): suite del workspace 1017 pruebas, 0 fallos, 2 ignoradas (la herramienta manual de
  RSS y la sonda de importación); `fmt` y `clippy -D warnings` limpios. `xtask parity` 165 OK,
  0 DIFF, 0 SKIP (una corrida previa dio 164 OK y 1 DIFF en `f-sovereignty`: `events.jsonl`
  cambió entre las dos copias del arnés, como la vez anterior). Puerta de ligereza de 5 min con
  la carga realista: sale 0; Pss 21 905 → 44 141 → 47 677 → 47 569 → 47 553 → 47 469 KiB
  (minutos 0 a 5), 6–7 hilos, 2815 peticiones, 0 errores, 0 no-2xx, ninguna ruta reenviada; GET
  `/usage/state` 104/903/2473 ms, GET `/state` 548/750/909, estáticos en la ola 0/21/22.

### Ejecutado — 5 de octubre de 2026, 11:32 (release `7f3235a940d3`, main `755b1fc`)

Fusión en `main` por avance rápido (la 2e solo toca `crates/`, `xtask/`, `docs/` y `Cargo.lock`:
el Python vivo no cambia). Paso 0 conforme: release actual `790e58866107`, sin drop-ins, mismos
entornos. Sombra en 4782: paridad 165 OK, 0 DIFF (una primera corrida dio 1 DIFF en
`POST /terminal-panes`: qué sesión reutiliza la terminal rápida depende del momento; repetida,
limpia); 0 reenvíos en la traza; `/providers` y `/optimization/plans` iguales;
`/accounts?harness=claude` y `/analytics/week?offset=-1` difieren solo en los campos de límites
(la sombra corre sin efectos y no los carga). La navegación manual con `chrome-bg` no se hizo: el
navegador del Mac tenía sus dos sesiones ocupadas por otros procesos.

Paso 2: nada en vuelo; base `1791221478|121847` turnos, heredado 462 049 KiB, frente 50 203 KiB
con 5 hilos. `--stage` → `7f3235a940d3`, `previous` = `790e58866107`. Drop-in `malloc.conf`;
`GLIBC_TUNABLES` solo en el frente (PID 583166). Respuesta 200, 0 líneas de rutas desactivadas,
las 21 sesiones de tmux intactas.

Paso 3, una muestra por minuto (Pss del frente en KiB, hilos, Pss del heredado):

| min | frente | hilos | heredado |
|---|---|---|---|
| 1 | 40 805 | 6 | 449 469 |
| 2 | 45 201 | 6 | 449 517 |
| 3 | 43 217 | 6 | 449 411 |
| 4 | 48 757 | 6 | 449 414 |
| 5 | 47 477 | 6 | 449 380 |
| 6 | 46 729 | 6 | 449 460 |
| 7 | 40 328 | 6 | 449 371 |
| 8 | 44 436 | 6 | 447 098 |
| 9 | 44 404 | 6 | 446 498 |
| 10 | 44 404 | 6 | 446 579 |

Máximo 48 757 KiB (47,6 MiB, con la primera importación en frío), minuto 5 → 10 −3 MiB. `GET
/state declina`: 0; reenvíos: ninguno. Turnos `1791221478|121847` → `1791222111|122005` (el frente
importa). `diff` de bordes y `pane-models.txt`: vacío. Minuto 30: 43 075 KiB, 7 hilos; minuto 60:
40 843 KiB, 6 hilos (−2,2 MiB del 30 al 60), `NRestarts=0`, mismo PID. No se revirtió.

El heredado sigue en ≈ 440 MiB: ya no importa ni reconstruye, pero el Python no devuelve al
sistema lo que reservó; baja solo al reiniciar `cc-dash-legacy` (no hace falta para la 2e).
Muestra de las 4–6 h (16:17:12, 4 h 45 min después del cutover): 43 084 KiB, 7 hilos,
`NRestarts=0`, mismo PID (583166); por debajo de 56 MiB y 12 hilos. No se revirtió.
Pendiente: la muestra del día siguiente.
