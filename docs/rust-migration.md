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

## Tercer bloque: adaptador MCP nativo

El ejecutable comandos-extensions implementa la frontera serve en /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/crates/comandos-extensions. La ejecución stdio sin filtros sustituye el proceso mediante exec. Los caminos con filtros, HTTP y SSE heredado conservan llamadas, paginación, errores y campos adicionales. La autenticación nativa mantiene la renovación compartida con bloqueo entre procesos y la sincronización de los registros MCP de origen. No instala el ejecutable ni cambia conexiones activas.

La implementación usa JSON con reqwest y Tokio. La elección permite conservar SSE heredado, que el SDK rmcp actual ya retiró. Las credenciales no siguen redirecciones ni endpoints SSE de otro origen. La salida estándar del ejecutable es exclusivamente MCP. Los mensajes de diagnóstico propios no muestran URLs, secretos ni respuestas privadas.

El cierre cancela solicitudes pendientes y recoge el proceso hijo; una renovación OAuth que ya empezó se guarda antes de terminar. La espera de un bloqueo externo tiene límite de veinte segundos. Los cuerpos de protocolo tienen límite de 8 MiB, con sesenta y cuatro solicitudes simultáneas y colas de entrada/salida de ocho elementos. Estos límites acotan el crecimiento, pero varias respuestas máximas concurrentes aún pueden consumir cientos de MiB. Las mediciones pequeñas de RSS no representan ese caso extremo.

La lectura de configuración admite JSONC y rechaza claves duplicadas también dentro de objetos anidados. Las escrituras de credenciales son atómicas y privadas, conservan campos desconocidos y guardan copias previas de las fuentes modificadas. La comprobación previa a reemplazar fuentes conserva el contrato Python; no constituye una operación atómica compare-and-swap frente a escritores externos que ignoran el bloqueo.

La generación nativa del conteo de tokens queda para el siguiente bloque. No se fabrican valores ni se carga un tokenizador en el proxy. La importación y sincronización del catálogo, las comprobaciones de acceso a proveedores y los restantes comandos de extensiones también siguen pendientes. El adaptador no debe reemplazar todavía el comando instalado completo.

El runner de Rust monta un usuario sintético y los certificados públicos del sistema. El runner de referencia /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/scripts/rust-python-oracle recibe explícitamente el entorno Python y la caché pública del tokenizador, ambos de sólo lectura; conserva red/hogar privados y el límite de grupo de 3 GiB. Los datos de cuentas no se montan.

Medición del proxy release sobre un servidor HTTP sintético compartido: Rust 5,256 KiB de RSS al iniciar, después de 100 llamadas y después de listar herramientas. Python registró 57,480, 57,784 y 57,896 KiB, respectivamente. El ejecutable Rust mide 5,892,288 bytes. Esta comparación excluye el servidor de prueba y el pico transitorio del tokenizador Python; Rust todavía no ejecuta la medición de tokens de herramientas en este bloque. El ensayo deberá repetirse al conectar el contador Rust.

La autenticación también pasó tres pruebas de intercambio HTTPS real con una autoridad certificadora temporal: descubrimiento OAuth, POST de renovación con secreto básico o formulario, reintento de 401 y rechazo de redirecciones. Las pruebas mantienen verificación TLS activa y sólo contactan al servidor sintético dentro del namespace.

Verificación completa del bloque: 67 pruebas Rust, 39 pruebas de protocolo/HTTPS del ejecutable, 90 pruebas originales Python y los oráculos del núcleo, almacenamiento, notificaciones, esquema, hooks y backups siguen pasando. Formato, Clippy, debug, release y núcleo WASM pasan. Log: /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/.migration-build/validation-block3.txt. La revisión independiente del bloque se registra antes de marcarlo cerrado.

