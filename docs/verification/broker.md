# Broker compartido de servidores MCP (`comandos ext broker`)

El broker conserva un único arranque por clave mientras los clientes esperan, incluso ante
errores recuperables. Esta descripción corresponde al código de
`/home/someguy/codebase/0xJesus/ComandOS/crates/comandos-extensions/src/broker/` y sus pruebas en
`/home/someguy/codebase/0xJesus/ComandOS/crates/comandos-extensions/tests/broker_daemon.rs`.
La activación de un binario instalado se distingue de la verificación de estas fuentes.

## Qué hace

Un daemon por usuario escucha en `$XDG_RUNTIME_DIR/comandos/broker.sock` (sin esa variable,
`/tmp/comandos-<uid>/broker.sock`). Cada sesión que corre `comandos ext serve <nombre>` para
un servidor stdio compartible se conecta y envía una línea `attach`. Tras `{"ok":true}`, reenvía
líneas entre su stdin/stdout y el socket. El daemon reparte las líneas JSON-RPC con el `Mux`
(ids traducidos, `initialize` cacheado, cancelaciones al desconectar).

## Protocolo de `attach`

La sesión envía, en una línea:

```json
{"attach":"<nombre>","cwd":"<directorio absoluto>","env":{"CLAVE":"valor"},"catalog":"<ruta absoluta del catálogo>","home":"<HOME de la sesión>","environ":{"CLAVE":"valor"},"path":"<PATH de la sesión>"}
```

- `cwd`: el directorio donde el proxy directo habría lanzado el proceso (el `cwd` del
  catálogo resuelto contra el directorio de la sesión, o el directorio de la sesión).
- `env`: el `env` del catálogo con `${VAR}` expandido en el entorno de la sesión.
- `environ` (opcional): el entorno completo del proceso `serve` (`std::env::vars_os`, solo los
  pares UTF-8 válidos).
- `path` (opcional, protocolo anterior): el `PATH` del proceso `serve`. Con `environ` ya no
  hace falta; el cliente lo sigue mandando solo para un daemon anterior que no entienda
  `environ`, y el daemon lo ignora si llega `environ`.
- `catalog` y `home`: si no coinciden con los del daemon, la respuesta incluye el error,
  `"fallback_safe":true` y `"retryable":false`. Esa sesión puede usar proxy directo.

El daemon responde `{"ok":true}` solo cuando el upstream ya contestó su primer `initialize`
(o en seguida, si el upstream ya estaba inicializado). Un upstream nuevo lo inicializa el
propio daemon con capacidades vacías; el `initialize` real de cada sesión se contesta desde la
caché del `Mux`. Si el upstream muere antes de contestar o en los 500 ms siguientes a
lanzarlo, rechaza `initialize` o no contesta en 30 s, la respuesta es un error
(`upstream murió al arrancar: <estado>`, `upstream rechazó initialize`,
`upstream no respondió a initialize`) y esa clave queda 30 s en enfriamiento: toda alta recibe
el mismo error y no se relanza. El actor recoge el grupo antes de liberar sus altas o
permitir su reemplazo. El registro y el cliente esperan ese resultado sin otro plazo que
abandone el arranque compartido.

Los errores se clasifican en
`/home/someguy/codebase/0xJesus/ComandOS/crates/comandos-extensions/src/broker/registry.rs`:

- Fallo de arranque, cooldown, retirada del actor o lectura fallida del catálogo:
  `{"error":"...","fallback_safe":false,"retryable":true}`. Los clientes reintentan
  `attach` contra el mismo registro. Incluso después de recoger el proceso fallido,
  lanzar un proxy por cliente produciría duplicados durante la recuperación.
- Validación de `attach`, identidad de catálogo/HOME diferente o modo no compartible:
  `{"error":"...","fallback_safe":true,"retryable":false}`. El broker autoriza directo.

Un cliente actualizado trata un error sin esos campos como resultado ambiguo de un daemon
anterior. Reintenta con esperas crecientes de 200 ms hasta 5 s, conservando al dueño conocido.
El límite de 5 s del daemon recibe solamente la primera línea `attach` de una conexión nueva.
El límite de 6 s del cliente corresponde al `initialize` cacheado durante la reanudación.

## Versión de protocolo

El `initialize` de calentamiento del daemon pide `LATEST_PROTOCOL_VERSION` (`2025-11-25`,
`/home/someguy/codebase/0xJesus/ComandOS/crates/comandos-extensions/src/serve/normalize.rs`) con capacidades `{}`; el upstream contesta la mayor que soporta (`U`) y
ese `InitializeResult` queda en caché. A cada cliente se le entrega una copia con
`protocolVersion` negociada contra `U`:

