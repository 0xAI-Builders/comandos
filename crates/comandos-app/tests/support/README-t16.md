Las pruebas de T16 ejecutan contratos y cuerpos reales sin inicializar GTK, consultar procesos personales ni tocar el portapapeles del escritorio. La aceptación GUI queda pendiente en la VM de raíz.

El worktree aislado es `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-gtk-next`. Las referencias originales son `/home/someguy/codebase/0xJesus/ComandOS/bin/cc-app` (SHA256 `1b535bc6f4463959cc3ab992fa5a4ab859fbdaec43fcf628123cfede68b4ca6b`) y `/home/someguy/codebase/0xJesus/ComandOS/lib/tmux_clipboard.py` (SHA256 `45cee4b0639f98364e8492a9f1196e9d0bc746ee129409dc640f543c693f2a73`). Se extraen funciones mediante AST; no se importa ni ejecuta el arranque del original.

`/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-gtk-next/crates/comandos-app/tests/support/t16_original.py` calibra menús ES/EN locales/SSH, filtro y CRUD con campos desconocidos, paste por buffer propio/stdin, watermark numérico, reply, paths, popover, notify y geometría/release. `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-gtk-next/crates/comandos-app/tests/support/t15_adapter.rs` extrae los cuerpos de dispatch, replay, copy/paste, selection, feedback, captura de clic y teardown de producción. Los colaboradores de GTK, HTTP, Jobs, PTY y Clipboard se inyectan como puertos; el motor de selección/encode_paste y los cuerpos de negocio siguen siendo reales. El probe T14 en `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-gtk-next/crates/comandos-app/tests/support/t14_adapter.rs` comprueba también el guard del menú hospedado por T16.

Los tests nativos de tmux usan un ejecutable falso que exige `-S` con socket privado y registra argv/stdin; no arrancan servidor tmux. El test hijo sin variable de fixture retorna inmediatamente: su pass se informa como helper, no como caso funcional extra. Los HOME, XDG y TMP viven bajo `/tmp/gtk-t16-private-20261006`, fuera de Git, con los directorios de entorno 0700; `/usr/share/mime` está copiado en el XDG privado.

Para reproducir en este entorno aprobado:

```sh
cd /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-gtk-next
python3 /tmp/gtk-t16-private-20261006/run.py review-t16 nice -n 10 cargo test --offline -j 2 -p comandos-app --test clipboard_bridge --test snippets --test menu_actions --test keys --test app_commands --test term_view_model --test snapshot_persistence --test theme --test header_models
python3 /tmp/gtk-t16-private-20261006/run.py review-engine nice -n 10 cargo test --offline -j 2 -p comandos-term --test input --test select
python3 /tmp/gtk-t16-private-20261006/run.py review-lint nice -n 10 cargo clippy --offline -j 2 -p comandos-app -p comandos-store --all-targets -- -D warnings
python3 /tmp/gtk-t16-private-20261006/run.py review-build nice -n 10 cargo build --offline -j 2 -p comandos-app --bin comandos-app
python3 /tmp/gtk-t16-private-20261006/run.py review-fmt cargo fmt --package comandos-app --package comandos-store --check
```

El runner y su entorno completo quedan en `/tmp/gtk-t16-private-20261006/run.py` y `/tmp/gtk-t16-private-20261006/evidence/env.json`; el manifiesto de comandos está en `/tmp/gtk-t16-private-20261006/evidence/commands.jsonl`. No se deben ejecutar aquí suites que inicialicen GTK o el helper de tmux real.

Lecturas de snippets usan DocHandle.read_readonly. El writer de compatibilidad sólo actúa en Legacy, con CAS sobre el documento JSON Python y guard antes de publicar. Mirror/Unified/Sealed y MOVED fallan de forma visible; la escritura por facade S5a sigue pendiente. El helper de autoridad reutiliza el protocolo readonly existente, sin crear controlfiles: no añade un protocolo G0 nuevo ni afirma linearización frente a publicadores que omitan los leases existentes.

Siguen diferidos los consumidores de T17/T18, incluidos start_ai_here, overlays y cached pane frames; el menú conserva su acción con MissingConsumer explícito. Pendientes remotos T16: clipboard_unicode_bracketed_paste, snippet_crud_and_send, terminal_context_menu_all_actions y pixel_gate_snippets_menu. Los checks locales no acreditan GUI, paridad visual completa, font/theme gate ni permanencia/RSS de una hora.
