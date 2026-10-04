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

## Rutas sin llamador vivo (39)

Siguen reenviadas al Python en esta fase. En la Fase 2b responderán `410 {"error":"Ruta retirada"}`
salvo que el journal muestre un llamador real (entonces se portan). Fuente: inventario §1.12.

Sin ningún llamador (13): `GET /pomodoro/report`, `GET /session-config-history`,
`GET /project-profiles`, `POST /project-profile`, `POST /session/recover`, `POST /pause`,
`POST /usage/capture`, `POST /chains/delete`, `POST /app/command`, `GET /events/v2`,
`POST /events/v2`, `POST /event`, `GET /events`.

Solo en `lib/operator_catalog.py` (26): GET `/dedication`, `/proxy`, `/ui-log/summary`,
`/session-brain`, `/usage/guard`, `/usage/changes`, `/usage/provider-compare`,
`/usage/experiments`, `/usage/analytics`, `/usage/interactions`; POST `/proxy`,
`/optimization/default`, `/skill-toggle`, `/mcp-toggle`, `/usage/experiment`, `/usage/rating`,
`/usage/refresh`, `/usage/quota`, `/usage/subscription`, `/usage/settings`, `/news/refresh`,
`/models/refresh`, `/open-with-account`, `/tab-new`, `/harness/switch`, `/model/switch-cancel`.

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
