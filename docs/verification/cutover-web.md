# Procedimiento de port por componente

1. Ejecutar `cargo run -p xtask -- web-port pin <id>` contra el origen actual.
2. Leer el inventario en `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase3/xtask/web/inventory.json` y las exportaciones en `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase3/xtask/web/interop.json`.
3. Cuando el componente genere HTML, portar la plantilla y comparar el DOM normalizado con el HTML original. Las hojas puras conservan sus consumidores y su HTML existente.
4. Implementar las exportaciones y los manejadores en Rust. Compartir los globales mutables con el JS restante mediante propiedades de `window`. Un objeto con nombres de métodos, sin funciones ejecutables, no cuenta como port.
5. Ejecutar los mismos casos del original sobre las exportaciones del WASM compilado y sobre el JS original. Conservar el oráculo y registrar la URL exacta del WASM.
6. Crear o actualizar la declaración del componente, con origen, hash, exportaciones y dependencias.
7. Capturar los estados producidos por acciones en `chrome-bg` del Mac mini. Antes de aceptar una comparación verde, sustituir o retirar una función del candidato y comprobar que el arnés falla. Comprobar anchos 1400, 844, 390 y 320, DPR 1 y 2, y estilos de escritorio frente a remoto.
8. Probar también el arranque del frente real, incluido el cargador roto y el regreso único a `?web=off`. Las fixtures de exportaciones no prueban la compuerta del frente.
9. El controlador ejecuta la sombra de 24 horas en teléfono y escritorio contra 4782, con `--shadow-readonly`.
10. Activar con `comandos web set <id> on`; revertir con `comandos web set <id> off`.

La comparación reproducible usa `cargo run -p xtask -- web-bench behavior-diff --base <URL_PRIVADA>` y `web-bench audio-diff --base <URL_PRIVADA>`. Los datos de prueba y el servidor no requieren cuentas, servicios push reales ni una instancia del tablero vivo. El indicador `COMANDOS_DASH_TEST_HOOKS` solo habilita el renderizador de prueba cuando la fixture lo establece antes del arranque; no se instala ese global en la página normal.

## Estado del lote B4/B5, 2026-10-05

| Componente | Implementación y fixtures remotas | Frente real | Sombra | Activación / release |
| --- | --- | --- | --- | --- |
| ui-sounds | API de fábrica e instancia, gesto, preferencias, reproducción y cierre; 8 señales sin diferencia por muestra; 12 packs con casos representativos estéreo | DOM real verificado; compuerta del controlador separada | Pendiente | Pendiente |
| quick-terminal | Promise compartida, almacenamiento, reintento con el mismo id, callbacks y siguiente id; oráculo original | DOM real verificado; compuerta del controlador separada | Pendiente | Pendiente |
| device-drafts | Restore sin sobrescribir texto local, debounce, flush, retry y anclas UTF-16 sin pérdida; oráculo original | DOM real verificado; compuerta del controlador separada | Pendiente | Pendiente |
| session-config | Todos los métodos; identidad y cuentas conservadas; oráculo original | DOM real verificado; compuerta del controlador separada | Pendiente | Pendiente |
| workspace-layout | Todas las operaciones, constantes y errores originales; 16 fixtures | DOM real verificado; compuerta del controlador separada | Pendiente | Pendiente |
| push-settings | Permisos, altas/bajas/prueba/resync, rutas de eventos y botones reales; oráculo original | DOM real verificado; compuerta del controlador separada | Pendiente | Pendiente |

Las capturas anteriores de c8009c5 y 8d1f3ea eran HTML estático y no cargaban el WASM. Sus archivos se conservan, pero esa evidencia visual queda invalidada. Las 48 capturas posteriores de encabezados y JSON también quedan excluidas de la aceptación de DOM del producto: solo presentan resultados de APIs. Conservan valor como fixtures de API. La aceptación visual requiere los controles, estados, renderizadores y manejadores reales de index.html, pomodoro.js, workspace.js y term.html, con prueba del WASM cargado y calibración negativa sobre ese DOM. Los resultados están en `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase3/docs/verification/fase3/web-b5.json`, `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase3/docs/verification/fase3/web-ui-sounds.json` y `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase3/docs/verification/fase3/web-audio-packs.json`.

No se ha desplegado, activado ni cambiado la configuración o las sesiones vivas del usuario. Las pruebas de packs cubren los kernels y canales representativos; no certifican todas las combinaciones de pack y señal.

El lote reparado usa WASM `4d3c40ac1992`: 44 grupos de comportamiento coinciden con el JS original; el probe de ancla conserva la unidad UTF-16 55357 en el límite 300 y encuentra el índice 0. El cierre resuelve las promesas de reproducciones activas y detenidas, y retira sus callbacks. El probe Node sobre el WASM anterior falla; el reparado pasa. Chrome remoto confirma ambos casos sobre el WASM real. La evidencia está en `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase3/docs/verification/fase3/web-parity-fixes.json`.

La aceptación de DOM contiene 80 recortes de controles y estados de producción, en los cuatro anchos y ambos DPR, todos con diferencia de píxeles 0. Cada fila registra el original explícito `web=off` sin recurso WASM y el candidato `web=shadow` con el recurso 4d3c. Incluye Pomodoro, ajustes push habilitados/deshabilitados, editor de perfil, workspace, error de terminal y compositor del borrador. Dos recortes del editor detectan la retirada intencional de opciones del candidato. Ocho comparaciones adicionales de estilos (push y editor) pasan con la misma selección explícita de original/candidato. El reporte completo está en `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase3/docs/verification/fase3/web-product-dom.json`.

El primer barrido conservado incluye una diferencia del contenedor entero de toasts por un aviso de sondeo de fondo «Not Found». Se conserva ese resultado; los ocho recortes aceptados de terminal se repitieron sobre el toast real del error de terminal. No se excluye ninguna diferencia de ese toast. El compositor de borrador usa una fixture aislada que sirve literalmente `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase3/dash/term.html` y sus recursos, con entradas HTTP/WebSocket falsas y cero conexión PTY. Esto prueba el consumidor de producción, no un terminal nativo conectado.

Para repetir, el controlador debe mantener el frente privado aislado en 7321 y exponerlo a `chrome-bg`. Ejecutar `cargo run -p xtask -- shots pair --base http://127.0.0.1:7321 --suite b5-product` y `--suite product-quick`, y `shots remote-vs-desktop` con `product-session-remote` y `product-push-enabled-remote`. El compositor usa el servidor `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase3/xtask/web/fixtures/sounds/serve-product.py` en 7318 y la suite `b5-product-drafts` contra ese puerto. Los scripts y JSON de suites están bajo `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase3/xtask/web/`.

La prueba separada de la compuerta HEAD y del cargador roto pertenece al controlador: `/home/someguy/codebase/0xJesus/ComandOS/.scratch/takeover-20261005/web-gate/report.json`. La procedencia de la actualización a 4d3c está en `/home/someguy/codebase/0xJesus/ComandOS/.scratch/takeover-20261005/web-gate/repair-provenance.json`. El parche HTTP revisado no se aplica en este lote. Siguen pendientes la sombra de 24 horas en teléfono/escritorio, el terminal nativo conectado, escucha humana de audio, activación y release.