La revisión del tercer bloque quedó aprobada en bd9e195, con cero hallazgos pendientes. Se corrigieron la adición accidental de result a errores tools/list, el reloj obsoleto tras esperar bloqueo OAuth y la prueba de liberación de capacidad al cancelar una solicitud. La validación de esa corrección pasó 18 pruebas Rust y 43 de protocolo/HTTPS, más formato, Clippy y compilaciones debug/release.


## Cuarto bloque: metadatos y contador transitorio

Implementación en /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/crates/comandos-extensions/src/metadata.rs, /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/crates/comandos-extensions/src/tokenizer.rs y /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/crates/comandos-extensions/src/python_json.rs. El proxy recoge únicamente listas filtradas y completas de herramientas. Un hijo Rust calcula cl100k_base con datos públicos previamente calentados y verificados por SHA256; nunca descarga tablas durante inventarios y no introduce dependencia Python de ejecución. El proceso padre conserva únicamente cachés acotadas y metadatos. La validación de procedencia mantiene la semántica Unicode 14 del oráculo Python 3.11, incluida la resolución de rutas y alias de instalación.

Las mediciones se comparten mediante caché en disco y bloqueo por servidor. El contador tiene plazo de ocho segundos y dos cupos compartidos por hogar antes de cargar las tablas. Los hijos deben terminar y recogerse al cancelar, expirar o destruir el runtime; los cupos se mantienen también cuando la salida de un hijo se bloquea. Las escrituras son privadas y atómicas, y los errores devuelven tamaño desconocido sin sustituir un registro válido por un conteo inventado. Nunca se guardan definiciones, credenciales ni resultados de herramientas en estos registros.

La serialización canónica compara hashes, texto y longitudes con 9,999 casos Python. Conserva enteros grandes, identidad de flotantes finitos y claves opacas que coinciden con nombres internos de serde_json. Las cuatro fronteras de transporte usan el parser compartido. Los números no finitos producen metadatos desconocidos; UTF8 inválido y sustitutos Unicode sueltos continúan rechazados en la frontera Rust.

Validación completa antes de las últimas correcciones de procedencia y duración del cupo: 89 pruebas Rust, 54 de protocolo/HTTPS/metadatos, 90 originales, todos los oráculos anteriores, formato, Clippy, debug, release y WASM. Log /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/.migration-build/validation-block4-final.txt. Las correcciones finales, sus pruebas enfocadas y la medición de memoria se registran en /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/.superpowers/sdd/mcp-metadata-report.md antes de la revisión independiente. Este bloque no instala Rust ni reconecta proxies existentes.

Para reproducir los conteos es necesario preparar explícitamente la caché pública en el entorno aislado. El generador /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/crates/comandos-extensions/tests/metadata_oracle.py admite --prepare-cache para comprobar la fixture y copiar únicamente el archivo público verificado; modificar la fixture requiere --write. Se ejecuta con /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/scripts/rust-python-oracle y rutas explícitas al Python de referencia y a la caché pública. El siguiente bloque aborda importación, catálogo y sincronización; comprobación de acceso e instalación siguen pendientes.

La validación final enfocada pasó 37 pruebas Rust de extensiones, ocho pruebas de metadatos/protocolo/procesos, formato del workspace, Clippy con advertencias rechazadas y compilación release. Incluye comparar 1,112,064 valores escalares Unicode contra el oráculo de procedencia, así como cierre del runtime, salida bloqueada, rutas con ciclos de enlaces y coincidencia del conteo para texto Unicode 15/16.

RSS release de la versión corregida b3e0891: el padre usó 6,320 KiB al iniciar y 6,332 KiB tanto después de una lista completa como después de cien. Pico del hijo: 29,600 KiB. Con seis servidores distintos en el mismo hogar, se observaron como máximo dos tokenizadores cargados, 73,404 KiB de pico agregado de hijos y 37,900 KiB de padres. Los conteos persistidos coinciden con Python. El ejecutable mide 7,985,392 bytes. Son mediciones de fixtures concretas y no cotas de memoria para respuestas MCP máximas ni de los servidores externos. No quedó ningún contador vivo al terminar la prueba.

