# Empaquetado GTK de T20: cutover pendiente

Este cambio implementa la API de empaquetado de T20. La aceptación completa de
T20 y el cutover live siguen pendientes de revisión independiente y de las
puertas remotas del plan. No instala servicios ni ejecuta la App durante staging.

Base: `90f7c643f9fac12c4d632e542b1c97a04712ef2d`. Fuente del plan:
`/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-integration-acp/docs/superpowers/plans/2026-10-04-fase-4-app-gtk.md`.
Implementación:
`/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-install-app/crates/comandos-cli/src/install.rs`
y
`/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-install-app/crates/comandos-cli/src/install/release.rs`.

## Contrato de preparación

`install --stage-app RUTA_ABSOLUTA` requiere un archivo regular, legible y
ejecutable. No sigue symlinks del candidato ni de sus padres. Copia los bytes
leídos a una release inmutable, con permisos 0755. Usa el layout existente:
`HOME/.local/share/comandos/releases/<id>/comandos-app`; el puntero de candidato
es `HOME/.local/share/comandos/bin/comandos-app`, con destino relativo dentro del
mismo árbol de releases. HOME representa el directorio absoluto seleccionado
por `--home`, o por el entorno; no designa aquí una ruta personal concreta.

El id es los primeros doce dígitos de SHA256 de `comandos-app\0` seguido de los
bytes del candidato. El manifiesto contiene exclusivamente `state_protocol: 2`,
`artifact_version: 1`, `artifact: "comandos-app"` y el SHA256 completo del binario.
Se comprueban versión, tipo, hash e id antes de enlazar; un manifiesto desconocido,
alterado, ausente o symlink se rechaza. Reinstalar desde una release App exige
verificar también su manifiesto e id originales.

`--link cc-app` requiere ese candidato App validado y apunta directamente al
artefacto inmutable de su release. Nunca utiliza el binario CLI `comandos`.
Preparar una segunda App cambia sólo el puntero de candidato: la App live sigue
en la release anterior hasta un `--link cc-app` autorizado expresamente.

El primer estado de cc-app queda en el registro de rollback existente. Un archivo
regular conserva sus bytes y modo en el respaldo, y conserva el inodo cuando
el traslado ocurre en el mismo sistema de archivos; un symlink conserva los
bytes de su destino, incluso si es relativo o está roto; un destino ausente vuelve
a estar ausente. Actualizar el enlace entre releases App conserva el primer
registro. Se rechazan colisiones del respaldo, registros symlink o malformados y
cambios externos incompatibles, sin sobrescribir el original.

## Dry-run y separación de rollback

`--dry-run` admite staging CLI/App, enlace, rollback por nombre y rollback de
release CLI. Hace preflight sólo de lectura y muestra acciones, destino y ruta
absoluta del registro o marcador de rollback; no crea padres, enlaces, backups,
locks, archivos temporales ni servicios. `--releases` ya es de sólo lectura.
`--web` conserva su contrato: sólo acompaña a `--stage`, nunca a `--stage-app`.

`--rollback cc-app` restaura el primer original de cc-app. `--rollback-release`
conserva el contrato anterior: intercambia exclusivamente releases CLI y su
marcador `previous`; no activa App ni cambia cc-app. El prune CLI conserva su
límite anterior y excluye releases App completas cuyo manifiesto y hash son
válidos. Las releases App no se podan en esta entrega, para conservar artefactos
referenciados por cc-app y por su rollback.

## Evidencia y límites

Las pruebas usan candidatos de bytes pequeños que nunca se ejecutan, HOME y
XDG privados, TMP privado y PATH falso. Cubren dry-run sin cambios de bytes,
inodos, modo o mtime; enlace exclusivo a App; actualización sin cutover;
rollback del primer original; colisiones; symlinks; manifiestos desconocidos;
tampering; independencia del rollback CLI y regresión del staging web.

El informe con pin y comandos está en
`/home/someguy/codebase/0xJesus/ComandOS/.scratch/takeover-20261005/install-app-report.md`.
No sustituye la matriz AST completa, pixel gates remotas, rendimiento/memoria,
restore/IPC/bridge/cuentas reales, revisión independiente, build release
reproducible, ni autorización concreta de cutover. No se tocó cc-notifyd ni el
main/dispatcher; tampoco se añadió otra API de instalador pendiente I1/I2.