- Si la `params.protocolVersion` pedida es una cadena de `SUPPORTED_PROTOCOL_VERSIONS` y no es
  posterior a `U` (las fechas `YYYY-MM-DD` se comparan como cadenas), se devuelve la pedida.
- En cualquier otro caso (versión desconocida, posterior a `U`, ausente o no cadena), se
  devuelve `U` tal cual, aunque no esté en la lista.

El resto del resultado (`capabilities`, `serverInfo`, `instructions`) es idéntico para todos.
Los clientes que esperaban la primera respuesta reciben cada uno su versión. Si `U` no es una
cadena, el resultado va sin tocar.

La negociación supone que el upstream soporta todas las
versiones de la lista anteriores a `U`; uno que solo hable `U` respondería directo `U` a un
cliente que pide una anterior, y por el broker ese cliente recibe la anterior.

## Clave de compartición

Un upstream se comparte solo si el proceso sería idéntico: clave
`(nombre, cwd resuelto, sha256 del JSON canónico del spec efectivo)`. El spec efectivo es el del
catálogo del daemon en el momento del `attach` (`command`, `args`, `cwd` y demás campos), con su
`env` sustituido por el `env` expandido que manda la sesión y sin la clave `shared` (no cambia el
proceso); «canónico» es con las claves ordenadas a todos los niveles. Si el catálogo cambia los
`args` de un servidor, la siguiente sesión arranca un upstream nuevo en vez de heredar el viejo. El daemon lo lanza con `current_dir(cwd)` y
con el entorno completo de la sesión (`environ` del `attach`, ver «Entorno del upstream») más el
`env` del spec, sin heredar nada del daemon.
`crate::command_env` es la única función que arma el comando (la usan `serve` y el broker).

## Entorno del upstream

El daemon corre bajo `systemd --user`, cuyo entorno no es el de la sesión (PATH sin nvm ni bun,
sin las variables que exporta el shell del usuario). Por eso el cliente manda su entorno
completo en `environ` y el daemon lanza el upstream con `env_clear()`, después ese `environ` y
encima el `env` del spec (el spec gana, como en el proxy directo). Nada del entorno del daemon
llega al upstream. Un `command` sin `/` se resuelve en el `PATH` efectivo: el `env.PATH` del
spec si lo tiene, si no el `PATH` del `environ` (primer archivo regular ejecutable, como
`execvp`).

`environ` no forma parte de la clave de compartición: dos sesiones con distinto entorno y mismo
spec comparten upstream, y la primera en llegar fija el entorno del proceso (igual que antes
fijaba su PATH). Lo que un servidor necesita distinto por sesión debe ir en el `env` del
catálogo, que sí entra en la clave. La unidad fija solo un PATH mínimo para el propio binario,
sin rutas de nvm con versión.

Sin `environ` (cliente anterior) se conserva el comportamiento previo: entorno del daemon, con
el `path` del `attach` como `PATH` del upstream y para resolver el comando.

El stderr del upstream va a `/dev/null`, como en el proxy directo y en el Python
(`/home/someguy/codebase/0xJesus/ComandOS/lib/extension_proxy.py`): los servidores escriben ahí tokens y URLs de autorización que no
deben acabar en el journal del daemon.

## Respaldo al proxy directo

- Antes del alta, daemon ausente sin dueño conocido ni candado retenido: proxy directo y
  stderr vacío. Un archivo de candado huérfano y desbloqueado permite ese respaldo.
- Un candado `broker.lock` retenido conserva la espera aunque falte el socket durante
  arranque o parada. Una cola de conexiones llena tampoco acredita ausencia del daemon.
- Un rechazo con `fallback_safe:true` autoriza directo antes de iniciar el lector de stdin.
  Con `COMANDOS_DEBUG=1`, el cliente escribe `broker no disponible, proxy directo` en stderr.
- Un EOF ambiguo durante el alta reintenta contra el dueño conocido. Tras `{"ok":true}`,
  el cliente puede reanudar o terminar la sesión, sin volver a directo: su lector de stdin
  puede consumir bytes durante la espera. El tipo de retorno de `relay` es un código de salida.
- Al cerrar stdin, la sesión cierra su mitad de escritura y sale cuando el broker cierra o a
  los 2 s.
- Con `--home` o `--catalog` explícitos, `serve` usa siempre el proxy directo.

Estas decisiones están en
`/home/someguy/codebase/0xJesus/ComandOS/crates/comandos-extensions/src/broker/client.rs` y
`/home/someguy/codebase/0xJesus/ComandOS/crates/comandos-extensions/src/cli.rs`.

## Reinicio del daemon con sesiones vivas

