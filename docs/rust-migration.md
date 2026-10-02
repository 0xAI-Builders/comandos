# Ejecución de la migración a Rust

La autorización de Jesús incluye todo el código propio, la lógica web en Rust/WASM y la conservación del diseño Medidor LED. Esta rama implementa la migración progresivamente; no instala sus binarios en las sesiones activas.

Base funcional: commit `214e55a`, que captura el código de trabajo además de `023ab78`. Se verificaron 976 rutas antes y después de copiar. La evidencia está en /home/someguy/codebase/0xJesus/ComandOS/.scratch/comandos-rust/evidence/isolated-base.json.

## Primer componente ejecutable

El núcleo puro de eventos conserva el contrato JSON de N1 y no tiene acceso a procesos, red o almacenamiento. Así podrá compartirse entre el servicio nativo y la interfaz WASM sin duplicar las reglas de sesión. Los efectos se inyectan: tiempo, identificador nuevo y momento de inicio del proceso. No se cambia el comportamiento para facilitar la traducción.

Archivos del componente:

- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/Cargo.toml: workspace y perfil de compilación.
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/crates/comandos-core/src/turn.rs: reducción y agrupación por panel; `reduce_turn` devuelve `None` cuando no hay cambio.
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/crates/comandos-core/src/event.rs: normalización, deduplicación y destino.
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/crates/comandos-core/src/hook.rs: normalización de hooks y resolución de bindings.
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/crates/comandos-core/src/bin/comandos-contract.rs: ejecutable JSONL de comparación; no sustituye ningún comando instalado.
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/tests/rust_contract_parity.py: compara el ejecutable con los módulos originales usando datos sintéticos, sin datos personales.
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/scripts/rust-sandbox: compilación y pruebas en namespaces separados, sin red del host, sin su directorio personal ni sus sockets.

Secuencia de implementación autorizada:

- [x] Fijar la base y comprobar que Bubblewrap crea namespaces separados.
- [x] Crear pruebas de contratos y comprobar su fallo antes de implementar.
- [x] Migrar reducción de estados y agrupación conservando eventos tardíos, duplicados, procesos ajenos y metadatos desconocidos.
- [x] Migrar normalización y deduplicación con límites Unicode y errores compatibles.
- [x] Migrar hooks y resolución exacta del panel, inyectando identidad de proceso.
- [x] Ejecutar las pruebas originales y la comparación diferencial, compilar release y revisar el código.

Verificación: invocar /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/scripts/rust-sandbox con `cargo test --workspace --locked`, y después con `python3 tests/rust_contract_parity.py`. La comparación devuelve un error ante cualquier discrepancia. Las pruebas de protección comprueban que el aislamiento no permite alcanzar el hogar real, el socket tmux ni el servicio HTTP activo. Compilación con un solo trabajo, prioridad reducida y límite de memoria virtual de 3 GiB por proceso.

## Condiciones para continuar

Conservar el código original como oráculo durante la migración; retirarlo del producto sólo cuando sus consumidores y efectos estén migrados y verificados. La biblioteca inicial no equivale a migrar la persistencia, las notificaciones ni la interfaz. La siguiente integración requiere migrar el gestor de esquema y sus copias de seguridad, el adaptador nativo de procesos/identificadores y las políticas de notificación antes de sustituir el comando de hooks. El almacenamiento actual recibe conexiones ya abiertas y migradas: no escoge rutas ni descubre sesiones por su cuenta.

El resto del alcance permanece en el mapa /home/someguy/codebase/0xJesus/ComandOS/.scratch/comandos-rust/map.md: servidor HTTP, sesiones y tmux, CLI y hooks, cuentas/proveedores, extensiones, analítica, interfaz nativa, web/WASM, PWA/terminal remota, broker, instalación y mantenimiento. No se declara paridad visual por conservar los assets: habrá que verificar el renderizado de la interfaz migrada en macmini.


