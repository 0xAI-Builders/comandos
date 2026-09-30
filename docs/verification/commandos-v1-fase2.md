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
