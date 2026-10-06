# M1: CLI y procesos en macOS

El CLI comparte la lógica de Linux y Darwin. Darwin obtiene procesos mediante
ps y lsof, lanza tmux directamente y prepara un directorio de ejecución privado.
M1 no incluye AppKit, extracción de GTK, instaladores Mac ni validación de UI.

Las rutas de este documento pertenecen al checkout de M1 en Linux:
/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-mac-m1-repair.
Los originales que se comparan pertenecen al checkout principal en Linux:
/home/someguy/codebase/0xJesus/ComandOS.

## Datos y consumidores

| Función | Linux | Darwin | Python original en Darwin |
|---|---|---|---|
| ProcSource::snapshot, ProcFs, PsSnapshot | ProcFs lee una raíz inyectada; argv conserva límites NUL, inicio en ticks, cwd mediante enlace. Los consumidores previos conservan sus lectores y su raíz falsa. | ps -axww -o pid=,ppid=,lstart=,command=; lstart se convierte en segundos UTC. LC_ALL=C y TZ=UTC estabilizan el formato. El inventario base deja cwd vacío; snapshot_with_cwd permite completar un inventario genérico; la atribución por proyecto usa agents_from_source y with_cwds después de filtrar únicamente candidatos canónicos. Un ps y, solo si hay candidatos, un lsof colectivo de sus PID. Se omiten filas incompletas. | agent_procs utiliza ps -axo pid=,command= y lsof; los otros lectores de procfs carecen de esos datos. |
| agent_procs::agent_procs_for_agents, agents_from_snapshot | Primer alias de los tres primeros argumentos, orden del directorio. | Dos primeros nombres canónicos; gana el menor nombre en el empate, como la rama Mac original. Sin cwd no se atribuye el proceso a un proyecto. | Misma selección canónica de dos nombres (el conjunto canónico se conserva separado del mapa de aliases), con lsof de los candidatos y plazo de ocho segundos para cwd. M1 limita lsof a dos segundos. |
| agent_procs::parent_pid, proc_cmdline, process_start; process_start_time | Lectores originales de la raíz procfs; errores conservan sus resultados previos. | Consulta dirigida a un solo PID mediante ps. No ejecuta lsof por cada consulta de padre o inicio. Fallo: 0, vector vacío o inicio ausente. | parent_pid usa ps como fallback (tres segundos); _proc_cmdline y _process_start solo leen procfs y devuelven vacío. |
| PaneInspector::new, inspect, process_argv, native_metadata | Misma raíz falsa/real, reglas de shell, orden de hijos, JSON y comprobación de inicio. | Un inventario ps por inspector, sin lsof de cwd innecesario; hijos por ppid e inicio para validar los registros de hooks. with_source permite probar estos consumidores con inventario propio. No observa de nuevo el inicio al leer cada registro. | El constructor itera /proc sin capturar la ausencia del directorio; los lectores individuales de cmdline/stat devuelven vacío ante OSError. No ofrece inventario Mac. |
| PaneInspector::flags, flags_for | Conserva el parser y los límites NUL de los argumentos que pueden reanudarse. | Devuelve vacío: el texto de ps pierde los límites originales y no se convierte en banderas de reanudación. | _flags captura el fallo de cmdline y devuelve vacío. |
| PaneInspector::inspect_codex, inspect_native, open_targets | Lee los descriptores procfs; puede inspeccionar un archivo abierto aunque su nombre original haya desaparecido. | lsof proporciona nombres absolutos; se abren por nombre. Un archivo borrado, renombrado, inaccesible o sustituido después de lsof puede faltar o diferir. No es una referencia fijada al descriptor del agente. Identificadores explícitos en argv siguen siendo observaciones de texto con límites perdidos. | Sin los fd de procfs, la búsqueda de rollouts/conversaciones abiertos aporta menos datos o ninguno. |
| agent_procs::read_environ, AccountCache::account_for_pid; PaneInspector::inspect_codex; estados::grok_home_for y codex_conversation | Lee environ, CODEX_HOME, directorios de cuentas y los fd originales para completar cuentas y conversación. | No se inventa el entorno de otro proceso. read_environ devuelve vacío; el contexto por cuenta/HOME personalizado puede faltar. codex_conversation conserva su lector procfs y devuelve vacío; el inspector puede aportar el ID por lsof. | _read_environ devuelve vacío; los consumidores de fd pierden la conversación. Los valores por defecto y archivos de cuentas siguen disponibles. |
| session_configuration::server_start; target::pane_identity; terminal::pane_identity; estados::gather_evidence; pane_exit::start | Campo de inicio de la raíz procfs; conserva también los errores de decodificación/campos que cada consumidor trataba antes. | Inicio dirigido por ps en segundos, usado solo dentro del mismo sistema. La raíz falsa sigue usando los lectores anteriores también en Darwin. Fallo de ps deja inicio vacío y los consumidores mantienen sus guardias existentes. | _pane_identity lee /proc y deja server_start vacío ante OSError; _process_start también devuelve vacío. |
| hooks::adapter::proc_stat, opencode::start_tick, agy::record_process | Lectores originales; OpenCode conserva incluso su regla del primer paréntesis. | Padre/inicio por ps; AGY usa argv observado para encontrar su antecesor. Sin una observación válida no escribe el registro. | Los adapters Bash/Python y plugin OpenCode leen procfs; no obtienen ese inicio en Darwin. |
| extension_launch::process_env, verify_launch, verify_mounts, launch_from_pid, namespace_preflight | Verifica entorno exacto, argv NUL, recibos, ns/mnt, mountinfo y raíz montada. | Mantiene el rechazo de verificación cuando falta procfs. namespace_preflight rechaza Darwin antes de crear su fixture o buscar unshare. No sustituye límites exactos o namespaces por ps; no certifica un lanzamiento gestionado ni una vista montada. | Las mismas lecturas de procfs fallan y verify_launch devuelve False; launch_from_pid no descubre el manifiesto. |
| browser::migrate::discover_configs (entorno de procesos) | Examina la raíz procfs cuando está autorizado ese descubrimiento. | Sin procfs no descubre directorios adicionales del entorno de procesos. Conserva descubrimiento por archivos. | El descubrimiento por /proc tampoco está disponible. |
| hooks::notify_http::worker_exe | Usa /proc/self/exe para retener la referencia al ejecutable incluso tras reemplazo de instalación. | La rama existente usa current_exe; no proporciona un enlace procfs al inodo borrado. No se añade otro mecanismo de publicación Mac. | Los helpers por nombre/ruta tampoco ofrecen ese enlace al inodo. |
| doctor::core (diagnósticos de servicios) | Consultas y arreglos de systemctl/ss originales. | Conserva el diagnóstico previo, que puede informar fallos de servicios Linux ausentes. Solo la sección desktop ya omite GTK en Darwin. La comprobación/instalación de launchd queda fuera de M1. | cc-doctor también utiliza estos diagnósticos Linux y omite GTK en Darwin. |
| winstart::kernel_release y doctor::kernel_release | Lee kernel/osrelease para clasificar Linux/WSL, con sus seams falsos existentes. | El valor de procfs queda vacío. Doctor reconoce Darwin mediante uname; winstart conserva su condición ajena a WSL. | Los scripts originales también condicionan estas rutas a Linux/WSL y usan uname para Darwin. |

