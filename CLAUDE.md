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
  /home/someguy/codebase/0xJesus/ComandOS/scripts/rust-build-cache.
  El runner aislado ya incorpora esa comprobación.
- Los worktrees comparten /home/someguy/codebase/0xJesus/ComandOS/.build/target-integration-acp.
  No crear un target por agente, revisión o intento ni cambiar de target para
  saltarse un rechazo del presupuesto. Ejecutar un solo Cargo a la vez.
- El runner rechaza opciones de salida o configuración que puedan cambiar ese
  target; no pasar `--target-dir` ni `--config` a las comprobaciones.
- Se comprueba la caché antes y después de cada comando. Con más de 16 GiB
  asignados o menos de 16 GiB libres se rechazan nuevas compilaciones. Es un
  control de admisión; una compilación puede exceder el límite mientras corre.
- El runner serializa las compilaciones con un candado común. Antes de admitir
  una nueva, si falta espacio o se supera el presupuesto, recicla únicamente
  archivos no ejecutables `.rlib`, `.rmeta` y `.d` de variantes antiguas de
  dependencias: mínimo siete días sin cambios y dos variantes más recientes
  conservadas por crate y perfil. Conserva enlaces duros, archivos abiertos o
  mapeados, binarios finales y directorios de perfiles completos. Nunca envía
  señales ni limpia releases, fuentes, worktrees, sesiones o historiales.
- El GC falla de forma conservadora si encuentra un compilador activo, una
  referencia o un montaje no verificable, o un proceso desconocido que no pueda
  inspeccionarse. Zombies no tienen archivos abiertos; los helpers protegidos de
  PAM y ssh-agent sólo se reconocen por su ejecutable padre del sistema. Si no
  hay suficientes artifacts reciclables, el presupuesto sigue rechazando builds.
- Ejecutar Cargo directamente evita este contrato: Cargo no resuelve la raíz
  común de los worktrees en su configuración TOML. No añadir rutas absolutas
  locales al archivo versionado de configuración ni crear nuevos targets para
  esquivar la admisión; usar siempre el runner anterior. Los perfiles dev/test
  ya desactivan debug e incremental, y release elimina símbolos.

## Al entregar: retirar la caché de compilación

- Por instrucción de Jesús, la caché compartida es temporal. Al terminar una
  entrega, eliminar su contenido regenerable; el límite de 16 GiB es para el
  trabajo en curso, no una retención permanente entre entregas.
- Hacerlo después de completar las pruebas y verificar que los binarios y el
  bundle web instalados están íntegros fuera del target, en las releases de
  /home/someguy/.local/share/comandos/releases. Verificar también que la app y
  los servicios activos usan esas releases; conservar la posibilidad de rollback.
- Antes de limpiar /home/someguy/codebase/0xJesus/ComandOS/.build/target-integration-acp,
  terminar todas las compilaciones propias, coordinar con los demás agentes y
  tomar el candado /home/someguy/codebase/0xJesus/ComandOS/.build/rust-build-cache.lock.
  Revalidar la ruta real y las referencias de procesos: ejecutable, cwd, FDs,
  mappings y montajes. Un proceso no inspeccionable no demuestra ausencia de
  referencias; si no se puede descartar su uso del target, conservar la parte
  afectada y comunicar qué impide terminar la limpieza.
- Limpiar sólo ese target regenerable y registrar sus bytes antes y después.
  No borrar releases instaladas, fuentes, worktrees, pruebas o sus fixtures,
  evidencias de entrega, respaldos, conversaciones, sesiones ni historiales.
  No detener procesos de trabajo para vaciar la caché. Una nueva compilación
  volverá a generar los artifacts que necesite en el mismo target compartido.
