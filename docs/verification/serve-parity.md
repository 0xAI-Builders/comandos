# Paridad del proxy MCP HTTP (`comandos ext serve`) con el proxy Python

Fecha: 2026-10-04 (revisión 1 tras la revisión de código). Prueba:
`crates/comandos-extensions/tests/serve_parity.rs`.

## Qué se compara

El proxy Rust (`comandos-extensions serve fake`, el mismo código que `comandos ext serve`)
y el proxy Python (`python3.11 bin/cc-extensions serve fake` → `lib/extension_proxy.py::serve_http`)
reciben por stdin la misma secuencia JSON-RPC. El oráculo corre en el venv de extensiones
(`mcp` 1.30.0, pydantic 2.13.5, pydantic-core 2.46.5). Ambos hablan con el mismo upstream
falso streamable-HTTP escrito en Rust (`tests/support/fake_mcp_http.rs`, `hyper`). Las líneas
de stdout deben ser idénticas byte a byte.

HOME temporal con un único servidor en `~/.config/comandos/extensions/catalog.json`:

```json
{"version":1,"servers":{"fake":{"enabled":true,"url":"http://127.0.0.1:<puerto>/mcp","transport":"http"}}}
```

El catálogo real del usuario no se usa. Con HOME temporal Python no ve el `mcp` instalado en
el site de usuario; la prueba enlaza (solo lectura) `~/.local/share/comandos/extensions-venv`
dentro del HOME temporal para que el fallback de `bin/cc-extensions` funcione sin modificarlo.
El HOME se borra al terminar aunque la prueba falle (guarda con `Drop`).

## Por qué el Rust reescribe las respuestas

El Python no reenvía bytes: valida cada resultado del upstream con su modelo pydantic y lo
vuelve a serializar con `model_dump_json(by_alias=True, exclude_none=True)`. Para igualarlo,
`src/serve/normalize.rs` describe los modelos de `mcp/types.py` (lista de campos sacada por
introspección de `model_fields`) y aplica, por nivel de modelo:

- campos declarados en orden de declaración y extras al final en su orden original
  (todos los modelos son `extra="allow"`);
- `null` omitido solo en campos del modelo y en extras de ese nivel; dentro de
  `dict[str, Any]` (`inputSchema`, `outputSchema`, `structuredContent`, `_meta`, `data`) se
  conserva (`"default": null` de un esquema sobrevive);
- `CallToolResult.isError` con su valor por omisión `false`;
- `AnyUrl` normalizado (`https://Example.COM/a/../b` → `https://example.com/b`);
- `Annotations.priority` (`float`): un entero sale `1.0`;
- números decimales reformateados como pydantic-core (`1.50` → `1.5`, `1e3` → `1000.0`,
  `1e16` → `1e+16`);
- un resultado que no valida (por ejemplo `tools/call` sin `content`) se convierte en
  `{"code":-32603,"message":"Upstream request failed for <nombre>: ValidationError"}`;
- el sobre JSON-RPC se reconstruye (`jsonrpc`, `id`, `result`/`error`) y el `error` del
  upstream pasa por `ErrorData` (`code`, `message`, `data` sin `null`).

Modelos cubiertos: `ListToolsResult`/`Tool`/`ToolAnnotations`/`ToolExecution`/`Icon`,
`CallToolResult` con `ContentBlock` (`TextContent`, `ImageContent`, `AudioContent`,
`ResourceLink`, `EmbeddedResource`) y `Annotations`, `ListResourcesResult`/`Resource`,
`ListResourceTemplatesResult`/`ResourceTemplate`, `ReadResourceResult` con
`TextResourceContents`/`BlobResourceContents`, `ListPromptsResult`/`Prompt`/`PromptArgument`,
`GetPromptResult`/`PromptMessage`, `CompleteResult`/`Completion` y `ErrorData`.

`initialize` también sigue al Python: hacia el upstream pide `2025-11-25`
(`LATEST_PROTOCOL_VERSION`) y aborta si la versión que le contestan no está en
`SUPPORTED_PROTOCOL_VERSIONS` (`2024-11-05`, `2025-03-26`, `2025-06-18`, `2025-11-25`); hacia el
cliente contesta la versión que el cliente pidió si está en esa lista y, si no, la última.
`instructions` solo se incluye si el upstream manda una cadena.

## Secuencias probadas

`rust_proxy_matches_python_proxy` (cliente pide `2025-06-18`; el falso contesta la versión que le
piden y ambos proxies le piden `2025-11-25`):

| # | Mensaje | Qué ejercita |
|---|---------|--------------|
| 1 | `initialize` | negociación hacia el cliente (`2025-06-18`), `instructions: null` del upstream omitido |
| 2 | `notifications/initialized` | sin respuesta |
| 3 | `tools/list` | sobre desordenado, `title`/`annotations`/extra `null` omitidos, `default: null`, `1.50` y `1e16` en el esquema, `nextCursor: null` |
| 4 | `tools/call echo` | bloque con `annotations`/`_meta` `null`, `structuredContent: null`, `isError=false`, UTF-8 sin escapar |
| 5 | `tools/call fail` | `isError: true` primero en el upstream, `priority: 1` → `1.0`, `structuredContent` con `null` y `1e3` |
| 6 | `prompts/list` | error del upstream con claves desordenadas y `data: null` |
| 7 | `tools/call bare` | resultado sin `content` → `-32603 … ValidationError` |
| 8 | `resources/list` | `AnyUrl` (`HTTPS://Example.COM/docs/../a b` → `https://example.com/a%20b`), `title: null` |
| 9 | `prompts/get` | `PromptMessage` y bloque de contenido con `null` |

