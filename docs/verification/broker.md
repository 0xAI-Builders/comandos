# Broker compartido de servidores MCP (`comandos ext broker`)

Fecha: 2026-10-04 (ronda de correcciones 1). Prueba:
`crates/comandos-extensions/tests/broker_daemon.rs`.

## Qué hace

Un daemon por usuario escucha en `$XDG_RUNTIME_DIR/comandos/broker.sock` (sin esa variable,
`/tmp/comandos-<uid>/broker.sock`). Cada sesión que corre `comandos ext serve <nombre>` para
un servidor stdio compartible se conecta y envía una línea `attach`. Tras `{"ok":true}`, copia
bytes entre su stdin/stdout y el socket. El daemon reparte las líneas JSON-RPC con el `Mux`
(ids traducidos, `initialize` cacheado, cancelaciones al desconectar).

## Protocolo de `attach`

La sesión envía, en una línea:

```json
{"attach":"<nombre>","cwd":"<directorio absoluto>","env":{"CLAVE":"valor"},"catalog":"<ruta absoluta del catálogo>","home":"<HOME de la sesión>"}
```

- `cwd`: el directorio donde el proxy directo habría lanzado el proceso (el `cwd` del
  catálogo resuelto contra el directorio de la sesión, o el directorio de la sesión).
- `env`: el `env` del catálogo con `${VAR}` expandido en el entorno de la sesión.
- `catalog` y `home`: si no coinciden con los del daemon, la respuesta es
  `{"error":"catálogo distinto"}` o `{"error":"home distinto"}`.

El daemon responde `{"ok":true}` solo cuando el upstream ya contestó su primer `initialize`
(o en seguida, si el upstream ya estaba inicializado). Un upstream nuevo lo inicializa el
propio daemon con capacidades vacías; el `initialize` real de cada sesión se contesta desde la
caché del `Mux`. Si el upstream muere antes de contestar o en los 500 ms siguientes a
lanzarlo, rechaza `initialize` o no contesta en 30 s, la respuesta es un error
(`upstream murió al arrancar: <estado>`, `upstream rechazó initialize`,
`upstream no respondió a initialize`) y esa clave queda 30 s en enfriamiento: toda alta recibe
el mismo error y no se relanza. El daemon espera el alta como mucho 5 s; la sesión, 6 s.

## Clave de compartición

Un upstream se comparte solo si el proceso sería idéntico: clave
`(nombre, cwd, sha256 del JSON canónico del env)`. El daemon lo lanza con `current_dir(cwd)` y
con el `env` de la sesión superpuesto a su propio entorno, como haría el proxy directo.
`crate::command_env` es la única función que arma el comando (la usan `serve` y el broker).

## Respaldo al proxy directo

- Sin socket: proxy directo y stderr vacío (broker no instalado).
- Con socket y cualquier error de `attach` (conexión rechazada, daemon muerto, error del
  daemon, plazo vencido): una línea `broker no disponible, proxy directo` en stderr y proxy
  directo.
- Si el broker cierra antes de que la sesión haya leído un byte de stdin (y sin haber escrito
  nada en stdout), también se vuelve al proxy directo. Después, el cierre es definitivo.
- Al cerrar stdin, la sesión cierra su mitad de escritura y sale cuando el broker cierra o a
  los 2 s.
- Con `--home` o `--catalog` explícitos, `serve` usa siempre el proxy directo.

## Límites

- Línea máxima: 16 MiB por conexión de cliente y por upstream (cabe una captura de Chrome en
  base64). Una línea mayor cierra solo esa conexión o ese upstream.
- Las líneas hacia el upstream (incluidas cancelaciones y errores del `Mux`) esperan hasta 2 s
  si el stdin está lleno; solo entonces se descartan con una línea de registro.
- Un cliente con 1024 líneas sin leer se desconecta.
- El actor procesa como mucho 64 líneas del upstream seguidas antes de atender altas, bajas y
  líneas de clientes.
- Cierre de un upstream: SIGTERM al grupo de procesos, 5 s, SIGKILL; después, SIGKILL al grupo
  otra vez para los nietos. `kill_on_drop` solo cubre al líder.

## Qué cubren las pruebas

- Dos sesiones `serve eco` comparten un pid; un servidor `"shared": false` no.
- Distinto `cwd` ⇒ distinto upstream; mismo `cwd` y `env` ⇒ el mismo.
- Sin daemon, proxy directo con stderr vacío.
- Socket presente con el daemon muerto: una línea en stderr y proxy directo.
- Upstream que muere al arrancar: error, proxy directo, y el segundo `attach` dentro de los
  30 s recibe el error sin relanzar (un solo `spawn` en el registro).
