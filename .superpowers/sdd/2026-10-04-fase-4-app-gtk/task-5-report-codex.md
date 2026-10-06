# T5: PTY de escritorio y estabilización

Implementados Settle e initial_size según F27–F30, y PtySession con pty-process 0.5.3. El PTY recibe dimensiones y O_NONBLOCK antes de crear el hijo; read y write nunca esperan al descriptor. La cola acepta como máximo 1 MiB y rechaza una escritura completa si excede la capacidad; flush conserva los bytes parcialmente escritos y limita cada despacho a 64 intentos. resize usa el mismo maestro. Se exportan el descriptor, PID, tty del hijo y recogida explícita una sola vez.

spawn conserva el entorno del proceso según el contrato; spawn_with_env permite un snapshot explícito y env_clear para confinamiento. Ambos retiran TMUX, TMUX_PANE y NO_COLOR y fijan TERM, COLORTERM y VTE_VERSION. Todos los tests usan HOME sintético y entorno explícito, shells /bin/sh sin perfil y ningún agente/credencial del usuario.

Drop manda SIGHUP solamente al grupo de la sesión creada por pty-process. Un trabajador reservado antes del spawn recoge el Child: su creación puede fallar antes de que haya proceso vivo. El hilo mantiene el PID del líder sin recoger durante las escaladas TERM (250 ms) y KILL (500 ms), para que ese PID no se reutilice mientras señaliza el grupo, y después sondea try_wait hasta un segundo. El hilo principal nunca duerme ni espera al hijo. No mata servidor tmux ni sesiones: la prueba observa que solo desaparece el cliente del servidor privado. Un hijo en estado de kernel no interrumpible puede sobrevivir al plazo; el código no bloquea indefinidamente el hilo de UI.

## Evidencia

RED inicial: term_pty no compilaba por módulos pty/settle ausentes. Durante el gate de attach, la fixture original dejaba status=on (valor por defecto de tmux) y reservaba una fila: 163×44 cambiaba a 163×43. El Python aprobado configura status=off antes de attach (línea 4067 en /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/bin/cc-app). Se configuró solamente esa opción en el servidor privado de la prueba, sin alterar el puerto productivo ni ocultar un ajuste de PTY.

GREEN: term_pty 10/10 y term_paint 18/18 (28 pruebas) en 1,47 s + 0,05 s. Cubren quiet/cap y saturación numérica, fallback de tamaño, UTF8 y stty antes/después, attach sin encoger 163×44 y resize a 120×40, salida/reap código 7 una sola vez, entorno y lectura vacía, backpressure con 200.000 bytes preservados y rechazo atómico >1 MiB, y proceso que ignora HUP/TERM recogido sin afectar otro proceso privado. El Drop medido por el test debe devolver antes de 100 ms.

Clippy comandos-app all-targets -D warnings, cargo fmt --check y git diff --check pasan. Cargo se ejecutó con -j2, offline y /home/someguy/codebase/0xJesus/ComandOS/.build/target-fase4. Tmux siempre usa -S privado y -f /dev/null. No se ejecutó GTK, Xvfb ni navegador; integración GLib corresponde a T6 y fidelidad visual al gate remoto T12.

## Revisión: conservar limpieza tras observar salida

El reviewer identificó que try_reap recogía el líder y eliminaba Child, haciendo que Drop omitiera descendientes del grupo. Regresión RED reproducida con un shell privado que termina y deja otro shell del mismo PGID ignorando HUP/TERM; tras try_reap y Drop el descendiente seguía vivo (se limpió explícitamente el proceso sintético después del RED).

GREEN: try_reap usa waitid WEXITED|WNOHANG|WNOWAIT, informa el resultado una sola vez y conserva Child/PID reservado hasta entregar al reaper. Drop todavía limpia el grupo y el reaper recoge el líder después de las escaladas. Esto evita tanto el descendiente olvidado como señalizar un PGID reutilizado. term_pty 11/11 pasa, incluida la nueva regresión; clippy all-targets -D warnings, fmt y diff-check pasan. El nombre público try_reap mantiene la interfaz del plan, pero la recogida efectiva corresponde al worker de Drop, después de limpiar el grupo.
