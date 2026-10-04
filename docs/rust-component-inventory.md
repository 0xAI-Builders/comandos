# Inventario de la migración

Base 214e55a. 170 archivos de producto y 205 de verificación con código o estilo. Inventario exhaustivo de esas extensiones y scripts con shebang; los terceros de vendor se conservan aparte. La presencia de una implementación Rust no autoriza su despliegue ni demuestra paridad de toda la aplicación.

Referencia adicional capturada el 2026-10-03T05:39:58.314312+00:00 desde /home/someguy/codebase/0xJesus/ComandOS, sobre la base de trabajo 263492fb996b5e782b2dc5ccd8c2dc36901c100f. Manifiesto /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/.superpowers/sdd/incoming-source-20261003T0540/manifest.json, SHA-256 faa170bb61ac883583058e83d6f1f4f850a3a6935fad5fe033a40e8259172801. Conserva 7 referencias actualizadas y añade 5 componentes de producto pendientes de migrar a Rust y 3 oráculos de transición. La base inicial 214e55a y los estados anteriores se conservan. Las pruebas sintéticas no demuestran detección del hilo seleccionado ni reanudación completa.

Once entradas son copias sin modificaciones de paquetes externos, con sus bytes contrastados de nuevo contra la evidencia registrada. Quedan identificadas como terceros; conservarlas y verificar su integración sigue siendo obligatorio. Las 159 entradas propias incluyen cuatro estilos a conservar. El addon propio de ligaduras permanece pendiente. Esta clasificación no aumenta el código propio migrado. Evidencia: /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/.superpowers/sdd/static-vendor-comparison-evidence.json.

Los assets, servicios e instaladores declarativos se deben contrastar en el empaquetado final. La lógica propia embebida en HTML/JS también se migra; CSS, fuentes e imágenes conservan su diseño.