El inicio en Darwin tiene resolución de un segundo y no es intercambiable con
los ticks Linux. Puede no distinguir dos encarnaciones del mismo PID dentro de
ese segundo. ProcSource aporta observaciones; no equivale a un pidfd ni concede
por sí mismo autoridad de señalización. No se añaden señales a procesos ajenos.

ps tiene plazo de cinco segundos; lsof, dos. Cada helper tiene como máximo
4 MiB de stdout y stderr descartado. Se conserva stdout parcial con salida no
cero, como subprocess.run en el original. Los comandos reciben argv directamente,
sin shell. Su grupo propio se cierra antes de recoger el líder, incluso cuando
este terminó normalmente. No se prometen datos completos de procesos cuyo usuario,
permisos, codificación o nombres impidan la observación.

## Lanzamientos y rutas

| Consumidor | Linux | Darwin | Original |
|---|---|---|---|
| quick::find_scope, platform_tmux; sessions::scope_cmd y spawn_terminal; ssh::setup; webterm::run_with y stop | Sigue usando systemd-run cuando corresponde, con flags, prefijos, entorno y guardias originales. Sin scope, las rutas que antes declinaban siguen declinando. | No busca ni ejecuta systemd-run, aunque exista un binario con ese nombre en PATH. La cola tmux/terminal se ejecuta directamente; webterm omite también systemctl al detener su fallback propio. | cc-app-mac lanza tmux sin systemd-run; scope_cmd de cc-dash ejecuta la cola directa cuando systemd-run no existe. |
| platform::runtime_directory_from_env; dash::from_env; browser::expose::directory; hooks::claude_status::run | Mantiene las rutas y efectos previos, incluido XDG ausente/vacío según el consumidor. | XDG_RUNTIME_DIR no vacío; si falta, TMPDIR/comandos-UID (TMPDIR ausente: /tmp). Requiere ruta absoluta sin .. y directorio propio, sin enlace, 0700. Crea únicamente en operaciones con creación autorizada. Las lecturas no crean el directorio. | cc-app-mac utiliza rutas temporales para su socket; el hook usa XDG_RUNTIME_DIR o /tmp y expose utiliza XDG o estado HOME. M1 añade la política compartida privada. |
| platform::hostname; native::desktop_device y term::bridge | Conserva proc/sys/kernel/hostname y los defaults previos del consumidor. | uname nodename, sin procfs. | desktop_device usa os.uname().nodename. |
| procs::child_exited_unreaped; runtime::acp_client; acp::protocol; mobile::process | waitid de nix con WNOWAIT; el orden de cierre/recogida permanece. | waitid seguro de rustix con NOWAIT. Requiere un Child poseído por el llamador; no recibe un PID arbitrario. | Los contratos ACP/mobile originales y reparaciones Rust fijan la limpieza de sus propios procesos; no se convierte una consulta ps en ownership. |
| extensions::skills::rename_new | renameat2 NOREPLACE original. | renameat_with NOREPLACE (RENAME_EXCL Apple), con los mismos fd de directorio y error cuando ya existe destino. | La publicación de skills no debe reemplazar un destino existente. |

