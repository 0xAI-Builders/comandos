# Paridad del proxy MCP HTTP (`comandos ext serve`) con el proxy Python

Fecha: 2026-10-04. Prueba: `crates/comandos-extensions/tests/serve_parity.rs`.

## Qué se compara

El proxy Rust (`comandos-extensions serve fake`, el mismo código que `comandos ext serve`)
y el proxy Python (`python3.11 bin/cc-extensions serve fake` → `lib/extension_proxy.py::serve_http`,
SDK `mcp` 1.30.0) reciben por stdin la misma secuencia JSON-RPC. Ambos hablan con el mismo
upstream falso streamable-HTTP escrito en Rust (`tests/support/fake_mcp_http.rs`, `hyper`).
Las cinco líneas de stdout deben ser idénticas byte a byte.

HOME temporal con un único servidor en `~/.config/comandos/extensions/catalog.json`:

```json
{"version":1,"servers":{"fake":{"enabled":true,"url":"http://127.0.0.1:<puerto>/mcp","transport":"http"}}}
```

El catálogo real del usuario no se usa. Con HOME temporal Python no ve el `mcp` instalado en
el site de usuario; la prueba enlaza (solo lectura) `~/.local/share/comandos/extensions-venv`
dentro del HOME temporal para que el fallback de `bin/cc-extensions` funcione sin modificarlo.

## Secuencia probada

| # | Mensaje | Respuesta (idéntica en ambos) |
|---|---------|-------------------------------|
| 1 | `initialize` (cliente pide `2025-06-18`) | `{"protocolVersion":"2025-06-18","capabilities":{"tools":{}},"serverInfo":{"name":"comandos-fake","version":"1"}}` |
| 2 | `notifications/initialized` | ninguna |
| 3 | `tools/list` | dos herramientas (`echo` con `inputSchema` anidado, `fail`) |
| 4 | `tools/call echo {"text":"hola ñ 日本"}` | `{"content":[{"type":"text","text":"hola ñ 日本"}],"isError":false}` (UTF-8 sin escapar) |
| 5 | `tools/call fail` | `{"content":[…],"isError":true}` |
| 6 | `prompts/list` (el upstream no lo implementa) | `{"error":{"code":-32601,"message":"Method not found"}}` |

## Resultado

- RED (antes del cambio): solo difería la respuesta 4. El upstream no manda `isError`; Python
  lo valida con `CallToolResult` (pydantic) y lo serializa con su valor por omisión `false`,
  mientras Rust reenviaba el resultado crudo.
- Corrección en `crates/comandos-extensions/src/serve.rs`: `normalize_call_tool_result` reescribe
  los resultados de `tools/call` como pydantic con `exclude_none`: campos declarados en orden
  (`_meta`, `content`, `structuredContent`, `isError`), `isError=false` si falta, extras al final
  en su orden original y sin `null` de primer nivel. Prueba unitaria con salidas tomadas del SDK.
- Corrección preventiva: las capacidades del `initialize` se emiten en el orden de
  `ServerCapabilities` de Python (`prompts`, `resources`, `tools`, `completions`). La secuencia
  no lo ejercita (el upstream solo anuncia `tools`).
- GREEN: las cinco respuestas son idénticas; la prueba pasó 5 de 5 ejecuciones seguidas (~0,6 s).

## Detalles de protocolo que necesitó el upstream falso

- `POST /mcp` con petición: una respuesta `Content-Type: application/json`.
- `POST /mcp` con notificación: `202 Accepted`, cuerpo vacío (el SDK de Python acepta 202 y
  descarta el cuerpo; Rust solo exige estado 2xx).
- `initialize` devuelve `Mcp-Session-Id: fake-session`; ambos clientes lo reenvían en todas las
  peticiones siguientes, y la prueba lo comprueba en el registro del falso.
- `GET /mcp`: tras `notifications/initialized` el SDK de Python abre un stream SSE para mensajes
  iniciados por el servidor. El falso responde `405`; Python lo registra, reintenta tras 1 s y
  desiste sin afectar la sesión. Rust no hace este GET.
- `DELETE /mcp`: ambos clientes cierran la sesión al terminar; el falso responde `200`.
- Ambos envían `Accept: application/json, text/event-stream`; el falso no lo inspecciona.
- El puerto se reserva con `bind(127.0.0.1:0)` dentro del propio falso antes de lanzar los
  proxies, sin ventana de carrera.

Registro del upstream por cliente en una ejecución:

- Rust: `POST initialize`, `POST notifications/initialized`, `POST tools/list`, `POST tools/call` ×2,
  `POST prompts/list`, `DELETE`.
- Python: lo mismo más un `GET` (405) después de `notifications/initialized`.

## Memoria (Pss/RSS)

Medido con `cargo xtask rss --samples 3`, mismo HOME temporal y el upstream falso vivo
(`COMANDOS_PARITY_HOME=<dir> cargo test -p comandos-extensions --test serve_parity -- --ignored
hold_fake_upstream_for_rss`). `xtask` mantiene abierto el stdin del proxy y mide a los 2 s, es
decir, con la sesión upstream ya inicializada y el proxy en reposo. El árbol medido es solo el
proceso del proxy (1 proceso en ambos casos; el re-exec de Python al venv conserva el PID).
Binario Rust en release (`cargo build --release -p comandos-cli`).

