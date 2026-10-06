# Migración privada de fuentes de archivo

S3 implementa respaldo, journal, relleno y consulta. No traslada ni respalda bases SQLite fuente; esa operación y la estimación de uso pertenecen a S4. `usage_move_estimate_ms` permanece nulo. El controlador conserva la ejecución sobre datos vivos y las puertas de publicación.

Implementación: `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-state-domains/crates/comandos-store/src/migrate/`. CLI: `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-state-domains/crates/comandos-cli/src/state/cli.rs`. Fixtures privados: `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-state-domains/crates/comandos-store/tests/migrate.rs` y `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-state-domains/crates/comandos-cli/tests/state_cli.rs`.

## Respaldo y reanudación

El migrador reúne solo fuentes del catálogo, excluyendo destino y controles por ruta normalizada y por identidad de archivo regular. Vuelve a contrastar esas identidades antes y después de cada lectura: sustituir una fuente por un hardlink del destino después del inventario produce error. El nuevo `<db>.migration.lock` también es control. Los enlaces de fuente y sus padres no se siguen. Cada copia toma el flock existente `<fuente>.lock`, conserva bytes y registra origen absoluto, hash, tamaño, modo y mtime nanosegundo. Las copias se crean exclusivas con 0600 dentro del run 0700; no se sobrescriben. El manifiesto se publica al final, se sincroniza y se verifica antes de preparar Mirror.

El journal confirma cada fuente y sus filas en la misma transacción. Un fallo deja el run `running`; `--resume` elige el último run abierto y verifica su respaldo antes del relleno. Saltar un hash ya confirmado no altera su recibo. Una fuente cambiada vuelve a pasar por la guarda mtime. Una fuente nueva obtiene respaldo y manifiesto confirmado antes de importarse. Si el corte dejó una copia sin registrar, se conserva y la nueva copia usa otro nombre exclusivo. Un respaldo inicial sin manifiesto no se adopta: queda disponible para inspección y una nueva ejecución usa otro run.

Una reanudación parcial no marca el run `done` hasta cubrir las fuentes del manifiesto. Un dominio seleccionado aunque esté vacío se prepara para futuras escrituras Mirror. La preparación toma el candado de modos y cambia únicamente Legacy. Unified y Sealed conservan autoridad; los guardias durables se validan y nunca se eliminan desde S3.

## Importación y verificación

Documentos JSON, texto y flags se conservan como bytes exactos con las claves `hooks/…`, `state/…` o `share/…`. Estados y procesos usan claves de archivo y PID, con guardas mtime. Layout conserva current/previous y minutos de los últimos siete días; los antiguos quedan en respaldo. Las órdenes consumidas no reviven por repetir una importación con el mismo hash; el journal identifica la orden importada cuando debe sustituirse por una fuente posterior. No se elimina una orden del espejo para importar otra más vieja.

El relleno JSONL solo corre sobre un registro vacío. Ignora y registra una última línea sin LF. La verificación la señala como diferencia: no cuenta como verificación limpia ni supone que esos bytes ya se importaron. Un registro que ya tiene líneas de espejo tampoco se reimporta; si falta historia, verify lo muestra. Verify compara bytes y claves, incluidos registros, snapshots u órdenes que quedaron sin archivo. El CLI registra los resultados con `domain='verify:<dominio>'`, fecha y lista de diferencias, sin implementar todavía las puertas de S6.

## Ensayo y consulta sin escrituras en HOME

Dry-run y status nunca abren el SQLite fuente. Ignoran TMPDIR y eligen un scratch 0700 cuyo padre físico está fuera del HOME inspeccionado. Si existe un candado de modos, lo abren en solo lectura y lo retienen. Ese candado protege escritores capaces; no se presume que proteja cualquier escritor SQLite.

DB y WAL se leen como bytes estables, se copian a staging y vuelven a contrastarse por dispositivo/inodo, tamaño, mtime, presencia y hash. La aparición o desaparición de WAL/journal, una sustitución de entrada o un journal activo producen error. La API SQLite Backup copia el staging a la base temporal del ensayo, conservando los commits del WAL. La fuente nunca crea SHM. Base futura, privacidad insegura y sellado sin base fallan antes de crear estado en HOME. Se borra únicamente el scratch generado por la operación.

Estos controles detectan cambios observables durante staging; no prometen una instantánea coordinada de todos los archivos y una base frente a escritores antiguos sin candado. Una fuente inestable se rechaza y la operación debe repetirse. Verify y sus puertas posteriores siguen siendo necesarios antes de cambiar autoridad.