## Fuentes y pruebas

Oráculos leídos o extraídos sin importar ni arrancar la app completa:

- /home/someguy/codebase/0xJesus/ComandOS/bin/cc-app-mac:132–164.
- /home/someguy/codebase/0xJesus/ComandOS/bin/cc-dash:3916, 5505–5545, 6110–6177.
- /home/someguy/codebase/0xJesus/ComandOS/lib/pane_snapshot.py:20–200.
- /home/someguy/codebase/0xJesus/ComandOS/lib/extension_launch.py:431–482.
- /home/someguy/codebase/0xJesus/ComandOS/bin/cc-webterm:152–185.
- /home/someguy/codebase/0xJesus/ComandOS/bin/cc-doctor:218 y diagnósticos core.
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-mac-m1-repair/crates/comandos-runtime/tests/fixtures/hooks/oracle/adapters/agy-hooks.sh.
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-mac-m1-repair/crates/comandos-runtime/tests/fixtures/hooks/oracle/opencode-comandos.js.

Pruebas del checkpoint:

- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-mac-m1-repair/crates/comandos-runtime/tests/procs.rs.
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-mac-m1-repair/crates/comandos-runtime/tests/platform.rs.
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-mac-m1-repair/crates/comandos-server/src/dash/native/quick.rs (módulo tests).