La revisión independiente encontró dos fallos en el candidato inicial: diferencias de redondeo decimal en empates exactos y crecimiento de los cursores fuera del presupuesto de herramientas. La corrección b3e0891 genera los dígitos con el formateador mantenido de serde_json y amplía el oráculo a 22,589 casos, incluidas potencias binarias y sus valores IEEE adyacentes. El estado de cursores tiene ahora un presupuesto de 65,536 bytes y 1,024 entradas, contando el cursor esperado; se libera al terminar. Agotar este presupuesto invalida sólo la captura de metadatos y no cambia la respuesta MCP. Después de corregir pasan 41 pruebas Rust de extensiones, 56 de protocolo/metadatos, el oráculo ampliado, formato, Clippy y compilaciones debug/release. Reporte: /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/.superpowers/sdd/mcp-metadata-fix-report.md.

El cuarto bloque quedó aprobado tras la segunda revisión en b3e0891, sin hallazgos Critical, Important ni Minor pendientes. El catálogo, los consumidores, la instalación y la paridad de las interfaces continúan como hitos separados; esta aprobación no autoriza sustituir todavía el comando instalado completo.

## Quinto bloque: catálogo, credenciales y skills

Los comandos import, sync y status ya tienen implementación Rust revisada en /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/crates/comandos-extensions. Conservan precedencia de importación, alias, cuentas nuevas, opciones propias de los clientes, conflictos entre cambios nativos y del catálogo, comentarios y tipos TOML, JSONC, políticas al desactivar/reactivar servidores y árboles completos de skills. La importación de credenciales copia sólo registros MCP que coinciden con el endpoint configurado y comparte el bloqueo de renovación OAuth. Las fechas de Grok se contrastaron con Python en cuatro zonas horarias, incluidos cambios de horario y el día omitido de Samoa.

Las configuraciones se comparan antes y después del backup. Una copia existente incompleta o enlazada provoca un error sin sobrescribirla. Los árboles de skills se preparan antes de sustituir el destino y se mueven con NOREPLACE; un fallo conserva el contenido desplazado. Esto protege frente a las condiciones probadas, pero no constituye un CAS atómico contra otros programas que escriban sin coordinarse. Los fingerprints de enlaces a recursos conservan la semántica original: no demuestran que el contenido externo apuntado permaneciera inmutable durante una copia.

El candidato 2e7955a pasó 127 pruebas Rust, 58 de protocolo, 90 originales y todos los oráculos anteriores, formato, Clippy y compilaciones debug/release/WASM. La revisión encontró una incompatibilidad con comillas dentro de comentarios JSONC cuando también había valores no finitos. La corrección 0a290db reutiliza el lector léxico común y pasó 27 pruebas de catálogo, 16 de autenticación, siete de importación, 42 de serve y los oráculos de referencia: diez etapas del catálogo, 23 casos TOML, tres variantes JSONC y 160 casos de credenciales. La segunda revisión quedó aprobada sin hallazgos pendientes. Las 20 funciones originales de catálogo, con 22 casos parametrizados, están trazadas a las pruebas nativas.

Evidencia completa: /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/.migration-build/validation-block5-final.txt y /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/.superpowers/sdd/extension-catalog-report.md. Reproducción de referencias: /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/scripts/rust-reference-check recibe las rutas absolutas del entorno Python de comparación y la caché pública, que monta en modo lectura dentro del entorno aislado.

Con 1,000 servidores ficticios y doce cuentas, cinco ejecuciones de status produjeron el mismo resultado: Python tuvo mediana de 418.38 ms y pico RSS de 53,892 KiB; Rust release, 8.77 ms y 6,720 KiB. La medición corresponde al candidato 2e7955a y a ese comando, no a la memoria total de ComandOS. Datos: /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/.migration-build/catalog-rss.json. El bloque no está desplegado; siguen pendientes check, instalación y las demás superficies del inventario.
