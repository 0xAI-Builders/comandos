# Reglas para agentes en este repo

## tmux: nunca el servidor del usuario

Las sesiones de trabajo de Jesús (20+ agentes) viven en el tmux `default` de
`/tmp/tmux-1000`. Un `kill-server` ahí las mata todas (pasó el 2026-10-04).

- Todo tmux de prueba va con socket explícito: `tmux -S <dir>/tmux-1000/default`
  o `-L <etiqueta-propia>`. **`TMUX_TMPDIR` solo no sirve**: tmux 3.2a lo ignora
  en silencio si el directorio no existe y cae en `/tmp/tmux-1000/default`.
- Nunca `rm -rf` del directorio del socket antes del `kill-server`; primero
  `kill-server`, después borrar.
- En Rust usa `Tmux::private(dir)` (`crates/comandos-server/src/dash/native/tmux.rs`)
  y en `xtask` `private_tmux(dir)`: ya pasan `-S`.
- Prohibido `tmux kill-server`, `kill-session`, `pkill tmux` o `kill -9 -1` sin
  `-S`/`-L` propio, aunque sea «para probar».

## Compilaciones: caché compartida con presupuesto

- Todas las compilaciones de migración en Linux pasan por
  /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-integration-acp/scripts/rust-build-cache.
  El runner aislado ya incorpora esa comprobación.
- Los worktrees comparten /home/someguy/codebase/0xJesus/ComandOS/.build/target-integration-acp.
  No crear un target por agente, revisión o intento ni cambiar de target para
  saltarse un rechazo del presupuesto. Ejecutar un solo Cargo a la vez.
- El runner rechaza opciones de salida o configuración que puedan cambiar ese
  target; no pasar `--target-dir` ni `--config` a las comprobaciones.
- Se comprueba la caché antes y después de cada comando. Con más de 32 GiB
  asignados o menos de 16 GiB libres se rechazan nuevas compilaciones. Es un
  control de admisión; una compilación puede exceder el límite mientras corre.
- Este control no borra artefactos ni detiene procesos. La limpieza requiere
  comprobar referencias y montajes de procesos activos y conservar binarios finales fuera
  de los targets. Preservar siempre sesiones, datos e historiales del usuario.
