# Referencias de controles de sesión

El trabajo mejora controles funcionales de ComandOS y su uso desde un teléfono. La implementación conserva la tipografía, los colores y los componentes existentes.

Mobbin no aparece entre las herramientas conectadas de esta sesión. No se consultaron pantallas de Mobbin ni se atribuyen decisiones a referencias de esa plataforma. No se generaron propuestas visuales alternativas: se implementaron los controles sobre la interfaz existente y se verificaron con pruebas aisladas de navegador.

| Fuente consultada | Qué se revisó | Uso |
|---|---|---|
| [Interfaz existente](../dash/index.html) | Tema, selector, tarjetas, chat y formulario de inicio. | Sí: reutilización de estilos y flujos. |
| [Terminal existente](../dash/term.html) | Gestos, entrada, historial y barra de acciones. | Sí: selección nativa y compositor sobre el terminal existente. |
| [SQLite: usos apropiados](https://www.sqlite.org/whentouse.html) | Almacenamiento local de una aplicación. | Sí: decisión de persistencia; no es una referencia visual. |
| [SQLite: WAL](https://www.sqlite.org/wal.html) | Concurrencia de lectura y escritura. | Sí: persistencia local; no es una referencia visual. |

La dirección aplicada es un selector con un solo envío, tarjetas para comparar sesiones y controles táctiles accesibles. Un mosaico que renderice todas las terminales no se usa para el resumen, para evitar trabajo de renderizado innecesario.

Los archivos del trabajo visual son `DESIGN.md`, `proof.html` y `shots/`. Las capturas muestran datos de prueba; no son evidencia de validación en un teléfono físico.