| Proxy | Pss mediana | RSS mediana |
|-------|-------------|-------------|
| Python (`python3.11 bin/cc-extensions serve fake`) | 45 388 KiB | 57 776 KiB |
| Rust (`comandos ext serve fake`) | 5 605 KiB | 7 596 KiB |

Unas 8 veces menos Pss por proxy. Filas en `docs/verification/rss.jsonl`.

## Límites conocidos

- Python reserializa todas las respuestas con modelos pydantic; Rust reenvía el JSON del upstream
  salvo en `initialize` y, ahora, en `tools/call`. Si un upstream ordena las claves de una
  herramienta de otra forma que `Tool` (`name`, `title`, `description`, `inputSchema`, …), manda
  `null` en campos opcionales o en bloques de contenido, la salida diferiría. La secuencia usa el
  orden del modelo.
- `protocolVersion` del `initialize`: Python responde con la versión que pide el cliente si la
  soporta; Rust responde con la del upstream. Coinciden aquí porque ambas son `2025-06-18`.

## Servidores stdio (`serve` hace `exec`)

`crates/comandos-extensions/tests/serve_stdio_parity.rs` compara `serve <stdio>` con
`python3.11 bin/cc-extensions serve <stdio>` usando un binario de prueba
(`src/bin/env_dump.rs`) que vuelca cwd, argv y las variables `COMANDOS_*`/`HOME`.

Lo que hace el Python (`lib/extension_proxy.py::serve`): `chdir(expanduser(cwd))`,
`command=expanduser(command)`, stderr a `/dev/null` (`dup2`) y
`os.execvpe(command, [command, *args], resolved_env(spec))`, con
`resolved_env = os.environ + {k: expandvars(str(v))}`. Solo cuenta como stdio un servidor con
`command` sin `enabled_tools`/`disabled_tools`; con filtros pasa por el proxy MCP.

Resultado: el Rust (`cli.rs` → `command(spec, true).exec()`) ya era idéntico en volcado
byte a byte (cwd con `~`, argv[0] expandido, `${VAR}`/`$VAR`/variable inexistente, valores
numéricos y booleanos, herencia del entorno del padre, búsqueda en `PATH`), stderr vacío y
sin proceso intermedio (el pid que reporta el comando es el del hijo lanzado). Única
diferencia hallada y corregida: servidor inexistente o deshabilitado imprimía
`Server unavailable` y el Python `Server unavailable: <nombre>`. Un comando inexistente
termina con el mismo código y sin salida en ambos.

`status` coincide byte a byte. `count` no existe como en el brief: en el Rust es la orden
interna del tokenizador (`tokenizer::count_command`), sin equivalente Python, y no se compara.

### Ronda de corrección de stdio (verdad de Python y casos límite)

La decisión «exec directo o proxy MCP» y el entorno ahora siguen la verdad de Python
(`py_truthy` en `lib.rs`, usada por `cli::server_spec` y `broker::direct_stdio`):

- `enabled` falso (`false`, `0`, `null`, `""`) o servidor vacío `{}` ⇒ `Server unavailable: <nombre>`.
- `command` vacío o no cadena ⇒ proxy, no `exec`; `disabled_tools` verdadero no array (p. ej. una
  cadena) ⇒ proxy; `enabled_tools` presente (incluso `null`) ⇒ proxy.
- `cwd` vacío se omite. `cwd` inexistente: el Python falla en `chdir` y su envoltorio imprime
  `Extension operation failed: FileNotFoundError` (exit 1); Rust hace `chdir` él mismo antes del
  `exec` e imprime la misma línea (`PermissionError`/`NotADirectoryError`/`OSError` según errno).
- `env` con listas/diccionarios: `str()` de Python da el `repr` (`['a', 1, None, True, "it's"]`,
  `{'k': [1.5, {'z': False}]}`); los `float` usan el `repr` de Python (`1e10` ⇒ `10000000000.0`,
  `1e-5` ⇒ `1e-05`, `1e16` ⇒ `1e+16`); `false`/`null` ⇒ `False`/`None`.
- `env.PATH` propio: el comando se busca con el `PATH` nuevo en ambos (caso `pathenv`).

Comando inexistente: stderr queda vacío en ambos aunque `main` imprima «Extension command failed»,
porque `Command::exec` hace el `dup2` de stderr a `/dev/null` en el propio proceso antes del `execvp`
fallido (Python también redirige antes). Mismo código de salida (1).

Diferencia conocida, fuera del `exec`: en el camino proxy con stdin cerrado antes de que el upstream
inicialice (caso `filtered`/`dstring`/`emptycmd` con un hijo que no habla MCP), Python sale con 1
(`Extension operation failed: ExceptionGroup`) y Rust con 0; la prueba solo exige rama proxy (sin
volcado del comando) y stdout igual.