El cliente fino reenvía por líneas y lleva la cuenta de su `initialize` original, de si ya
mandó `notifications/initialized` y de sus peticiones en vuelo (ids con `method` sin respuesta;
el `id` y el `method` de primer nivel se leen en streaming, sin deserializar la línea). Al
recibir EOF del daemon decide si fue un reinicio:

- Reinicio: el proceso del daemon (pid por `SO_PEERCRED` al conectar) ya no vive (o es zombi),
  o el socket desapareció o es otro inodo. Un daemon parado con SIGTERM borra el socket antes
  de cerrar a sus clientes, así que el caso `systemctl restart` se detecta aunque el proceso
  viejo siga cerrando upstreams.
- Cierre definitivo: el daemon sigue vivo con el mismo socket (el upstream terminó, cliente
  lento, inactividad). La sesión sale como antes, con EOF.

En un reinicio, cada petición en vuelo recibe en seguida
`{"code":-32603,"message":"broker reiniciado"}` con su id, y el cliente intenta hasta cinco
veces (tras 200 ms, 1 s, 3 s, 6 s y 10 s: un reinicio real espera hasta 5 s por cada upstream
que tarda en morir) un `attach` nuevo con la misma línea. Cada intento puede esperar al dueño
o a su recuperación; esas cinco esperas no constituyen un plazo total. Si lo consigue, reenvía el
`initialize` original (si ya tenía respuesta), descarta su respuesta, reenvía
`notifications/initialized` (si se había mandado) y sigue. La sesión solo nota el error de sus
peticiones en vuelo; el estado del servidor es el de un upstream nuevo. Sin daemon tras los
cinco intentos, la sesión sale como antes (ya no puede volver al proxy directo: habló con el
broker). Las peticiones de herramientas en vuelo no se repiten automáticamente.

## Límites

- Línea máxima: 256 MiB por conexión de cliente y por upstream, la misma que el proxy directo
  (`transport::MAX_RESPONSE`). Una línea de cliente mayor cierra solo esa conexión. Una línea
  del upstream mayor no cierra el upstream compartido: se deja de guardar, se consume hasta su
  `\n` buscando su `id` de primer nivel (aunque vaya al final, como en el SDK de TypeScript)
  y, si es una respuesta, su dueño recibe `{"code":-32603,"message":"respuesta demasiado
  grande"}` con su id; si es un request o una notificación del upstream, se descarta. El
  registro anota el tamaño, nunca la carga.
- Las líneas hacia el upstream (incluidas cancelaciones y errores del `Mux`) esperan hasta 2 s
  si el stdin está lleno; solo entonces se descartan con una línea de registro.
- Un cliente con 1024 líneas sin leer se desconecta.
- El actor procesa como mucho 64 líneas del upstream seguidas antes de atender altas, bajas y
  líneas de clientes.
- Descriptores: la unidad fija `LimitNOFILE=65536` y el daemon, al arrancar, sube su límite
  blando al duro (`setrlimit` vía `nix`, sin `unsafe`). Si aun así queda por debajo de 4096,
  escribe un aviso en el registro.
- Cierre de un upstream: SIGTERM al grupo de procesos, 5 s, SIGKILL; después, SIGKILL al grupo
  otra vez para los nietos. `kill_on_drop` solo cubre al líder.

## Qué cubren las pruebas

- Dos sesiones `serve eco` comparten un pid; un servidor `"shared": false` no.
- Distinto `cwd` ⇒ distinto upstream; mismo `cwd` y `env` ⇒ el mismo.
- Mismo nombre con otros `args` en el catálogo ⇒ distinto upstream; añadir `"shared": true` no.
- Sin daemon, proxy directo con stderr vacío.
- Socket presente con el daemon muerto: proxy directo con stderr vacío; con `COMANDOS_DEBUG=1`,
  una línea en stderr.
- Upstream que muere al arrancar: error recuperable, sin proxy directo; el segundo `attach`
  dentro del cooldown recibe el mismo error (un solo `spawn` en el registro).
- Upstream colgado: vence el initialize del actor a los 30 s; otro servidor responde mientras
  tanto en menos de 3 s.
- Tres clientes con arranque de 7 s comparten un upstream y reciben sus propios ids.
- Rechazo inicial y proceso resistente a TERM: tres clientes esperan la recuperación;
  se observan dos arranques secuenciales y como máximo un proceso vivo. Tres mutaciones
  encoladas antes del alta se ejecutan una vez por nonce.
- EOF ambiguo antes de `ok`, y EOF después de `ok` antes de stdin, conservan al dueño.
  La variante posterior retira temporalmente el socket: tres clientes reanudan sin proxies directos.
