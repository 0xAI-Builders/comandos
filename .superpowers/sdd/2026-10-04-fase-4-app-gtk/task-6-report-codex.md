# T6: widget terminal integrado

TermView implementa un DrawingArea GTK real sobre TermEngine y PtySession. El constructor exige que GTK ya esté inicializado en el hilo principal; no inicializa un display. Sandbox requiere clear_env. TermOptions recibe un snapshot de entorno y tamaño tmux, callbacks de título/salida/timbre/enlace/SSH y before_spawn para recuperación. El propietario debe conservar TermView mientras el widget esté instalado.

IO usa unix_fd_add_local: IN/HUP/ERR drena hasta WouldBlock con presupuesto de 32×8192 bytes por despacho; si HUP trae más de ese presupuesto, conserva la fuente hasta terminar el drenaje. Las respuestas del motor vuelven al PTY y OUT existe solo mientras haya bytes pendientes. Los callbacks de título, campana y OSC52 se ejecutan después de liberar el préstamo del modelo. Errores de entrada se devuelven por feed/paste o se muestran mediante callback/diagnóstico en los manejadores internos.

La asignación espera quiet 250 ms/cap 1500 ms; mide Pango antes del primer attach y usa tamaño existente cuando GTK aún mide 1 px. Respawn usa 800 ms. El timer es de un solo disparo y se rearma por daño, settle, synced-update y SSH, con sondeo de salida de 100 ms; no corre a 125 Hz cuando está inactivo. Destroy y Drop eliminan todas las fuentes y sueltan solamente el PTY propio. Las closures capturan Weak, sin ciclos de Rc.

Cairo/Pango ejecuta Rect/Text/Glyph/Line y cada variante DrawOp mediante match exhaustivo. Las letras se colocan en sus celdas; glyphs convierten DPR una sola vez. Hay subrayados simple/doble/curvo/punteado/discontinuo, tachado, cursor sólido con texto de contraste/beam/underline/outline, selección, texto IME y scrollback. FrameCache conserva órdenes y cursor del frame anterior durante DEC2026: ni exposición ni blink pueden mostrar una actualización parcial. El gate nativo compara el frame antes/durante/después del timeout.

IMMulticontext maneja preedit/commit y filtra el evento antes del encoder; no envía de nuevo un key-press consumido. El mapping GDK cubre navegación, funciones, números, puntuación, keypad y modificadores. Se reutilizan encode_key/mouse/wheel/focus/paste y select/find_urls. Selección simple/word/line/block copia al clipboard/primary; middle paste respeta mouse reporting, enlaces usan callback y hover pointer. Los helpers Python de URL/ruta envuelta están portados con unicode y coordenadas, y se usan como fallback tras OSC8/URLs nativos. DND pega texto/rutas shell-quoted sin ejecutarlas. SSH conserva agrupación 45 ms, delta ±24, smooth ±8 y redondeo Python; el adaptador remoto recibe callback, sin ejecutar ssh aquí.

Preferencias contrastadas con el oráculo: familia con fallbacks, tamaño en puntos, interlineado 1.2, terminal_padding por defecto 8 (0–40), HEADER_EXTRA 14, terminal_opacity 30–100 %, cursor y fuente/escala modificables. No se usó un mock del widget para declarar paridad.

## Evidencia nativa

RED: term_view_model no compilaba por keys/links/schedule ausentes; FrameCache no existía; Ctrl+[ devolvía None en vez de ESC. GREEN: 37 pruebas (lib 3, term_view_model 5, term_paint 18 y term_pty 11). Además de modelos, Cairo ImageSurface sin servidor gráfico verifica DPR una sola vez y que el subrayado curvo tenga raster distinto del simple. Pruebas PTY siguen confinadas al shell sintético y tmux privado. Cargo check pasa y clippy --workspace --all-targets -D warnings pasa; fmt/diff-check limpios. Todo con -j2, offline y /home/someguy/codebase/0xJesus/ComandOS/.build/target-fase4.

## Puertas remotas pendientes de T12

No se ejecutó GTK, GTK init, gtk_smoke, Xvfb ni navegador local. Estas pruebas nativas no prueban fidelidad visual ni señales GTK reales. Se entregan los casos para el runtime Linux GTK3/WebKitGTK aislado accesible desde Mac; Chrome/macOS por sí solo no basta:

- ime_commit_reaches_pty: componer ñ/漢字, verificar exactamente una copia de bytes en el PTY sintético y ningún key-press duplicado.
- terminal_mouse_and_focus: habilitar modos mouse/SGR/focus en la fixture, verificar coordenadas/bytes y selección/clipboard/links/DND cuando no hay reporting.
- cursor_blink_stops_when_unfocused: comparar capturas antes/después de 600 ms, blur y DEC2026; cursor y frame deben conservarse durante sync.
- term_fd_sources_drop: destruir el widget con read y write pendientes, verificar cero callbacks posteriores, cero respawns, cliente retirado y servidor/sesión privada vivos.
- fuente/márgenes/DPR: comparar Pango/fallbacks, 163×44 antes del primer attach, geom y capturas de glyphs/selection/cursor contra VTE.

## Cobertura de funciones para app-drift

make_term se reparte en TermView::new/configure_font/spawn/connect_events/draw. _bg_rgba y _term_line_scale corresponden a preferencias/pintura. _spawn_when_settled e _tmux_window_size consumen los contratos T5/T3, sin reimplementarlos. normalize_wrapped_url_text y url_from_wrapped_text son APIs con sus nombres originales; cell_at_event/terminal_grid_at_event se adaptan en point/absolute. on_ssh_scroll, _ssh_scroll_amount y _flush_ssh_scroll se conectan a coalescing/callback; feed se expone en TermView::feed. Apertura de archivos/URLs y popovers con efectos corresponden a T14, no se simulan ni se ejecutan desde este widget.