`old_client_version_is_negotiated_like_python`: `initialize` pidiendo `2024-11-05` (ambos
contestan `2024-11-05`) y `tools/list`.

`unsupported_upstream_version_fails_like_python`: el upstream contesta `1999-01-01`. Ambos salen
con código 1, stdout vacío y en stderr `Extension operation failed: ExceptionGroup`.

`a_burst_of_ready_responses_is_all_delivered` (solo Rust): 20 `tools/call` contra
`/mcp-burst`, que retiene las respuestas y las suelta juntas. Salen las 20 y el proxy contesta
un `ping` después. La cola de salida (8) usa `send().await`: contrapresión como el stream de
anyio del Python. Con el `try_send` anterior, la 9.ª respuesta lista en el mismo instante
mataba el proxy con «Downstream output limit reached».

## Resultado

- Ronda inicial: solo difería `isError:false` en `tools/call`.
- Revisión 1, RED con el `serve.rs` anterior contra la secuencia ampliada: fallan las tres
  pruebas (versión `2025-03-26` en vez de la del cliente, `instructions: null` reenviado, sobre y
  claves del upstream sin reordenar, `null` sin quitar, `priority: 1`, `{"isError":false}` en vez
  del `-32603`, URL sin normalizar, y con versión no soportada el Rust seguía sirviendo).
- GREEN: las tres pruebas pasan; 3 de 3 ejecuciones seguidas (~0,7 s).

## Detalles de protocolo que necesitó el upstream falso

- `POST /mcp` con petición: una respuesta `Content-Type: application/json`.
- `POST /mcp` con notificación: `202 Accepted`, cuerpo vacío.
- `initialize` devuelve `Mcp-Session-Id: fake-session`; ambos clientes lo reenvían, y la prueba
  lo comprueba en el registro del falso junto con la versión pedida (`2025-11-25` en ambos).
- `GET /mcp`: tras `notifications/initialized` el SDK de Python abre un stream SSE. El falso
  responde `405`; Python reintenta tras 1 s y desiste. Rust no hace este GET.
- `DELETE /mcp`: ambos cierran la sesión al terminar; el falso responde `200`.
- `/mcp-badversion`: mismo servidor con una versión de protocolo no soportada.
- Las respuestas se escriben como texto crudo (no con `json!`) para poder mandar claves
  desordenadas y grafías numéricas que pydantic reescribe.
- El puerto se reserva con `bind(127.0.0.1:0)` dentro del propio falso, sin ventana de carrera.

## Tamaño de las respuestas

El tope de 8 MiB por respuesta del upstream desaparece: el Python no pone límite y upstreams
reales (capturas de chrome-bg) lo superan. Queda `MAX_RESPONSE = 256 MiB`
(`src/transport.rs`) para cuerpo HTTP, evento SSE y línea stdio del upstream, solo como defensa
ante un upstream roto. Los mensajes del cliente siguen limitados a 8 MiB (`MAX_MESSAGE`).

## Memoria (Pss/RSS)

Medido con `cargo xtask rss --samples 3`, mismo HOME temporal y el upstream falso vivo
(`COMANDOS_PARITY_HOME=<dir> cargo test -p comandos-extensions --test serve_parity -- --ignored
hold_fake_upstream_for_rss`), con `.build/target/release` en `PATH` para lanzar `comandos` por
nombre. `xtask` mantiene abierto el stdin del proxy y mide a los 2 s, con la sesión upstream ya
inicializada y el proxy en reposo. El árbol medido es solo el proceso del proxy (1 proceso en
ambos casos; el re-exec de Python al venv conserva el PID). Binario Rust en release
(`cargo build --release -p comandos-cli`), con la normalización de esta revisión.

| Proxy | Pss mediana | RSS mediana |
|-------|-------------|-------------|
| Python (`python3.11 bin/cc-extensions serve fake`) | 45 349 KiB | 57 764 KiB |
| Rust (`comandos ext serve fake`) | 5 929 KiB | 7 920 KiB |

Unas 7,6 veces menos Pss por proxy. Filas en `docs/verification/rss.jsonl`.

## Límites conocidos

- La validación replica presencia, `null`, tipos estructurales (objeto, lista, cadena, literal,
  URL) y uniones; no replica las coerciones laxas de pydantic en escalares (por ejemplo
  `"isError": 1` → `true`, `size: 3.0` → `3`, `"priority": "0.5"`), que el Rust reenvía tal cual.
- En una unión `TextResourceContents | BlobResourceContents` con `text` y `blob` a la vez se
  elige la primera variante; pydantic podría elegir otra en modo «smart».
- Los fallos de transporte siguen saliendo como `-32603 "Upstream request failed"`; el Python
  añade `for <nombre>: <clase de excepción>`.
- El formato de los decimales coincide con el pydantic-core 2.46.5 del venv (`1e+16`); el
  pydantic-core 2.33 del site de usuario escribe `1e16`. El oráculo es el venv.