## Persistencia y recepción transaccional implementadas

- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/crates/comandos-store/src/lib.rs: eventos, recepciones, deduplicación, paginación y reservas de entrega. El consumidor conserva BEGIN/COMMIT/ROLLBACK.
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/crates/comandos-store/src/marks.rs: marcas humanas, favoritos independientes, revisiones optimistas y aplicación única por evento/ámbito.
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/crates/comandos-store/src/intake.rs: normalizar, resolver el panel y guardar el evento con sus efectos sobre marcas en una transacción. Los duplicados conservan su recepción sin volver a aplicar efectos.
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/crates/comandos-store/examples/replay.rs: ejecutable de desarrollo que exige una base explícita ya migrada; no tiene ruta predeterminada ni instala hooks.

Se usa rusqlite 0.40.2 con la funcionalidad de backup y la biblioteca SQLite del sistema. Referencia primaria: https://docs.rs/rusqlite/0.40.2/rusqlite/struct.Connection.html. Los métodos Rust reciben conexión, fecha e identificadores; el adaptador nativo final deberá suministrarlos. Las marcas gráficas y animaciones del módulo original siguen pendientes de migración.

## Reproducción local

Requisitos del runner: Linux x86_64, Bubblewrap con user namespaces, toolchain Rust con destino wasm32-unknown-unknown, GCC, SQLite, Python 3.10 y uv. Se verificó Rust 1.96.0. Este runner de desarrollo no es un instalador multiplataforma.

Preparar dependencias sólo dentro de la copia aislada:

```sh
/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/scripts/rust-prepare
```

Ejecutar todas las comprobaciones:

```sh
/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/scripts/rust-check
```

La preparación descarga dependencias sin ejecutar compilaciones y coloca el oráculo Python en /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/.migration-build/python. Compilación y pruebas se ejecutan después sin red, con hogar y runtime vacíos. El árbol de código queda montado como /work dentro del namespace; ésa no es una ruta del host. Si falta el alias de desarrollo de SQLite, se crea uno únicamente en la caché aislada. Ninguna dependencia se instala en el entorno activo de ComandOS.

## Evidencia del 2 de octubre de 2026

La ejecución completa terminó con código 0:

- 29 pruebas Rust, incluidas ocho recepciones concurrentes del mismo evento y rollback después de actualizar el primer ámbito.
- 52 pruebas originales Python de estados, eventos y marcas.
- 7,928 casos diferenciales del núcleo con igualdad de resultados frente a Python.
- 54 pasos del oráculo SQLite, comparando datos, recepciones, marcas, aplicaciones y estado de transacción.
- Interoperabilidad sobre el esquema completo creado por Python: Rust escribe seis recepciones de prueba y Python verifica resultados, integridad y claves foráneas.
- Rustfmt y Clippy sin advertencias; compilaciones debug y release nativas y de la biblioteca pura para wasm32-unknown-unknown.

Log completo: /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/.migration-build/validation.txt.

La revisión independiente encontró una pérdida de precisión al comparar timestamps enteros grandes con flotantes. Se reprodujo con una prueba fallida y se corrigió; se añadieron 48 casos diferenciales y una prueba Rust. La revisión posterior del almacenamiento no encontró defectos críticos ni importantes. También se reforzó la prueba de fallo parcial de consumidores.

Las 976 rutas de la fuente original seguían iguales a sus hashes iniciales al finalizar la verificación. El servicio original seguía activo con el mismo PID 1610034. No se reinició, desplegó ni conectó un componente nuevo con sesiones reales.

### Medición acotada del núcleo

Carga sintética: 2,000 solicitudes JSONL, cada una agrupa 16 eventos; cinco procesos nuevos por implementación. Ambas producen los mismos resultados. Medianas medidas con GNU time:

| Métrica | Python | Rust release |
|---|---:|---:|
| CPU | 0.15 s | 0.07 s |
| Tiempo transcurrido | 0.15 s | 0.08 s |
| Pico RSS | 9,632 KiB | 1,792 KiB |

