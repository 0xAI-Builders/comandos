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
