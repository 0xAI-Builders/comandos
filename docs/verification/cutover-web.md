# Procedimiento de port por componente

1. Ejecutar `cargo run -p xtask -- web-port pin <id>` contra el origen actual.
2. Leer `xtask/web/inventory.json` y `xtask/web/interop.json` para confirmar origen, exportaciones, dependencias y riesgos.
3. Portar plantillas a `crates/comandos-web-view` y validar DOM normalizado con fixtures capturadas mediante `shots dom-dump` cuando el arnes lo exponga.
4. Portar logica a `crates/comandos-web/src/components/<id>.rs`. Los globales de `interop.json` se exportan por `comandos_web_dom::bridge`; estado mutable compartido con JS vivo se mantiene temporalmente en propiedades de `window`.
5. Portar `tests/<id>_checks.cjs` a pruebas Rust host o `wasm-bindgen-test` con nombres equivalentes.
6. Crear `crates/comandos-web/components/<id>.json` con `kind`, `source`, `sha256`, `exports` y `deps`.
7. Ejecutar `shots pair --suite <id>` y `shots remote-vs-desktop --suite <id>` en `chrome-bg`/macmini contra fixtures privadas.
8. Sombra 24 h en telefono y navegador de escritorio contra 4782 con `--shadow-readonly`.
9. Activar con `comandos web set <id> on`.
10. Revertir con `comandos web set <id> off`.

## Estado

| Componente | Pin | Pruebas host | Sombra | Activacion | Release | Notas |
| --- | --- | --- | --- | --- | --- | --- |
| ui-sounds | 2026-10-05 | En este lote | Pendiente controlador | Pendiente | Pendiente | Audio visual/listening requiere chrome-bg en macmini. |
| quick-terminal | 2026-10-05 | En este lote | Pendiente controlador | Pendiente | Pendiente | No activado en HOME vivo. |
| device-drafts | 2026-10-05 | En este lote | Pendiente controlador | Pendiente | Pendiente | Export compartido para term.html. |
| session-config | 2026-10-05 | En este lote | Pendiente controlador | Pendiente | Pendiente | Plantilla fixture privada. |
| workspace-layout | 2026-10-05 | En este lote | Pendiente controlador | Pendiente | Pendiente | Fixtures JSON compartidas. |
| push-settings | 2026-10-05 | En este lote | Pendiente controlador | Pendiente | Pendiente | No pide permisos en pruebas host. |
