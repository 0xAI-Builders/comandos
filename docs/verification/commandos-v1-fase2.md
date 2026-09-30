# CommandOS v1 · fase 2 — verificación V1 (barra de comandos y cabecera)

Fecha: 2026-09-30. Rama `implementation/comandos-v1-fase2` fusionada en `main`
(FF `0561687..e837350`) y activada con `install.sh` + reinicio de `cc-dash`
+ relanzamiento del GTK (`comandos-relaunch-1790794727`) conservando las 17
sesiones tmux vivas. Navegador: MCP `chrome-bg` en la Mac mini contra
`http://127.0.0.1:4777` (`cc-browser-expose start 4777`) con
`window.__COMANDOS_DEV_WEBTERM = true` para el modo remoto. ttyd (4780) no se
reenvía, así que el iframe de terminal sale "modo degradado" y `/term/token`
da 404: infraestructura del reenvío, no de esta fase.

## Recorrido y resultado

| # | Comprobación | Resultado |
|---|---|---|
| 1 | Cabecera Ordenada: dos filas, orden ☰ · esp/list/trab · terminal · nueva sesión · saltar · snippets · analytics · remoto · servidores · resúmenes · campana · pomodoro · ajustes · reloj | OK (`fase2-desktop-barra.jpg`) |
| 2 | ☰ abre el menú (tema, sonido, límites, resumen, perfiles, uso de extensiones) y se cierra al repetir | OK |
| 3 | Servidores abre `#ssh-bar` sin cambios (18 servidores, `gestionar`) y lo cierra al repetir; el código de la barra SSH es byte-idéntico | OK (`fase2-desktop-servidores.jpg`) |
| 4 | Campana abre el cajón `#notices` con grupos y lo cierra | OK |
| 5 | Resúmenes abre `#news-reader` y lo cierra | OK |
| 6 | Snippets abre su popover | OK |
| 7 | Barra: Cadenas guardadas + acordeón por CLI (Claude 2.1.285 "sin verificar", Codex 0.159.2, Grok, OpenCode, Antigravity con versiones del watcher), yolo primero, todo plegable, chips "nuevo" para modelos recién detectados | OK |
| 8 | CLI del pane seleccionado marcado "en este pane" y abierto solo (con estado abierto persistido): con todo plegado, elegir Merauto (claude) abre solo Claude + Arranque yolo; elegir Signara (codex) marca Codex | OK tras el fix `461ee03` |
| 9 | Un clic teclea letra por letra sin Enter: `claude --dangerously-skip-permissions` quedó en el prompt de la terminal rápida (tmux `capture-pane`) sin ejecutarse | OK |
| 10 | Terminal rápida desde la barra: sesión `term-q…`, carpeta `~/codebase/0xJesus/Terminal/T-2026-09-30-13-06-14`, destino `… · %49` | OK |
| 11 | Cadena: modal Cadenas con el mismo acordeón; clic en fila o en «+ cadena» añade paso; guardar crea `~/.config/comandos/cadenas/prueba-v1-codex.md`; Correr fija el destino y «Siguiente» avanza 1→2; Escape cierra | OK |
| 12 | Selección de pestañas no mata terminales rápidas (`tmux has-session` con nombre entre comillas: vivas) | OK — el "no encontrada" anterior era la expansión `=palabra` de zsh |
| 13 | Consola sin errores JS propios (solo 404 de `/term/token`, ver arriba) | OK |
| 14 | Móvil 390×844: sin scroll horizontal, cabecera en tres filas, barra completa en el cajón | OK (`fase2-mobile-390.jpg`) |
| 15 | Rutas: `/commands/catalog`, `/pane/type`, `/chains`, `/operator*` → 410 | OK |

## Cambios hechos durante la verificación

- `461ee03` — con estado abierto persistido, el CLI detectado en el pane no se
  abría al cambiar de destino; ahora se abre de forma aditiva (nunca pliega).

## Pendiente fuera de esta fase

- Reenviar ttyd cuando se verifique desde la Mac (o probar desde el GTK).
- La cadena `prueba-v1-codex` y las carpetas `T-2026-09-30-13-0{1-34,4-12,6-14}`
  son restos de esta prueba; se borran salvo que Jesús quiera conservarlas.

## Corrección de estilos (30-sep, tarde)

Jesús exigió respetar 100 % los estilos de los prototipos aprobados. Comparación
visual lado a lado en la Mac mini (mockup `prototype-v2-barra.html` sin parámetros
y `?round=cli` para el modal, contra `http://127.0.0.1:4777`):

