# Fase 2f-1 — Tarea 3: informe

## Crear y revivir sesiones (`sessions.rs`)

Commit `8745401` (rama `migration/rust-fase2f`, sobre `b11bca7`).

Archivos:
- `crates/comandos-server/src/dash/native/sessions.rs`: `SessionsRoute::{RecoverTab, Ensure,
  New, Shell, Up}` (POST, `Key::Raw`) y las piezas que consumen T4–T6, todas `pub` (con
  `pub(crate)` el compilador las da por muertas hasta T4–T6 y `clippy -D warnings` falla):
  `agent_launch`, `agent_set`, `scope_cmd`, `tmux_new_session`, `ensure_shell_window`,
  `select_claude_window`, `focus_session`; `AGENT_LAUNCH` literal. Privadas: `spawn_terminal`,
  `pick_terminal`, `raise_win`/`wm` (pulso keep-above en `native.tasks()`), `write_focus`
  (`app-focus.json` con `std::fs::write`, sin escritura atómica, como el `open(…, "w")`),
  `order_clients` (`split(None, 1)` + `sorted(int, reverse=True)` estable), `shlex_quote`.
- `crates/comandos-server/tests/dash_native_sessions.rs` (5 pruebas, nuevo).
- `xtask/parity/2f/tabs.jsonl`: `t-ensure-shell`, `t-new-shell`, `t-new-viva`, `t-up-sin-dir`,
  `t-recover-tab-sin-cwd`.
- `xtask/parity/README.md`: nota P20 (el `systemd-run` del arnés no crea nada; la creación real
  la verifica el gemelo).

Sin cambios en `mod.rs` (las rutas ya estaban cableadas desde el andamio: `every_route_has_a_cut`
no cambia), `tabs.rs`, `tab_registry.rs`, `tmux.rs` ni `support/`.

### Comportamiento

- `/recover-tab` va tras el preámbulo sin `resolve_project_session` (como en el Python);
  `/ensure`, `/new`, `/shell`, `/up` tras `target::post_target`. Literales y orden del Python
  (`bin/cc-dash` 9540, 9713, 9737, 9758, 9774).
- `scope_cmd` usa el `Program` de `opts.scope` tal cual (las banderas `--user --scope --collect
  --quiet` ya van en su prefijo, P18) + `opts.tmux.program` (ruta y prefijo) + su entorno. Plazo
  15 s: `TimeoutExpired` → 504; fallo al lanzar o decodificar → 500.
- `opts.scope == None` → las cinco rutas declinan antes de nada (R2 de la 2d).
- **Decline solo antes del primer efecto, medido.** Un `task_local` (`EFFECTS`) se marca justo
  antes de cada efecto (`new-session`, `new-window`, `select-window`, `switch-client`, escrituras
  del registro y de `app-focus.json`, `wmctrl`, la terminal); `answer` convierte un `Decline`
  posterior en 500 con su línea en stderr (ruling 2). Antes de crear la sesión se calculan el
  agente, su lanzador (`agent_launch` declina con un agente no ASCII: el `.upper()` del Python es
  Unicode) y se leen los archivos del registro (`app-tabs.json`, `app-tabs-meta.json` y, en
  `/recover-tab`, el historial): un registro incierto es 500 sin sesión nueva.
- `agent_set`: `AGENTS` de la conf (o `DEFAULT_AGENTS`) ∪ harnesses del registro; sin
  `config/providers.json` solo la conf (el `except: pass`); un registro que el frente no
  reproduce con certeza declina (solo se usa antes de efectos).
- `cwd` de `/ensure`/`/shell`/`/up` sin `str()`: un valor verdadero que no es texto declina antes
  de nada (el `os.path.isdir(int)` del Python es un `stat` de descriptor). Un agente verdadero que
  no es texto da el 500 del `agent.replace` del Python (sin efectos). `os.path.isdir` contra
  `opts.cwd` y en `spawn_blocking`.
- `focus_session`: `has-session`, `list-clients -t =local`; con app abierta `app-focus.json`
  (`session_labels`; lo que el Python captura —tmux, E/S— no escribe; un archivo del registro
  incierto es 500 por el ruling 2); si no, `switch-client` del cliente más activo o
  `spawn_terminal` (retorno temprano, sin `wmctrl`, como el Python); después `raise_win`.
- Todo programa fuera de tmux por ruta absoluta de `procs::which_in(opts.search_path, …)` con
  `opts.program` y `gui_env_for` (wmctrl, terminales, `systemd-run` de la terminal).

## Pruebas

TDD contra el oráculo en el gemelo confinado (TMPDIR=/tmp/c18, sin `TMUX`/`DISPLAY`/
`WAYLAND_DISPLAY`/`DBUS_SESSION_BUS_ADDRESS`):

- `create_and_revive_match_python`: `/new` proyecto (argv exacto de `new-session` y `new-window`
  y la cola de `systemd-run --user --scope --collect --quiet tmux new-session` afirmados), `/new`
  viva, `/new` sin proyecto, `/ensure win=shell`, `/ensure` de `ssh-x` (identidad derivada y
  ventana `ssh x; exec $SHELL`), `/shell` y `/up` sin clientes (terminal por el `systemd-run`
  falso), `/recover-tab` con `codex`, sin carpeta (400) y con agente fuera de `agent_set`, y
  `/ensure` que revive con `cwd`. En cada paso: estado y cuerpo, los cinco archivos del registro
  y las órdenes de tmux que mutan iguales a los del Python (raíz del HOME como `~`); al final
  ventanas, llamadas al `systemd-run` falso y falsos lanzados (`claude`, `codex`, `ssh`, `kitty`).
