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
| ui-sounds | API de fábrica e instancia, gesto, preferencias, reproducción y cierre; 8 señales sin diferencia por muestra; 12 packs con casos representativos estéreo | Controlador | Pendiente | Pendiente |
| quick-terminal | Promise compartida, almacenamiento, reintento con el mismo id, callbacks y siguiente id; oráculo original | Controlador | Pendiente | Pendiente |
| device-drafts | Restore sin sobrescribir texto local, debounce, flush, retry y anclas; oráculo original | Controlador | Pendiente | Pendiente |
| session-config | Todos los métodos; identidad y cuentas conservadas; oráculo original | Controlador | Pendiente | Pendiente |
| workspace-layout | Todas las operaciones, constantes y errores originales; 16 fixtures | Controlador | Pendiente | Pendiente |
| push-settings | Permisos, altas/bajas/prueba/resync, rutas de eventos y botones reales; oráculo original | Controlador | Pendiente | Pendiente |

Las capturas anteriores de c8009c5 y 8d1f3ea eran HTML estático y no cargaban el WASM. Sus archivos se conservan, pero esa evidencia visual queda invalidada. La evidencia sustitutiva registra la carga del WASM y las acciones, además del fallo de la calibración negativa. Los resultados están en `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase3/docs/verification/fase3/web-b5.json`, `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase3/docs/verification/fase3/web-ui-sounds.json` y `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase3/docs/verification/fase3/web-audio-packs.json`.

No se ha desplegado, activado ni cambiado la configuración o las sesiones vivas del usuario. Las pruebas de packs cubren los kernels y canales representativos; no certifican todas las combinaciones de pack y señal.