| Mockup aprobado | Implementación (9e20e0b) |
|---|---|
| `shots/fase2-mockup-barra.jpg` | `shots/fase2-estilos-barra.jpg` |
| `shots/fase2-mockup-modal.jpg` | `shots/fase2-estilos-modal.jpg` |

Portado valor por valor (paleta, radios, tamaños, píldoras): cabecera con libro,
destino y píldora de catálogo; botón Cadenas violeta; búsqueda con lupa; tarjeta de
cadenas guardadas con Correr; fila de CLI con monograma, versión y badge; arranques
como píldoras (yolo ámbar sobre violeta oscuro); filas de dos líneas con chips en
línea; tarjeta de cadena corriendo; pestañas de terminales rápidas. Modal: cabecera
con búsqueda y «escribe en», tablero de CLI a lo ancho con comandos en tres columnas
como tarjetas con «+ cadena» y asa, barra de ranuras con seis huecos, Nombre,
Guardar y Correr, nota al pie. Lista plana por CLI (ronda 6 A) y solo comandos
presentes en el binario. En el escritorio el modal es una ventana GTK centrada.
Fuera de alcance de esta pasada: la cabecera superior (botones 3D aprobados en el
grill de fase 1, que trabaja la otra sesión).

## Comandos tal como los lista cada CLI (30-sep, noche)

Jesús pidió la lista de comandos con las descripciones directas de cada CLI, sin
categorías propias. Cada CLI se abrió en un tmux privado (`-L cs-probe`, carpetas ya
confiadas, sin Enter) y se leyó su propio menú `/` por prefijos
(`tools/cli-commands/scrape_prefix.py`); en Claude, donde el buscador es difuso, se
tecleó cada nombre y se leyó su fila exacta (`verify_names.py`). Quedan solo los
comandos de fábrica: la descripción debe existir en el binario
(`builtin_filter.py`), así se excluyen skills y plugins del usuario.

| CLI | Versión | Comandos |
|---|---|---|
| Claude Code | 2.1.286 | 99 |
| Codex | 0.159.2 | 55 |
| Grok | 1.0.44 | 88 (`/m` es alias de `/model`, `/t` de `/theme`) |
| OpenCode | 1.18.33 | 17 |
| Antigravity | 1.2.14 | 42 (las descripciones largas llegan con «...» como las muestra el CLI) |

Las listas crudas viven en `tools/cli-commands/scraped/`; `tools/cli-commands/build.py`
genera `config/cli-commands.json` (una lista plana por CLI, orden alfabético, texto en
inglés tal cual). Lo único añadido son los chips de argumentos ya verificados de
`/model` y `/effort`, y se quita el estado de sesión capturado («(currently …)»). La
detección en el binario acepta el nombre con o sin barra, porque Codex (Rust) y
Antigravity (Go) lo guardan sin ella. En la barra, las descripciones largas se cortan
a dos líneas y la completa queda en el `title`.

## Arranques desde `<cli> --help` (30-sep, noche)

Los bloques «Arrancar en modo yolo · sin permisos» y «Arrancar normal» eran texto
nuestro y los flags salían de `agent-roles.json` + `launch.extra` del catálogo. Con
eso la barra ofrecía `codex --full-auto`, que Codex 0.159.2 ya no tiene, y decía que
OpenCode no tiene flag yolo cuando `opencode --help` trae `--auto`.

Ahora `lib/cli_help.py` ejecuta `<cli> --help` (cacheado por ruta, mtime y tamaño del
ejecutable; el watcher lo calienta) y lo interpreta (commander, clap, yargs y flag de
Go). Por CLI la barra muestra, con el texto exacto de la ayuda:

- el binario solo, con el primer párrafo de su ayuda, y las cuentas reales;
- en ámbar, los flags cuya propia descripción dice que se salta permisos o
  confirmaciones: `claude --allow-dangerously-skip-permissions` y
  `--dangerously-skip-permissions`, `codex --dangerously-bypass-approvals-and-sandbox`,
  `grok --always-approve`, `opencode --auto`, `agy --dangerously-skip-permissions`;
- cada sección de la ayuda con su título original («Options», «Commands»,
  «Available subcommands»), plegada, con `codex --help` como fuente y los valores
  posibles como chips (`--sandbox read-only|workspace-write|danger-full-access`,
  `--ask-for-approval on-request|never`, `--permission-mode …`).

Todos los CLI arrancan cerrados, y las secciones de la ayuda también. Las ayudas
reales quedan como fixtures en `tests/fixtures/cli-help/`.