El informe y provenance del checkpoint registran comandos, pins, resultados y
SDK privados. Los checks Apple no enlazan ni ejecutan un binario Darwin. Los
fixtures Linux no equivalen a aceptación del producto Mac, UI, proveedores reales,
TTY, rendimiento ni servicios instalados. No se ejecutan pruebas que enumeren
procesos personales; ese límite se registra al sustituir el gate global Linux.

## C5: mantenimiento Codex en Darwin

El checkpoint C5 conserva la implementación Linux. En Darwin, el CLI permite
`comandos --version`, las cuatro ayudas Codex, `comandos codex yolo-policy -- …`,
`comandos codex yolo-install` y el lanzador autónomo instalado. La instalación
sigue validando la ayuda del ejecutable original, hash/manifiesto del artefacto,
el primer backup y las rutas de shell. `--dry-run` no publica el lanzador ni
modifica shell o backups; consultar la ayuda del vendor propio sigue siendo una
operación explícita del preflight. No utiliza Python como implementación del
lanzador.

`comandos codex full-access` (también el alias `cc-codex-full-access`) y
`comandos codex thread-release` requieren Linux. En Darwin terminan con código 1
y un diagnóstico en stderr antes de validar HOME/planes, leer autoridad de
Store, inventario, cuentas o transcripts, crear directorios o ejecutar helpers.
La ayuda sigue disponible con código 0. El rechazo incluye dry-run, retries y
`full-access --install-only`; para instalar en Darwin se usa `yolo-install`.
No se sustituye la autoridad pidfd por observaciones ps/lsof ni se reconstruyen
argumentos o entornos de otras cuentas a partir de texto ambiguo. El adaptador
Local rechaza también su construcción directa en plataformas sin Linux.

El transporte de los hijos poseídos de Codex usa
`comandos_runtime::procs::child_exited_unreaped`, ya compartido por M1. En Linux
conserva waitid/WNOWAIT; en Darwin usa rustix waitid/NOWAIT. El líder permanece
sin recoger hasta cerrar su grupo con SIGKILL, tanto tras salida normal como
por error/cancelación. Los pidfd de señales a un agente exacto se compilan sólo
en Linux. No se concede autoridad para señalar procesos ajenos en Darwin.

Fuente y pruebas de este checkpoint, en el checkout autor de Linux:

- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-cli-c5-darwin/crates/comandos-cli/src/codex/mod.rs.
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-cli-c5-darwin/crates/comandos-cli/src/codex/process.rs.
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-cli-c5-darwin/crates/comandos-cli/src/codex/protocol.rs.
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-cli-c5-darwin/crates/comandos-cli/src/codex/runtime.rs.
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-cli-c5-darwin/crates/comandos-cli/src/codex/portability_tests.rs.
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-cli-c5-darwin/crates/comandos-cli/tests/codex.rs.
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-cli-c5-darwin/crates/comandos-cli/tests/codex_full_access.rs.

Los checks Apple de aarch64/x86_64 con warnings rechazados son comprobaciones
de compilador, incluidos los targets de tests; no enlazan ni ejecutan un CLI
Darwin. Los tests del gate de plataforma se ejecutan en hijos Linux privados
con selección de plataforma interna a los tests, sin override de producto.
La integración full-access con proc/pidfd y los tests dependientes de ese
backend están marcados Linux-only. Los fixtures Python son oráculos o vendors
falsos propios; no se operan cuentas, inventarios ni proveedores reales.
Quedan pendientes el enlace/ejecución nativa en Mac, TTY y los contratos del
vendor real. Este checkpoint no valida AppKit, GTK, servicios, instalación
personal ni cutover.

### Lecturas cortas de NativeTools

Una reparación independiente del port C5 drena lecturas no bloqueantes
positivas sin pausa por fragmento. Comprueba el plazo antes de cada lectura,
incluidas las repeticiones por Interrupted. Sólo espera cuando el pipe está
bloqueado o ya llegó EOF pero el líder sigue pendiente. Conserva el techo de
4 MiB, la salida parcial con código distinto de cero y el cierre del grupo
poseído antes de recoger al líder. No modifica el umbral de tres segundos del
test original de exceso de salida. El repro extraído limita exclusivamente el
slice de lectura a 512 bytes para ejercitar lecturas cortas deterministas; no
cambia el lector productivo de 8192 bytes. La repetición nativa del test Mac
original queda a cargo de Root.