- Upstream colgado: el `attach` vence a los 5 s y cae al proxy directo; otro servidor responde
  mientras tanto en menos de 3 s.
- Peticiones concurrentes con los mismos ids desde dos sesiones: cada respuesta vuelve a su
  sesión con su id (el fake devuelve los `params` en `echo`).
- Al cerrar una sesión la otra sigue; si el upstream muere (pasados los 500 ms) la sesión
  restante recibe EOF y el siguiente `attach` relanza.
- SIGTERM al daemon cierra los upstreams, borra el socket y sale con 0.
- Inactividad de 1 s cierra el upstream sin clientes.
- Un segundo daemon con el primero vivo sale con error y el primero sigue sirviendo.
- Un socket huérfano de un daemon muerto con SIGKILL se reemplaza.
- El registro del daemon no contiene cargas JSON-RPC.

`systemd-analyze --user verify systemd/comandos-broker.service` solo señala que el binario
`%h/.local/share/comandos/bin/comandos` no existe (no se instala en esta tarea); ningún aviso de
ciclo de orden tras quitar `After=default.target`.

## Memoria

Medido con `cargo xtask rss --samples 3` sobre el binario release, upstream `fake_mcp_stdio`
(release). Medianas de tres muestras, KiB:

| Árbol medido | Procesos | RSS | PSS |
|---|---|---|---|
| Daemon sin clientes | 1 | 4564 | 2641 |
| `fake_mcp_stdio` solo | 1 | 2140 | 545 |
| Daemon con un cliente fino y el fake adjunto (daemon + fake) | 2 | 7556 | 2806 |
| Cliente fino `serve eco` conectado al daemon | 1 | 5112 | 1995 |

El daemon con el fake adjunto ocupa unos 5416 KiB de RSS (7556 − 2140) y 2261 KiB de PSS
(2806 − 545). Cada sesión pasa a costar un cliente fino de unos 5 MiB de RSS en lugar de su
propia copia del servidor MCP.

Líneas de `xtask rss`, abreviadas (rutas relativas al workspace, sin `unix_time`); no se
añadieron a `rss.jsonl`:

```
{"cmd":["env","HOME=/tmp/cbm9/h","XDG_RUNTIME_DIR=/tmp/cbm9/run",".build/target/release/comandos-extensions","broker"],"samples":3,"pss_kib_min":2577,"pss_kib_median":2641,"rss_kib_min":4516,"rss_kib_median":4564,"procs":1,"pss_exact":true}
{"cmd":[".build/target/release/fake_mcp_stdio"],"samples":3,"pss_kib_min":477,"pss_kib_median":545,"rss_kib_min":2072,"rss_kib_median":2140,"procs":1,"pss_exact":true}
{"cmd":["env","HOME=/tmp/cbm9/h","XDG_RUNTIME_DIR=/tmp/cbm9/run",".build/target/release/comandos-extensions","broker"],"samples":3,"pss_kib_min":2776,"pss_kib_median":2806,"rss_kib_min":7496,"rss_kib_median":7556,"procs":2,"pss_exact":true}
{"cmd":["env","HOME=/tmp/cbm9/h","XDG_RUNTIME_DIR=/tmp/cbm9/run",".build/target/release/comandos-extensions","serve","eco"],"samples":3,"pss_kib_min":1867,"pss_kib_median":1995,"rss_kib_min":4992,"rss_kib_median":5112,"procs":1,"pss_exact":true}
```

En la tercera medición, un bucle en segundo plano conectaba un cliente (`serve eco` con un
`initialize`) en cuanto aparecía el socket; `procs: 2` confirma que el fake estaba adjunto.

## Límites conocidos

- Solo se comparten servidores stdio sin `enabled_tools`/`disabled_tools`; los HTTP y los
  filtrados siguen por el proxy directo.
- El upstream compartido se inicializa con capacidades de cliente vacías: un servidor que
  dependa de `roots`, `sampling` o `elicitation` del cliente se comporta como con un cliente
  que no las declara. Si eso importa, hay que marcarlo `"shared": false`.
- Del entorno de la sesión solo cuenta el `env` del catálogo; el resto de variables viene del
  daemon.
- `systemd/comandos-broker.service` está en el repositorio pero no se instala en esta tarea.
