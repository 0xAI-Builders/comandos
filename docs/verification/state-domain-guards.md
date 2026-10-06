# Guardia durable de dominios sellados

`mode_of(None, dominio)` devuelve `Legacy`, pero los handles consultan un guardia persistente antes de acceder a un archivo. Sin esa segunda comprobación, perder la conexión de un dominio sellado permitiría recrear estado legado. La decisión conserva la interfaz del plan y hace que la ausencia de conexión produzca un error si el dominio tiene un guardia.

La implementación y sus pruebas privadas están en:

- `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-state-domains/crates/comandos-store/src/unified/modes.rs`
- `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-state-domains/crates/comandos-store/tests/domains.rs`

## Orden y recuperación

El nombre del guardia se deriva de la ruta de la base y añade el sufijo `.sealed-<dominio>`. Con conexión se usa la ruta de SQLite; sin conexión se usa `unified_path(home)`, incluido `COMANDOS_DB`. El guardia debe conservarse junto a la base cuando se cambia su ubicación. No se migra como contenido de un dominio.

`set_mode` toma el candado de modos de esa base, abre su propia transacción `IMMEDIATE`, sincroniza el guardia antes de confirmar `sealed` y registra modo, autor y fecha en `domain_modes`. Solo elimina el guardia después de confirmar un cambio explícito a otro modo. Rechaza una transacción del llamador y rechaza sellar una base en memoria.

Un fallo después de crear el guardia puede dejar la fila sin sellar. Un corte después de confirmar el desellado puede dejar el guardia presente. Los dos casos bloquean los handles hasta que un cambio explícito de modo confirme la recuperación. Un guardia corrupto, ilegible o con un tipo de archivo incorrecto también bloquea el acceso. Esta decisión puede detener una operación que sería recuperable, pero evita elegir archivos sin evidencia suficiente.

Los handles toman el mismo candado de modos antes del `flock` del archivo legado y lo conservan hasta terminar. Las escrituras `mirror` confirman archivo y después fila bajo ese `flock`; las de `unified` confirman fila y después archivo. `sealed` escribe solo la base. Los handles requieren conexiones sin transacciones del llamador y validan esquema, directorio privado y permisos de la base antes del acceso. Una fila ausente en una base disponible no provoca una lectura de archivo.

Las pruebas cubren sellado fallido, guardia corrupto, desellado interrumpido, cambios de modo concurrentes, el candado legado existente, transacciones revertidas del llamador y pérdida de base con ruta por defecto o `COMANDOS_DB`. Los cortes se simulan con fallos SQL y estados persistidos, sin matar procesos.

## Controles e instalación privada

El inventario clasifica la base de destino, WAL, SHM, journal, candado de modos y guardias como `metadatos-control`, incluso si una ruta configurada coincide con un documento o queda fuera de las raíces habituales. El clasificador está en `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-state-domains/crates/comandos-store/src/domains/catalog.rs`; su verificación está en `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-state-domains/crates/comandos-cli/tests/state_inventory.rs`.

El preflight bloquea errores de lectura y escritores ambiguos. Acepta exactamente protocolo 2; un protocolo futuro requiere revisión del contrato. Los escritores Python conocidos conservan el alcance de su dominio. El manifiesto de capacidad se sincroniza con permisos `0644` antes del binario final. Una release existente sin capacidad compatible no se vuelve a etiquetar.

Estos binarios permanecen privados. Declarar protocolo 2 todavía no habilita publicar: faltan la migración de consumidores y las puertas de ciclo de vida del controlador.