- `up_without_dir_writes_nothing` (Review Focus 3): `/up`, `/shell`, `/ensure` de una sesión sin
  carpeta → 400 igual, sin servidor tmux, sin `systemd-run`, registro intacto.
- `no_scope_in_production_declines_before_creating`: sin scope las cinco rutas llegan al
  heredado; ni una orden de tmux (`tmux.log` no existe), ni servidor, ni registro.
- `focus_with_clients_matches_python`: clientes de control (`tmux -C attach`, con `-S`) en
  `local` (app abierta → `app-focus.json` igual, `wmctrl -x comandos` con pulso) y en `otra`
  (`switch-client -c <cliente> -t =p2f`, `wmctrl -a p2f`, `add,above`/`remove,above` a los 0,6 s).
- `bad_inputs_match_python`: sesión inválida, no texto, agente no texto (500 sin sesión).
- Unitarias en `sessions.rs`: `shlex_quote`, orden de clientes y errores, tabla de terminales,
  `scope_cmd` sin banderas dobles.

Resultados: `dash_native_sessions` 5/5 (tres corridas seguidas), `comandos-server` completo en
verde; `cargo fmt --check` y `cargo clippy --workspace --all-targets -j 4 -- -D warnings`
limpios. Suite completa del workspace (`cargo test --workspace --no-fail-fast`, una vez al
final, mismo entorno): **1044 pasan, 0 fallan, 1 ignorada** en 125 binarios. Nota: el
`scratchpad` es compartido con el carril B; mi primera corrida escribía en el mismo archivo que la
suya y se repitió con un nombre propio.

Paridad aislada (`xtask parity --fixture xtask/parity/frente.jsonl --fixture
xtask/parity/2f/tabs.jsonl --hooks ~/.claude/hooks`, binario `comandos-cli` recién compilado):
**163 OK, 5 DIFF, 0 SKIP de 168**; las 14 líneas `t-*` (las 5 nuevas incluidas) OK. Los 5 DIFF
son de `frente.jsonl` (`a-notices`, `a-notices-device`, `a-watch-ya`, `a-watch-rev-distinta`,
`f-pomodoro`): ids de eventos aleatorios y `serverNowMs` en bases de estado vacías distintas, el
mismo artefacto que la T2 documentó al correr sin `--state-db`. No pasé `--state-db`
(`~/.local/state` queda fuera por las reglas de esta tarea).

## Desviaciones

- **Funciones `pub` en vez de `pub(crate)`** (motivo arriba).
- **La terminal de `spawn_terminal` va por ruta absoluta** dentro de `systemd-run --user
  --collect --quiet <ruta> …` (el Python pasa el nombre `kitty`; regla P9). En producción el
  `PATH` es el mismo, así que resuelve al mismo ejecutable.
- **Fixture**: las líneas `/new` no comparan `files` (el registro guarda la ruta del HOME de cada
  copia y el arnés no la normaliza); `t-ensure-shell` va antes que ellas y sí compara
  `app-tabs-meta.json`. En el arnés no hay `~/codebase`, así que `t-new-proyecto` del brief es
  `t-new-shell`/`t-new-viva`.
- **`app-focus.json` con un archivo del registro incierto**: 500 (ruling 2) donde el Python, con
  su `except`, seguiría sin escribir o escribiendo; solo con anidamiento ≥ 1000 o sustitutos
  sueltos.
- `os.path.isdir` de un `cwd` relativo se resuelve contra `opts.cwd` (directorio del frente al
  arrancar), no contra el del proceso Python.

## Seguridad (tmux, agentes, servicios vivos)

- No ejecuté tmux, `systemd-run`, agentes, lanzadores `cc-*`, terminales ni `wmctrl` a mano.
  Todo tmux de las pruebas va por `-S` al socket privado de un HOME temporal bajo `/tmp/c18`
  (guardián del `fakebin` con `env -i` y entorno confinado, o `TestHome::tmux_command`).
- Canario antes de crear sesiones: las cinco pruebas que crean o podrían crear llaman a
  `assert_confined` (cada agente/terminal/`ssh`/`wmctrl` del `fakebin` es el falso que anota, el
  guardián lleva `-S '<socket privado>'` y `env -i`, `systemd-run` es el falso que solo ejecuta
  tmux, `PATH` confinado = solo el `fakebin`, scope del frente con `env_clear`, prólogo del
  oráculo presente). Además corrí antes los canarios existentes (`dash_twin`, 11/11). Las
  sesiones creadas corrieron el `claude`/`codex`/`ssh` FALSOS (comprobado por su registro).
- Los clientes de control los lanza la prueba con `-S` y los mata ella (su propio proceso
  cliente). Ninguna orden `kill-*` de tmux en código ni pruebas; limpieza por el `Drop` de
  `TestHome` (`kill-server -S` y después borrar).
- Producción: `scope_cmd` usa exactamente el `systemd-run` y el `tmux` que usaría el Python
  (`Tmux::system()` = `tmux` del entorno del proceso; ningún `-S` ni `TMUX_TMPDIR` nuevo). Las
  rutas solo tocan `=<sesión pedida>` (y `switch-client` del cliente más activo, como el Python);
  nunca matan ni renombran sesiones. Las pruebas comparan cada argv de tmux con el del Python.
- `~/.claude` solo como copia de `xtask parity` (`cp -a` dentro de su netns, `systemd-run` del
  arnés = `exit 0`). Sin tocar `~/.local/state`, `~/.local/share/comandos`, systemd, servicios ni
  puertos 4777–4782; sin red.
- `pgrep -a tmux` (solo lectura) antes y después: ningún servidor nuevo mío (ninguno bajo
  `/tmp/c18` ni `cmd-native-*`); los `comandos-parity-915731-*` de `/tmp/c10` eran de otro agente.