### C5: EPERM en un grupo propio que sólo contiene zombies

Darwin puede devolver EPERM al señalar un grupo que conserva el líder
terminado sin recoger. XNU omite los zombies y devuelve EPERM si no encontró
miembros elegibles; ese errno también puede indicar falta real de permisos
([Apple XNU](https://raw.githubusercontent.com/apple-oss-distributions/xnu/main/bsd/kern/kern_sig.c)).
C5 conserva el rechazo genérico: únicamente tras EOF y waitid/NOWAIT del hijo
propio, un EPERM de SIGKILL puede verificarse con una consulta dirigida.

La verificación toma prestado Child y comprueba que sigue sin recoger.
El llamador estableció el grupo propio con process_group(0) al crear el hijo;
la fila del líder en ps debe confirmar que PGID coincide con PID. No usa
getpgid sobre el zombie: Darwin puede devolver ESRCH aunque ps aún muestre
esa fila, por lo que ese syscall no prueba la ausencia del grupo
([Apple proc_find](https://raw.githubusercontent.com/apple-oss-distributions/xnu/main/bsd/kern/kern_proc.c)).
Ejecuta sólo /bin/ps con COMMAND_MODE=unix2003,
selector -g único y columnas pid=,pgid=,stat=. Ese selector se traduce a
KERN_PROC_PGRP; -G seleccionaría grupos de usuario y no sirve para esta prueba
([Apple ps](https://raw.githubusercontent.com/apple-oss-distributions/adv_cmds/main/ps/ps.c)).
Exige salida completa, UTF-8, exit 0, líder Z presente, PGID exacto, PID
únicos y todas las filas Z con flags válidos. Vacío, filas omitidas del líder,
malformación, miembro vivo, timeout, exceso de salida, cancelación o error
rechazan la excepción. No lee un inventario global ni da autoridad para
señalar PID observados. El grupo se cierra antes de recoger al líder;
la consulta leaf no usa el transporte C5 y no tiene recursión de limpieza.

NativeTools conserva internamente el exit status para esta autoridad, pero
Tools::run sigue devolviendo stdout parcial con exit distinto de cero, como
requiere el contrato previo. Se mantienen 4 MiB, lectura sin pausa por
fragmento, comprobaciones de plazo/cancelación y RAII del grupo propio.

La prueba de cancelación del instalador usa un canal Unix privado R/EOF del
mismo vendor en Linux y Darwin. La espera de readiness cubre el deadline de
help productivo de 15 segundos; el plazo desde TERM hasta salir sigue siendo
dos segundos. No interpreta la ausencia de /proc en Mac como prueba de cierre.
Una prueba nativa adicional consulta sólo grupos creados por el test: líder Z
y líder Z con descendiente propio vivo. Las comprobaciones cruzadas validan
compilación; Root debe ejecutar ese caso y el instalador real sobre fixtures
privadas en el Mac antes de aceptar el gate nativo.

### C5: aliases del launcher en Darwin

El launcher resuelve el destino real de current_exe antes de buscar su
launcher.json. Esa API puede devolver la ruta del symlink que lo invocó
([contrato Rust](https://doc.rust-lang.org/std/env/fn.current_exe.html));
buscar al lado de bin/codex o del wrapper producía missing en el caso nativo.
La autoridad sigue viniendo de current_exe, nunca de argv[0]. Después se
conservan las lecturas regulares acotadas, manifiesto v2/tipo/ruta/hash,
original y rechazo de recursión. El fixture ejecuta entry, wrapper y artefacto
directo, y comprueba argv literal, cuenta y primer backup. La probe Linux
extrae la función real y sólo sustituye current_exe por el alias observado:
RED missing pasa a GREEN sin cambiar lectores ni política. La repetición
nativa sobre este pin reparado sigue a cargo de Root.