El ejecutable de contratos mide 533,392 bytes. Los datos de todas las corridas están en /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/.migration-build/core-benchmark.json. El comparador reproducible está en /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/tests/rust_core_benchmark.py.

Estas cifras describen el núcleo puro y su adaptador JSONL. No demuestran rendimiento del servidor, la interfaz, el almacenamiento o la aplicación completa; tampoco establecen un presupuesto de producción.

### Límites de compatibilidad explícitos

Los timestamps de entrada de esta frontera JSON se representan como enteros de 64 bits o flotantes finitos; SQLite exige enteros con signo de 64 bits para persistirlos. Python puede construir enteros mayores en memoria, que tampoco puede guardar en esas columnas. El adaptador HTTP final debe mantener una respuesta de validación consistente para entradas fuera de rango. El resto del contrato observado, incluidos identificadores Unicode, valores nulos, orden por secuencia y metadatos desconocidos en el estado, está cubierto por las pruebas descritas.

El proyecto completo aún no está migrado. Este bloque no sustituye el servidor HTTP, los administradores de sesión/tmux, los clientes GTK/macOS, la web/WASM, las notificaciones, el broker remoto ni los instaladores. No se eliminó código original ni se introdujo un puente Python/Rust en producción.


## Segundo bloque: estado, hooks nativos y notificaciones

Implementaciones añadidas en /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/crates/comandos-runtime, /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/crates/comandos-store/src/state_db, /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/crates/comandos-core/src/notifications.rs y /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/crates/comandos-store/src/notifications.rs.

El ejecutable comandos-events implementa record, claim-local, migrate e import-legacy sin invocar Python. Conserva identidad del proceso, deduplicación, reserva única de sonido, importación histórica sin enlazarla a paneles y las once migraciones existentes. Los backups son privados y no se sobrescriben si dos migraciones ocurren en el mismo segundo. Una migración no puede iniciarse dentro de una transacción ajena; rechazarla evita un bloqueo y conserva esa transacción. La revisión independiente del bloque se registra por separado antes de declararlo terminado.

Verificación completa posterior a la recuperación: 51 pruebas Rust; 90 pruebas originales Python (se excluye el instalador porque no corresponde a esta frontera); 7,928 casos del núcleo; 54 operaciones del oráculo SQLite; 6,600 rutas de notificación, 16 preferencias y 9 avisos; 571 operaciones de notificaciones persistidas; igualdad exacta del esquema SQL Python/Rust; upgrade y backup leídos por Python; hooks y recepción concurrente. Formato, Clippy, debug, release y biblioteca WASM pasan. Evidencia: /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/.migration-build/validation-block2.txt.

El runner usa ahora además un scope systemd independiente: MemoryHigh=2G, MemoryMax=3G y MemorySwapMax=512M para todo el árbol de compilación/pruebas. El límite por proceso y Bubblewrap permanecen. No debe confundirse este límite con la memoria total de los agentes o del sistema.

El incidente de memoria fue atendido con autorización posterior de Jesús. Las únicas correcciones desplegadas son preservación de parámetros de reanudación, arranque atómico de tmux, caché de medición MCP compartida y tokenizador en proceso transitorio. El reinicio de la GUI conservó los 28 PID de panel. Los proxies ya abiertos conservan el código/tokenizador cargado hasta que reconecten; no se cortaron masivamente sus conexiones. La GUI recuperada tiene límite de 2 GiB; no limita el resto de agentes y MCP. La causa demostrada del consumo es la multiplicación de conectores y tokenizadores; no se ha demostrado todavía una fuga creciente dentro de cada conector externo.

Inventario de código/estilo propio y oráculos: /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/docs/rust-component-inventory.json y /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/docs/rust-component-inventory.md. No está completa la migración ni se ha hecho el corte al producto Rust.
