# Broker compartido de servidores MCP (`comandos ext broker`)

Fecha: 2026-10-04. Prueba: `crates/comandos-extensions/tests/broker_daemon.rs`.

## Qué hace

Un daemon por usuario escucha en `$XDG_RUNTIME_DIR/comandos/broker.sock` (sin esa variable,
`/tmp/comandos-<uid>/broker.sock`). Cada sesión que corre `comandos ext serve <nombre>` para
un servidor stdio compartible se conecta, envía `{"attach":"<nombre>"}` y, tras `{"ok":true}`,
copia bytes entre su stdin/stdout y el socket. El daemon mantiene un solo proceso upstream por
nombre y reparte las líneas JSON-RPC con el `Mux` (ids traducidos, `initialize` cacheado,
cancelaciones al desconectar).

Sin socket, `serve` usa el proxy directo de siempre sin escribir nada en stderr. Con socket
pero sin respuesta en 300 ms (conexión) o 1 s (respuesta al `attach`), escribe
`broker no disponible, proxy directo` y también vuelve al proxy directo.

## Qué cubren las pruebas

- Dos sesiones `serve eco` reciben el `serverInfo.version` del mismo pid: un solo upstream.
- Un servidor `"shared": false` no pasa por el broker: dos invocaciones, dos pids.
- Sin daemon, `serve` ejecuta el comando directo y su stderr queda vacío.
- Dos sesiones usan el mismo id de petición (`2`) y cada una recibe su respuesta con su id.
- Al cerrar una sesión, la otra sigue funcionando.
- Si el upstream muere (SIGKILL), la sesión restante recibe EOF y su proceso termina; el
  siguiente `attach` lanza un upstream nuevo.
- SIGTERM al daemon cierra los upstreams, borra el socket y sale con 0.
- Con `COMANDOS_BROKER_IDLE_SECS=1`, el upstream sin clientes se cierra al segundo.
- Un segundo daemon con el primero vivo sale con error.
- Un socket huérfano de un daemon muerto con SIGKILL se reemplaza al arrancar el siguiente.
- El registro del daemon no contiene cargas JSON-RPC.

## Memoria

Medido con `cargo xtask rss --samples 3` sobre el binario release
(`.build/target/release/comandos-extensions`), upstream `fake_mcp_stdio` (release). Medianas
de tres muestras, KiB:

| Árbol medido | Procesos | RSS | PSS |
|---|---|---|---|
| Daemon sin clientes | 1 | 4488 | 2557 |
| `fake_mcp_stdio` solo | 1 | 2072 | 477 |
| Daemon con un cliente fino y el fake adjunto (daemon + fake) | 2 | 7444 | 2848 |
| Cliente fino `serve eco` conectado al daemon | 1 | 5104 | 2081 |

El daemon con el fake adjunto ocupa unos 5372 KiB de RSS (7444 − 2072) y 2371 KiB de PSS
(2848 − 477). Cada sesión pasa a costar un cliente fino de unos 5 MiB de RSS en lugar de su
propia copia del servidor MCP (un proceso node suele ocupar decenas de MiB).

Líneas de `xtask rss`, abreviadas (rutas relativas al workspace, sin `unix_time`); no se añadieron a `rss.jsonl`:

```
{"cmd":["env","HOME=/tmp/cbm9/h","XDG_RUNTIME_DIR=/tmp/cbm9/run","COMANDOS_BROKER_IDLE_SECS=600",".build/target/release/comandos-extensions","broker"],"samples":3,"pss_kib_min":2489,"pss_kib_median":2557,"rss_kib_min":4412,"rss_kib_median":4488,"procs":1,"pss_exact":true}
{"cmd":[".build/target/release/fake_mcp_stdio"],"samples":3,"pss_kib_min":477,"pss_kib_median":477,"rss_kib_min":2072,"rss_kib_median":2072,"procs":1,"pss_exact":true}
{"cmd":["env","HOME=/tmp/cbm9/h","XDG_RUNTIME_DIR=/tmp/cbm9/run","COMANDOS_BROKER_IDLE_SECS=600",".build/target/release/comandos-extensions","broker"],"samples":3,"pss_kib_min":2772,"pss_kib_median":2848,"rss_kib_min":7364,"rss_kib_median":7444,"procs":2,"pss_exact":true}
{"cmd":["env","HOME=/tmp/cbm9/h","XDG_RUNTIME_DIR=/tmp/cbm9/run",".build/target/release/comandos-extensions","serve","eco"],"samples":3,"pss_kib_min":1875,"pss_kib_median":2081,"rss_kib_min":4984,"rss_kib_median":5104,"procs":1,"pss_exact":true}
```

En la tercera medición, un bucle en segundo plano conectaba un cliente
(`serve eco` con un `initialize`) en cuanto aparecía el socket, para que el daemon lanzara el
fake antes de que `xtask` midiera a los 2 s; `procs: 2` confirma que el fake estaba adjunto.

## Límites conocidos

- Solo se comparten servidores stdio sin `enabled_tools`/`disabled_tools`; los HTTP y los
  filtrados siguen por el proxy directo.
- Con `--home` o `--catalog` explícitos, `serve` no usa el broker (el daemon lee el catálogo
  del usuario).
- `systemd/comandos-broker.service` está en el repositorio pero no se instala en esta tarea.