| Fuente absoluta | Estado | Implementación Rust |
|---|---|---|
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/adapters/agy-hooks.sh | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/adapters/agy-statusline.py | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/adapters/codex-hooks.sh | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/adapters/codex-notify.sh | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/adapters/gemini-hooks.sh | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/adapters/grok-hooks.py | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/adapters/opencode-comandos.js | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/assets/opentype/opentype.min.js | Tercero sin modificaciones; conservar y verificar integración | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/assets/uisfx/uisfx-0.4.0.js | Tercero sin modificaciones; conservar y verificar integración | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/assets/xterm/addon-attach.js | Tercero sin modificaciones; conservar y verificar integración | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/assets/xterm/addon-canvas.js | Tercero sin modificaciones; conservar y verificar integración | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/assets/xterm/addon-fit.js | Tercero sin modificaciones; conservar y verificar integración | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/assets/xterm/addon-ligatures-web.js | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/assets/xterm/addon-web-links.js | Tercero sin modificaciones; conservar y verificar integración | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/assets/xterm/xterm.css | Tercero sin modificaciones; conservar y verificar integración | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/assets/xterm/xterm.js | Tercero sin modificaciones; conservar y verificar integración | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/bin/cc-acp | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/bin/cc-agents | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/bin/cc-app | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/bin/cc-app-mac | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/bin/cc-browser-expose | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/bin/cc-browser-npx-guard | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/bin/cc-browser-remote | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/bin/cc-centro | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/bin/cc-codex-full-access | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/bin/cc-dash | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/bin/cc-doctor | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/bin/cc-extension-session | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/bin/cc-extensions | Parcial: serve/count/import/sync/status revisados; check e instalación pendientes | /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/crates/comandos-extensions/src/main.rs |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/bin/cc-keys | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/bin/cc-mobile | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/bin/cc-next | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/bin/cc-notifyd | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/bin/cc-pane-model | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/bin/cc-session-snapshot | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/bin/cc-term | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/bin/cc-webterm | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/bin/cc-webterm-attach | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/bin/cc-winstart | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/bin/cc_usage.py | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/bin/ccx | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/dash/analytics-render.js | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/dash/analytics.css | Estilo a conservar; verificar renderizado pixel perfect | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/dash/analytics.js | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/dash/buttons.css | Estilo a conservar; verificar renderizado pixel perfect | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/dash/chain-builder.js | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/dash/command-sidebar.js | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/dash/device-drafts.js | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/dash/extensions.css | Estilo a conservar; verificar renderizado pixel perfect | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/dash/extensions.html | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/dash/extensions.js | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/dash/index.html | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/dash/news-reader.js | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/dash/notifications.js | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/dash/pomodoro.js | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/dash/prototype-grok-space.html | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/dash/prototypes/prototype-acomodar-pestanas.html | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/dash/prototypes/prototype-analytics.html | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/dash/prototypes/prototype-avisos-resumenes.html | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/dash/prototypes/prototype-botones-arcade.html | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/dash/prototypes/prototype-desktop-unificado.html | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/dash/prototypes/prototype-deslizar-pestanas.html | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/dash/prototypes/prototype-iguales.html | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/dash/prototypes/prototype-orden-pestanas.html | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/dash/prototypes/prototype-pestanas-juego.html | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/dash/prototypes/prototype-pomodoro-audio.js | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/dash/prototypes/prototype-pomodoro-sync-repro.cjs | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/dash/prototypes/prototype-reloj-arena.html | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/dash/prototypes/prototype-reloj-comportamiento.html | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/dash/prototypes/prototype-semaforos.html | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/dash/prototypes/prototype-ui-sounds.js | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/dash/prototypes/prototype-usage-cards.html | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/dash/prototypes/prototype-v1-grill.html | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/dash/prototypes/prototype-v1-server.cjs | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/dash/prototypes/prototype-v2-barra.html | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/dash/prototypes/prototype-v2-fase2.html | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/dash/prototypes/prototype-v2-smoke.cjs | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/dash/prototypes/vendor/uisfx-0.4.0.js | Tercero sin modificaciones; conservar y verificar integración | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/dash/push-settings.js | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/dash/quick-terminal.js | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/dash/session-config.js | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/dash/sw.js | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/dash/term.html | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/dash/ui-sounds.js | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/dash/vendor/markdown-it-15.0.2.umd.min.js | Tercero sin modificaciones; conservar y verificar integración | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/dash/vendor/purify-3.4.16.min.js | Tercero sin modificaciones; conservar y verificar integración | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/dash/work-marks.js | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/dash/workspace-dock.js | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/dash/workspace-layout.js | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/dash/workspace.css | Estilo a conservar; verificar renderizado pixel perfect | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/dash/workspace.js | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/design/proof.html | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/design/prototype-extensiones-pane.html | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/docs/research/extensiones-por-sesion/grok-ns.py | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/hooks/cc-notify.sh | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/hooks/cc-status.sh | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/hooks/cc-usage-tool.sh | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/install.sh | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/lib/accounts.py | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/lib/acp.py | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/lib/agent_stop.py | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/lib/allocation.py | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/lib/analytics_week.py | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/lib/app_state.py | Rust — revisado; sin despliegue | /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/crates/comandos-store/src/state_db/mod.rs |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/lib/browser_config_migration.py | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/lib/capabilities.py | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/lib/claude_trust.py | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/lib/cli_catalog.py | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/lib/cli_help.py | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/lib/codex_full_access.py | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/lib/codex_thread_release.py | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/lib/codex_yolo_install.py | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/lib/codex_yolo_policy.py | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/lib/command_chains.py | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/lib/event_intake.py | Rust — revisado; sin despliegue | /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/crates/comandos-runtime/src/bin/comandos-events.rs |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/lib/event_store.py | Rust | /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/crates/comandos-store/src/lib.rs |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/lib/extension_auth.py | Rust — revisado; sin despliegue | /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/crates/comandos-extensions/src/auth.rs |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/lib/extension_catalog.py | Rust — revisado; sin despliegue | /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/crates/comandos-extensions/src/catalog.rs |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/lib/extension_launch.py | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/lib/extension_metadata.py | Parcial: metadatos y conteo Rust revisados; instalación por migrar | /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/crates/comandos-extensions/src/metadata.rs |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/lib/extension_observations.py | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/lib/extension_proxy.py | Parcial: serve y metadatos nativos; check pendiente | /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/crates/comandos-extensions/src/serve.rs |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/lib/focus_progress.py | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/lib/grok_state.py | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/lib/gtk_tabstrip.py | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/lib/gtk_workspace.py | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/lib/mcp_descriptions.py | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/lib/model_catalog.py | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/lib/model_watch.py | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/lib/news_editions.py | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/lib/news_radar.py | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/lib/news_reading.py | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/lib/news_watch.py | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/lib/notification_delivery.py | Rust — revisado; sin despliegue | /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/crates/comandos-store/src/notifications.rs |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/lib/operator_catalog.py | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/lib/operator_dispatch.py | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/lib/operator_receipts.py | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/lib/pane_extensions.py | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/lib/pane_snapshot.py | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/lib/pane_typing.py | Rust — revisado; integración pendiente | /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/crates/comandos-runtime/src/pane_typing.rs |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/lib/platform.sh | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/lib/pomodoro.py | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/lib/providers.py | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/lib/quick_terminal.py | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/lib/retire-telegram.sh | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/lib/session_operations.py | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/lib/session_profiles.py | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/lib/session_tabs.py | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/lib/terminal_history.py | Rust — revisado; integración pendiente | /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/crates/comandos-runtime/src/terminal_history.rs |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/lib/terminal_panes.py | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/lib/tmux_clipboard.py | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/lib/tmux_snapshot.py | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/lib/tui_state.py | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/lib/turn_state.py | Rust | /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/crates/comandos-core/src/turn.rs |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/lib/web_push.py | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/lib/work_marks.py | Parcial: almacenamiento; dibujo pendiente | /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/crates/comandos-store/src/marks.rs |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/lib/workspace_layout.py | Rust — revisado; integración pendiente | /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/crates/comandos-core/src/workspace/layout.rs |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/lib/workspace_state.py | Rust — dominio y persistencia revisados; integración pendiente | /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/crates/comandos-core/src/workspace.rs ; /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/crates/comandos-store/src/workspace.rs |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/scripts/install-extensions.py | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/services/browser/apparmor/install-chrome-apparmor.sh | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/services/browser/broker.py | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/tools/analytics_extract.cjs | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/tools/analytics_fixture.cjs | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/tools/analytics_scope_css.py | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/tools/cli-commands/build.py | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/tools/cli-commands/builtin_filter.py | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/tools/cli-commands/scrape_prefix.py | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/tools/cli-commands/verify_names.py | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/tools/css_orphans.py | Pendiente | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/tools/png_diff.py | Pendiente | — |

Oráculos añadidos en esta captura, incluidos en el total de verificación.

| Fuente absoluta | Estado | Implementación Rust |
|---|---|---|
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/tests/test_codex_full_access.py | Oráculo de transición; conservar hasta reemplazo verificado | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/tests/test_codex_thread_release.py | Oráculo de transición; conservar hasta reemplazo verificado | — |
| /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/tests/test_codex_yolo_policy.py | Oráculo de transición; conservar hasta reemplazo verificado | — |