- Candado retenido sin socket y cola de conexiones llena: los clientes esperan al broker.
- Catálogo incompleto durante una escritura: la sesión activa continúa y el siguiente cliente
  conserva el mismo upstream cuando vuelve el catálogo. Dedicated y catálogo distinto permiten directo.
- Llamada de 3 s con idle configurado en 1 s: termina sobre el mismo upstream; otra alta
  no espera a esa llamada y la mutación se ejecuta una sola vez.
- Peticiones concurrentes con los mismos ids desde dos sesiones: cada respuesta vuelve a su
  sesión con su id (el fake devuelve los `params` en `echo`).
- Al cerrar una sesión la otra sigue; si el upstream muere (pasados los 500 ms) la sesión
  restante recibe EOF y el siguiente `attach` relanza.
- Daemon parado con SIGTERM, y daemon matado con SIGKILL, con una sesión viva y una petición en
  vuelo; otro daemon arranca en el mismo socket: la petición recibe -32603 «broker reiniciado»,
  la sesión contesta `ping` y habla con el upstream del daemon nuevo, sin ver la respuesta del
  `initialize` repetido.
- Daemon con `PATH=/usr/bin:/bin`: un `command` sin `/` arranca solo si el `attach` trae `path`
  con su directorio; el upstream ve ese `PATH`; el `env.PATH` del spec gana.
- `attach` con `environ`: el PATH del `environ` resuelve el comando, el upstream ve
  `SHADOW_ONLY=1` del `environ` y no ve variables del daemon; el `env.PATH` del spec gana.
- Una sesión `serve` real con `SHADOW_ONLY=1` en su entorno (y no en el del daemon): el
  upstream compartido la ve.
- SIGTERM al daemon cierra los upstreams, borra el socket y sale con 0.
- Respuesta de 20 MiB: llega íntegra. Respuesta de 300 MiB a un cliente: ese cliente recibe
  -32603 con su id, el otro cliente sigue con el mismo upstream y nada le llega.
- Daemon lanzado con `ulimit -Sn 256`: su límite blando acaba igual al duro.
- El marcador que el upstream escribe en stderr no aparece en el registro del daemon.
- Inactividad de 1 s cierra el upstream sin clientes.
- Un segundo daemon con el primero vivo sale con error y el primero sigue sirviendo.
- Un socket huérfano de un daemon muerto con SIGKILL se reemplaza.
- El registro del daemon no contiene cargas JSON-RPC.
- Versión de protocolo: dos `attach` crudos que piden `2025-11-25` y `2025-06-18` al mismo
  upstream reciben cada uno la suya; con `FAKE_MCP_MAX_PROTOCOL=2025-06-18` (el fake contesta
  `min(pedida, máxima)`), el que pide `2025-11-25` recibe `2025-06-18`. Las pruebas del `Mux`
  (`/home/someguy/codebase/0xJesus/ComandOS/crates/comandos-extensions/tests/broker_mux.rs`) cubren versión desconocida, ausente, no cadena, posterior al upstream
  y dos clientes en espera con peticiones distintas.

La suite se ejecuta desde `/home/someguy/codebase/0xJesus/ComandOS` con
`cargo test -p comandos-extensions --test broker_daemon --test broker_mux`.
Las pruebas usan HOME, sockets y servidores sintéticos aislados; sus resultados no acreditan
por sí mismos el estado de las aplicaciones externas ni la versión del daemon instalado.

## Medición histórica de memoria

El registro de memoria siguiente se conserva como medición histórica, sin repetirlo para este cambio.
Se midió con `cargo xtask rss --samples 3` sobre el binario release, upstream `fake_mcp_stdio`
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
- El entorno completo de la primera sesión determina el upstream, pero sólo el `env` del
  catálogo entra en la clave. Sesiones con distinto `environ` pueden compartir proceso.
- Un daemon vivo bloqueado, o un servidor que falla repetidamente, puede mantener un alta
  esperando hasta que el supervisor termine el cliente. No hay un plazo total de recuperación.
- Antes del primer `attach` exitoso no se lee stdin, para conservar la entrada de un eventual
  proxy directo. Por ello, cerrar sólo stdin durante esa espera no cancela el proceso fino.
  Tampoco se atiende ese EOF mientras el bucle espera reattach. Un supervisor que cierre
  stdin sin terminar el proceso puede dejar ese cliente esperando; el candado no es un recolector de huérfanos.
- Los procesos ya cargados conservan su ejecutable. Cambiar la release activa afecta a nuevos
  clientes; el daemon necesita un arranque posterior para cargar su código nuevo. Reiniciar
  una unidad con `KillMode=control-group` termina también sus upstreams y puede cortar llamadas.
